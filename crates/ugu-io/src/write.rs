// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! Writes a document as `.ugu2`.
//!
//! The output depends only on the document: entries come in a fixed order
//! with a fixed timestamp, and stored data is written in id order. Only data
//! that operations refer to is written; the rest is kept in memory for undo.

use std::collections::BTreeSet;
use std::io::{Seek, Write};

use ugu_core::document::{Document, Layer, LayerKind};
use ugu_core::ops::{AssetId, Blend, MaskId, MotionStyle, Op, Sampling, StrokeId, Wobble};
use ugu_core::store::{BrushEngine, Mask, Point};
use zip::CompressionMethod;
use zip::write::{SimpleFileOptions, ZipWriter};

use crate::format::*;

#[derive(Debug)]
pub enum WriteError {
    Io(std::io::Error),
    Zip(zip::result::ZipError),
    Json(serde_json::Error),
}

impl std::fmt::Display for WriteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(f, "cannot write the file: {error}"),
            Self::Zip(error) => write!(f, "cannot write the file: {error}"),
            Self::Json(error) => write!(f, "cannot encode the document: {error}"),
        }
    }
}

impl std::error::Error for WriteError {}

impl From<std::io::Error> for WriteError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<zip::result::ZipError> for WriteError {
    fn from(error: zip::result::ZipError) -> Self {
        Self::Zip(error)
    }
}

/// Writes `document` to `out` and returns `out`. `document_id` names the
/// document across saves, for recovery.
pub fn write<W: Write + Seek>(
    document: &Document,
    document_id: [u8; 16],
    out: W,
) -> Result<W, WriteError> {
    let mut used = Used::default();
    used.layers(&document.layers);

    let manifest = Manifest {
        format: FORMAT.to_owned(),
        schema: SCHEMA,
        render_revision: RENDER_REVISION,
        document_id: hex(&document_id),
        required: Vec::new(),
    };
    let dto = DocumentDto {
        canvas: document.canvas,
        background: color_hex(document.background.0),
        frames: document.frames,
        frames_per_second: document.frames_per_second,
        wobble: wobble(document.wobble),
        layers: document.layers.iter().map(layer).collect(),
        strokes: used
            .strokes
            .iter()
            .map(|id| {
                let stroke = &document.store.strokes[id];
                StrokeDto {
                    id: id.0,
                    color: color_hex(stroke.color.0),
                    width: stroke.width,
                    brush: BrushDto {
                        engine: match stroke.brush.engine {
                            BrushEngine::Line => EngineDto::Line,
                            BrushEngine::Airbrush => EngineDto::Airbrush,
                            BrushEngine::Spray => EngineDto::Spray,
                        },
                        opacity: stroke.brush.opacity,
                        hardness: stroke.brush.hardness,
                        antialias: stroke.brush.antialias,
                        size_dynamics: stroke.brush.size_dynamics,
                        wobble_scale: stroke.brush.wobble_scale,
                    },
                    seed: format!("{:016x}", stroke.seed),
                }
            })
            .collect(),
        masks: used.masks.iter().map(|id| MaskDto { id: id.0 }).collect(),
        images: used
            .assets
            .iter()
            .map(|id| ImageDto {
                id: hex(&id.0),
                size: document.store.assets[id].size,
            })
            .collect(),
    };

    let deflated = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    // PNG is already compressed.
    let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    let mut zip = ZipWriter::new(out);
    zip.start_file(MANIFEST, deflated)?;
    zip.write_all(&serde_json::to_vec(&manifest).map_err(WriteError::Json)?)?;
    zip.start_file(DOCUMENT, deflated)?;
    zip.write_all(&serde_json::to_vec(&dto).map_err(WriteError::Json)?)?;
    zip.start_file(STROKES, deflated)?;
    zip.write_all(&strokes_bytes(
        used.strokes
            .iter()
            .map(|id| (*id, &*document.store.strokes[id].points)),
    ))?;
    zip.start_file(MASKS, deflated)?;
    zip.write_all(&masks_bytes(
        used.masks.iter().map(|id| (*id, &document.store.masks[id])),
    ))?;
    for id in &used.assets {
        zip.start_file(image_entry(&hex(&id.0)), stored)?;
        zip.write_all(&document.store.assets[id].png)?;
    }
    Ok(zip.finish()?)
}

fn header(magic: [u8; 4], count: usize) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&magic);
    bytes.extend_from_slice(&BINARY_VERSION.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&(count as u32).to_le_bytes());
    bytes
}

/// `"UGS\0"`, version, flags, stroke count, then for each stroke in id
/// order its id, point count and x, y, pressure per point; little-endian.
pub fn strokes_bytes<'a>(
    strokes: impl ExactSizeIterator<Item = (StrokeId, &'a [Point])>,
) -> Vec<u8> {
    let mut bytes = header(STROKE_MAGIC, strokes.len());
    for (id, points) in strokes {
        bytes.extend_from_slice(&id.0.to_le_bytes());
        bytes.extend_from_slice(&(points.len() as u32).to_le_bytes());
        for point in points {
            bytes.extend_from_slice(&point.x.to_le_bytes());
            bytes.extend_from_slice(&point.y.to_le_bytes());
            bytes.extend_from_slice(&point.pressure.to_le_bytes());
        }
    }
    bytes
}

/// `"UGM\0"`, version, flags, mask count, then for each mask in id order
/// its id, left, top, width, height and rows of bits (the leftmost pixel in
/// the highest bit, each row padded to whole bytes); little-endian.
pub fn masks_bytes<'a>(masks: impl ExactSizeIterator<Item = (MaskId, &'a Mask)>) -> Vec<u8> {
    let mut bytes = header(MASK_MAGIC, masks.len());
    for (id, mask) in masks {
        bytes.extend_from_slice(&id.0.to_le_bytes());
        for value in mask.bounds {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.extend_from_slice(&mask.bits);
    }
    bytes
}

#[derive(Default)]
struct Used {
    strokes: BTreeSet<StrokeId>,
    masks: BTreeSet<MaskId>,
    assets: BTreeSet<AssetId>,
}

impl Used {
    fn layers(&mut self, layers: &[Layer]) {
        for layer in layers {
            match &layer.kind {
                LayerKind::Paint(paint) => self.ops(&paint.ops),
                LayerKind::Group(group) => self.layers(&group.children),
            }
        }
    }

    fn ops(&mut self, ops: &[Op]) {
        for op in ops {
            match op {
                Op::Paint { stroke, clip } | Op::Erase { stroke, clip } => {
                    self.strokes.insert(*stroke);
                    self.masks.extend(clip);
                }
                Op::Fill { coverage, clip, .. } => {
                    self.masks.insert(*coverage);
                    self.masks.extend(clip);
                }
                Op::PlaceImage { asset, .. } => {
                    self.assets.insert(*asset);
                }
                Op::TransformSelection { mask, .. } | Op::ClearSelection { mask } => {
                    self.masks.insert(*mask);
                }
                Op::Crop { .. } | Op::Resample { .. } => {}
                Op::Isolated(section) => self.ops(&section.ops),
            }
        }
    }
}

fn wobble(wobble: Wobble) -> WobbleDto {
    WobbleDto {
        amount: wobble.amount,
        style: match wobble.style {
            MotionStyle::Classic => MotionStyleDto::Classic,
        },
    }
}

fn blend(blend: Blend) -> BlendDto {
    match blend {
        Blend::Normal => BlendDto::Normal,
        Blend::Multiply => BlendDto::Multiply,
        Blend::Screen => BlendDto::Screen,
        Blend::Overlay => BlendDto::Overlay,
    }
}

fn sampling(sampling: Sampling) -> SamplingDto {
    match sampling {
        Sampling::Nearest => SamplingDto::Nearest,
        Sampling::Smooth => SamplingDto::Smooth,
    }
}

fn layer(layer: &Layer) -> LayerDto {
    let (paint, group) = match &layer.kind {
        LayerKind::Paint(paint) => (
            Some(PaintDto {
                opacity: paint.opacity,
                blend: blend(paint.blend),
                clip_to_below: paint.clip_to_below,
                wobble: paint.wobble.map(wobble),
                initial_size: paint.initial_size,
                ops: paint.ops.iter().map(op).collect(),
            }),
            None,
        ),
        LayerKind::Group(group) => (
            None,
            Some(GroupDto {
                opacity: group.opacity,
                blend: blend(group.blend),
                clip_to_below: group.clip_to_below,
                children: group.children.iter().map(self::layer).collect(),
            }),
        ),
    };
    LayerDto {
        id: layer.id.0,
        name: layer.name.clone(),
        visible: layer.visible,
        reference: layer.reference,
        paint,
        group,
    }
}

fn op(op: &Op) -> OpDto {
    match op {
        Op::Paint { stroke, clip } => OpDto::Paint {
            stroke: stroke.0,
            clip: clip.map(|mask| mask.0),
        },
        Op::Erase { stroke, clip } => OpDto::Erase {
            stroke: stroke.0,
            clip: clip.map(|mask| mask.0),
        },
        Op::Fill {
            coverage,
            color,
            antialias,
            clip,
        } => OpDto::Fill {
            coverage: coverage.0,
            color: color_hex(color.0),
            antialias: *antialias,
            clip: clip.map(|mask| mask.0),
        },
        Op::PlaceImage {
            asset,
            transform,
            sampling: how,
        } => OpDto::Image {
            image: hex(&asset.0),
            transform: transform.0,
            sampling: sampling(*how),
        },
        Op::TransformSelection {
            mask,
            transform,
            sampling: how,
            keep_source,
        } => OpDto::TransformSelection {
            mask: mask.0,
            transform: transform.0,
            sampling: sampling(*how),
            keep_source: *keep_source,
        },
        Op::ClearSelection { mask } => OpDto::ClearSelection { mask: mask.0 },
        Op::Crop { offset, size } => OpDto::Crop {
            offset: *offset,
            size: *size,
        },
        Op::Resample {
            size,
            sampling: how,
        } => OpDto::Resample {
            size: *size,
            sampling: sampling(*how),
        },
        Op::Isolated(section) => OpDto::Isolated {
            opacity: section.opacity,
            wobble: section.wobble.map(wobble),
            ops: section.ops.iter().map(self::op).collect(),
        },
    }
}

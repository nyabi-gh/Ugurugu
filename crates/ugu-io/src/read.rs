// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! Reads a `.ugu2` file without trusting it.
//!
//! The entry count is checked before any entry is opened, every entry is
//! read through a limit on what it actually inflates to, and so is the total.
//! Declared sizes are never used for allocation. The document is fully
//! validated before it is returned.

use std::collections::HashSet;
use std::io::{Read, Seek};
use std::sync::Arc;

use sha2::{Digest, Sha256};
use ugu_core::document::{Document, DocumentError, Group, Layer, LayerId, LayerKind};
use ugu_core::ops::{
    Affine, AssetId, Blend, MaskId, Op, PaintLayer, Rgba8, Sampling, Section, StrokeId, Wobble,
};
use ugu_core::store::{self, Asset, Brush, BrushEngine, Mask, Point, Store, Stroke};
use zip::ZipArchive;

use crate::format::*;

pub mod limits {
    /// Manifest, document, and the stored data a document can refer to.
    pub const ENTRIES: usize = 2 + 3 * ugu_core::document::limits::OPERATIONS;
    pub const MANIFEST_BYTES: u64 = 64 * 1024;
    pub const DOCUMENT_BYTES: u64 = 64 * 1024 * 1024;
    /// All entries together, inflated.
    pub const TOTAL_BYTES: u64 = DOCUMENT_BYTES + ugu_core::store::limits::BYTES + 1024 * 1024;
}

#[derive(Debug)]
pub enum ReadError {
    Io(std::io::Error),
    /// Not a ZIP file, or a broken one.
    NotAnArchive(String),
    /// A file from Ugurugu 2.x or earlier, which 3.0 does not open.
    Legacy(Legacy),
    /// Made by a newer version: an unknown schema, render revision or
    /// required feature.
    Newer(String),
    /// Damaged or not a `.ugu2` file.
    Corrupt(String),
    /// Over a size limit.
    TooLarge(String),
    Invalid(DocumentError),
}

impl std::fmt::Display for ReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(f, "cannot read the file: {error}"),
            Self::NotAnArchive(reason) => write!(f, "not a Ugurugu document: {reason}"),
            Self::Legacy(kind) => write!(
                f,
                "{kind:?} files from Ugurugu 2.x or earlier cannot be opened"
            ),
            Self::Newer(what) => write!(f, "made by a newer version of Ugurugu ({what})"),
            Self::Corrupt(reason) => write!(f, "the document is damaged: {reason}"),
            Self::TooLarge(what) => write!(f, "the document is too large: {what}"),
            Self::Invalid(error) => write!(f, "the document is invalid: {error}"),
        }
    }
}

impl std::error::Error for ReadError {}

fn corrupt(reason: impl Into<String>) -> ReadError {
    ReadError::Corrupt(reason.into())
}

/// Formats of earlier versions, recognised only to say so.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Legacy {
    /// JSON: `.ugu`, `.wagle`, `.wobble`, web `.wawa` and `.wwpreset`.
    Json,
    /// Native `.wawa`.
    Wawa,
}

/// Recognises an earlier format from the first bytes of a file.
pub fn legacy(start: &[u8]) -> Option<Legacy> {
    let text = start.strip_prefix(b"\xef\xbb\xbf").unwrap_or(start);
    if text.iter().find(|byte| !byte.is_ascii_whitespace()) == Some(&b'{') {
        return Some(Legacy::Json);
    }
    // A .NET length-prefixed string: length 4, then the magic.
    start.starts_with(b"\x04WAWA").then_some(Legacy::Wawa)
}

/// Reads a document and its id.
pub fn read<R: Read + Seek>(mut input: R) -> Result<(Document, [u8; 16]), ReadError> {
    let mut start = [0u8; 64];
    let length = read_up_to(&mut input, &mut start).map_err(ReadError::Io)?;
    input.rewind().map_err(ReadError::Io)?;
    if !start.starts_with(b"PK")
        && let Some(kind) = legacy(&start[..length])
    {
        return Err(ReadError::Legacy(kind));
    }
    let archive =
        ZipArchive::new(input).map_err(|error| ReadError::NotAnArchive(error.to_string()))?;
    if archive.len() > limits::ENTRIES {
        return Err(ReadError::TooLarge(format!("{} entries", archive.len())));
    }
    let mut entries = Entries {
        archive,
        names: HashSet::new(),
        total: 0,
    };
    entries.check_names()?;

    let manifest: Manifest =
        serde_json::from_slice(&entries.read(MANIFEST, limits::MANIFEST_BYTES)?)
            .map_err(|error| corrupt(format!("manifest: {error}")))?;
    if manifest.format != FORMAT {
        return Err(corrupt("not a Ugurugu document"));
    }
    if manifest.schema != SCHEMA {
        return Err(ReadError::Newer(format!("schema {}", manifest.schema)));
    }
    if manifest.render_revision != RENDER_REVISION {
        return Err(ReadError::Newer(format!(
            "render revision {}",
            manifest.render_revision
        )));
    }
    if let Some(feature) = manifest.required.first() {
        return Err(ReadError::Newer(format!("feature {feature}")));
    }
    let id = parse_hex::<16>(&manifest.document_id).ok_or_else(|| corrupt("document id"))?;

    let dto: DocumentDto = serde_json::from_slice(&entries.read(DOCUMENT, limits::DOCUMENT_BYTES)?)
        .map_err(|error| corrupt(format!("document: {error}")))?;
    let store = entries.store(&dto)?;
    if let Some(stray) = entries.unread().next() {
        return Err(corrupt(format!("unexpected entry {stray}")));
    }
    let document = Document {
        canvas: dto.canvas,
        background: Rgba8(
            parse_color(&dto.background).ok_or_else(|| corrupt("background colour"))?,
        ),
        frames: dto.frames,
        frames_per_second: dto.frames_per_second,
        wobble: wobble(&dto.wobble),
        layers: dto.layers.iter().map(layer).collect::<Result<_, _>>()?,
        store,
    };
    document.validate().map_err(ReadError::Invalid)?;
    Ok((document, id))
}

fn read_up_to(input: &mut impl Read, buffer: &mut [u8]) -> std::io::Result<usize> {
    let mut filled = 0;
    while filled < buffer.len() {
        match input.read(&mut buffer[filled..])? {
            0 => break,
            count => filled += count,
        }
    }
    Ok(filled)
}

struct Entries<R> {
    archive: ZipArchive<R>,
    /// Names read so far.
    names: HashSet<String>,
    /// Bytes inflated so far.
    total: u64,
}

impl<R: Read + Seek> Entries<R> {
    fn check_names(&mut self) -> Result<(), ReadError> {
        let mut seen = HashSet::new();
        for index in 0..self.archive.len() {
            let entry = self
                .archive
                .by_index_raw(index)
                .map_err(|error| corrupt(error.to_string()))?;
            let name = entry.name().to_owned();
            if !seen.insert(name.clone()) {
                return Err(corrupt(format!("{name} appears twice")));
            }
            if entry.is_dir() || entry.encrypted() || !known_name(&name) {
                return Err(corrupt(format!("unexpected entry {name}")));
            }
            if !matches!(
                entry.compression(),
                zip::CompressionMethod::Stored | zip::CompressionMethod::Deflated
            ) {
                return Err(corrupt(format!("{name} uses an unsupported compression")));
            }
        }
        Ok(())
    }

    /// Reads an entry, stopping at `limit` bytes of actual output.
    fn read(&mut self, name: &str, limit: u64) -> Result<Vec<u8>, ReadError> {
        let entry = self
            .archive
            .by_name(name)
            .map_err(|_| corrupt(format!("{name} is missing")))?;
        let remaining = limits::TOTAL_BYTES - self.total;
        let cap = limit.min(remaining);
        let mut bytes = Vec::new();
        entry
            .take(cap + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| corrupt(format!("{name}: {error}")))?;
        if bytes.len() as u64 > cap {
            return Err(ReadError::TooLarge(name.to_owned()));
        }
        self.total += bytes.len() as u64;
        self.names.insert(name.to_owned());
        Ok(bytes)
    }

    fn unread(&self) -> impl Iterator<Item = &str> {
        self.archive
            .file_names()
            .filter(|name| !self.names.contains(*name))
    }

    fn store(&mut self, dto: &DocumentDto) -> Result<Store, ReadError> {
        let mut store = Store::default();
        let point_bytes = 12 + 12 * store::limits::POINTS_PER_STROKE as u64;
        for stroke in &dto.strokes {
            let id = StrokeId(stroke.id);
            let points = parse_points(&self.read(&stroke_entry(stroke.id), point_bytes)?)
                .ok_or_else(|| corrupt(format!("stroke {}", stroke.id)))?;
            let seed = parse_hex::<8>(&stroke.seed)
                .ok_or_else(|| corrupt(format!("stroke {} seed", stroke.id)))?;
            let value = Stroke {
                points: points.into(),
                color: Rgba8(parse_color(&stroke.color).ok_or_else(|| corrupt("stroke colour"))?),
                width: stroke.width,
                brush: Brush {
                    engine: match stroke.brush.engine {
                        EngineDto::Line => BrushEngine::Line,
                        EngineDto::Airbrush => BrushEngine::Airbrush,
                        EngineDto::Spray => BrushEngine::Spray,
                    },
                    opacity: stroke.brush.opacity,
                    hardness: stroke.brush.hardness,
                    antialias: stroke.brush.antialias,
                },
                seed: u64::from_be_bytes(seed),
            };
            if store.strokes.insert(id, value).is_some() {
                return Err(corrupt(format!("stroke {} appears twice", stroke.id)));
            }
        }
        for mask in &dto.masks {
            let bytes = self.read(&mask_entry(mask.id), store::limits::BYTES)?;
            let value = parse_mask(&bytes).ok_or_else(|| corrupt(format!("mask {}", mask.id)))?;
            if store.masks.insert(MaskId(mask.id), value).is_some() {
                return Err(corrupt(format!("mask {} appears twice", mask.id)));
            }
        }
        for image in &dto.images {
            let id = parse_hex::<32>(&image.id).ok_or_else(|| corrupt("image id"))?;
            let png = self.read(&image_entry(&image.id), store::limits::BYTES)?;
            if <[u8; 32]>::from(Sha256::digest(&png)) != id {
                return Err(corrupt(format!(
                    "image {} does not match its name",
                    image.id
                )));
            }
            let value = Asset {
                size: image.size,
                png: png.into(),
            };
            if store.assets.insert(AssetId(id), value).is_some() {
                return Err(corrupt(format!("image {} appears twice", image.id)));
            }
        }
        Ok(store)
    }
}

fn known_name(name: &str) -> bool {
    let number = |text: &str| {
        !text.is_empty() && text.len() <= 10 && text.bytes().all(|byte| byte.is_ascii_digit())
    };
    name == MANIFEST
        || name == DOCUMENT
        || name
            .strip_prefix("strokes/")
            .and_then(|rest| rest.strip_suffix(".bin"))
            .is_some_and(number)
        || name
            .strip_prefix("masks/")
            .and_then(|rest| rest.strip_suffix(".bin"))
            .is_some_and(number)
        || name
            .strip_prefix("images/")
            .and_then(|rest| rest.strip_suffix(".png"))
            .is_some_and(|hex| parse_hex::<32>(hex).is_some())
}

fn header(bytes: &[u8], magic: [u8; 4]) -> Option<&[u8]> {
    let rest = bytes.strip_prefix(&magic)?;
    let version = u16::from_le_bytes(rest.get(0..2)?.try_into().ok()?);
    let flags = u16::from_le_bytes(rest.get(2..4)?.try_into().ok()?);
    (version == BINARY_VERSION && flags == 0).then(|| &rest[4..])
}

fn parse_points(bytes: &[u8]) -> Option<Vec<Point>> {
    let rest = header(bytes, STROKE_MAGIC)?;
    let count = u32::from_le_bytes(rest.get(0..4)?.try_into().ok()?) as usize;
    let data = &rest[4..];
    if data.len() != count.checked_mul(12)? {
        return None;
    }
    let (points, _) = data.as_chunks::<12>();
    Some(
        points
            .iter()
            .map(|point| {
                let [x, y, pressure] = [0, 4, 8].map(|at| {
                    f32::from_le_bytes([point[at], point[at + 1], point[at + 2], point[at + 3]])
                });
                Point { x, y, pressure }
            })
            .collect(),
    )
}

fn parse_mask(bytes: &[u8]) -> Option<Mask> {
    let rest = header(bytes, MASK_MAGIC)?;
    let mut bounds = [0i32; 4];
    for (index, value) in bounds.iter_mut().enumerate() {
        *value = i32::from_le_bytes(rest.get(index * 4..index * 4 + 4)?.try_into().ok()?);
    }
    let bits: Arc<[u8]> = rest.get(16..)?.into();
    let mask = Mask { bounds, bits };
    // The bounds must account for every byte that follows.
    store::check_mask(MaskId(0), &mask).ok()?;
    Some(mask)
}

fn wobble(dto: &WobbleDto) -> Wobble {
    Wobble { amount: dto.amount }
}

fn blend(dto: BlendDto) -> Blend {
    match dto {
        BlendDto::Normal => Blend::Normal,
        BlendDto::Multiply => Blend::Multiply,
        BlendDto::Screen => Blend::Screen,
        BlendDto::Overlay => Blend::Overlay,
    }
}

fn sampling(dto: SamplingDto) -> Sampling {
    match dto {
        SamplingDto::Nearest => Sampling::Nearest,
        SamplingDto::Smooth => Sampling::Smooth,
    }
}

fn layer(dto: &LayerDto) -> Result<Layer, ReadError> {
    let kind = match (&dto.paint, &dto.group) {
        (Some(paint), None) => LayerKind::Paint(PaintLayer {
            ops: paint.ops.iter().map(op).collect::<Result<_, _>>()?,
            opacity: paint.opacity,
            blend: blend(paint.blend),
            clip_to_below: paint.clip_to_below,
            wobble: paint.wobble.as_ref().map(wobble),
            initial_size: paint.initial_size,
        }),
        (None, Some(group)) => LayerKind::Group(Group {
            opacity: group.opacity,
            blend: blend(group.blend),
            clip_to_below: group.clip_to_below,
            children: group.children.iter().map(layer).collect::<Result<_, _>>()?,
        }),
        _ => {
            return Err(corrupt(format!(
                "layer {} must be either paint or group",
                dto.id
            )));
        }
    };
    Ok(Layer {
        id: LayerId(dto.id),
        name: dto.name.clone(),
        visible: dto.visible,
        reference: dto.reference,
        kind,
    })
}

fn op(dto: &OpDto) -> Result<Op, ReadError> {
    let clip = |mask: &Option<u32>| mask.map(MaskId);
    Ok(match dto {
        OpDto::Paint { stroke, clip: mask } => Op::Paint {
            stroke: StrokeId(*stroke),
            clip: clip(mask),
        },
        OpDto::Erase { stroke, clip: mask } => Op::Erase {
            stroke: StrokeId(*stroke),
            clip: clip(mask),
        },
        OpDto::Fill {
            coverage,
            color,
            antialias,
            clip: mask,
        } => Op::Fill {
            coverage: MaskId(*coverage),
            color: Rgba8(parse_color(color).ok_or_else(|| corrupt("fill colour"))?),
            antialias: *antialias,
            clip: clip(mask),
        },
        OpDto::Image {
            image,
            transform,
            sampling: how,
        } => Op::PlaceImage {
            asset: AssetId(parse_hex::<32>(image).ok_or_else(|| corrupt("image id"))?),
            transform: finite(transform)?,
            sampling: sampling(*how),
        },
        OpDto::TransformSelection {
            mask,
            transform,
            sampling: how,
            keep_source,
        } => Op::TransformSelection {
            mask: MaskId(*mask),
            transform: finite(transform)?,
            sampling: sampling(*how),
            keep_source: *keep_source,
        },
        OpDto::ClearSelection { mask } => Op::ClearSelection {
            mask: MaskId(*mask),
        },
        OpDto::Crop { offset, size } => Op::Crop {
            offset: *offset,
            size: *size,
        },
        OpDto::Resample {
            size,
            sampling: how,
        } => Op::Resample {
            size: *size,
            sampling: sampling(*how),
        },
        OpDto::Isolated {
            opacity,
            wobble: w,
            ops,
        } => Op::Isolated(Box::new(Section {
            ops: ops.iter().map(op).collect::<Result<_, _>>()?,
            opacity: *opacity,
            wobble: w.as_ref().map(wobble),
        })),
    })
}

fn finite(transform: &[f64; 6]) -> Result<Affine, ReadError> {
    if transform.iter().all(|value| value.is_finite()) {
        Ok(Affine(*transform))
    } else {
        Err(corrupt("transform is not finite"))
    }
}

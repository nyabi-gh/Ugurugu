// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! Imports a Ugurugu 2.2.13 `.ugu` project (JSON, schema 13) as a new
//! document, following docs/rust/ugu-import.md.
//!
//! Every value is checked as 2.2.13 checks it, and the result is validated
//! as a `.ugurugu` file is. What 3.0 cannot show exactly as 2.2.13 does is
//! refused with the reason rather than changed.

use std::collections::HashMap;
use std::io::Read;
use std::sync::Arc;

use serde::Deserialize;
use serde::de::{self, IgnoredAny, SeqAccess, Visitor};
use ugu_core::document::{Document, DocumentError, Group, Layer, LayerId, LayerKind};
use ugu_core::ops::{
    Affine, AssetId, Blend, MaskId, Motion, MotionStyle, Op, PaintLayer, Rgba8, Sampling, Section,
    StrokeId, Wobble,
};
use ugu_core::store::{Brush, BrushEngine, Mask, Point, Store, StoreError, Stroke, TipShape};

use crate::import::{self, ImportError};

/// The only schema read; earlier ones are re-saved by 2.2.13 first.
pub const SCHEMA: i64 = 13;
const ALGORITHM: i64 = 3;
/// 2.2.13's project size limit.
pub const FILE_BYTES: u64 = 128 * 1024 * 1024;
/// 2.2.13's limit on decoded mask data.
const MASK_BYTES: u64 = 256 * 1024 * 1024;

#[derive(Debug)]
pub enum LegacyError {
    Io(std::io::Error),
    /// Not a 2.x project, or a damaged one.
    Damaged(String),
    /// Written by 2.2.12 or earlier.
    Older(i64),
    /// Written by a version after 2.2.13.
    Newer(i64),
    TooLarge(String),
    Refused(Refusal),
    Invalid(DocumentError),
}

/// What 3.0 does not show as 2.2.13 did (docs/rust/ugu-import.md section 2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// A fill flooded again on every frame, from a file first saved before
    /// schema 4.
    ProceduralFill,
    /// A crop or resize between merged layers' boundaries.
    CanvasChangeInMerge,
    /// An image over `import::EDGE` on a side.
    LargeImage,
    /// More data than a 3.0 document holds.
    TooMuchData,
    /// A colour not written as `#AARRGGBB` or `#RRGGBB`.
    Color(String),
}

impl std::fmt::Display for LegacyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(f, "cannot read the file: {error}"),
            Self::Damaged(reason) => write!(f, "not a readable Ugurugu 2.x project: {reason}"),
            Self::Older(schema) => write!(
                f,
                "schema {schema} is from before 2.2.13; save it again in 2.2.13 first"
            ),
            Self::Newer(schema) => write!(f, "schema {schema} is newer than 2.2.13"),
            Self::TooLarge(what) => write!(f, "the project is too large: {what}"),
            Self::Refused(refusal) => match refusal {
                Refusal::ProceduralFill => write!(f, "it has a fill that floods on every frame"),
                Refusal::CanvasChangeInMerge => {
                    write!(f, "it crops or resizes between merged layers")
                }
                Refusal::LargeImage => write!(f, "it has an image over {} pixels", import::EDGE),
                Refusal::TooMuchData => write!(f, "it holds more data than 3.0 allows"),
                Refusal::Color(text) => write!(f, "it has the colour {text}"),
            },
            Self::Invalid(error) => write!(f, "the project is invalid: {error}"),
        }
    }
}

impl std::error::Error for LegacyError {}

fn damaged(reason: impl Into<String>) -> LegacyError {
    LegacyError::Damaged(reason.into())
}

/// A 2.2.13 project as a 3.0 document, with the layer that was active.
#[derive(Debug)]
pub struct Imported {
    pub document: Document,
    pub active: Option<LayerId>,
}

/// Reads at most `FILE_BYTES` and imports them.
pub fn read(input: impl Read) -> Result<Imported, LegacyError> {
    let mut bytes = Vec::new();
    input
        .take(FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(LegacyError::Io)?;
    if bytes.len() as u64 > FILE_BYTES {
        return Err(LegacyError::TooLarge(format!(
            "over {} MiB",
            FILE_BYTES >> 20
        )));
    }
    import(&bytes)
}

pub fn import(bytes: &[u8]) -> Result<Imported, LegacyError> {
    let version: Version =
        serde_json::from_slice(bytes).map_err(|error| damaged(error.to_string()))?;
    let schema = version
        .schema_version
        .ok_or_else(|| damaged("no schema version"))?;
    if schema < SCHEMA {
        return Err(LegacyError::Older(schema));
    }
    if schema > SCHEMA || version.algorithm_version != Some(ALGORITHM) {
        return Err(LegacyError::Newer(schema));
    }
    let root: Root = serde_json::from_slice(bytes).map_err(|error| damaged(error.to_string()))?;
    Importer::new(&root)?.document(&root)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Version {
    schema_version: Option<i64>,
    algorithm_version: Option<i64>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Root {
    canvas: CanvasDto,
    animation: AnimationDto,
    layers: Vec<LayerDto>,
    clip_masks: Vec<ClipMaskDto>,
    binary_masks: Vec<BinaryMaskDto>,
    raster_assets: Vec<RasterAssetDto>,
    active_layer_id: Option<String>,
}

#[derive(Deserialize)]
struct CanvasDto {
    width: u32,
    height: u32,
    background: String,
}

#[derive(Deserialize)]
struct AnimationDto {
    frames: u32,
    fps: f64,
    wobble: f64,
    motion: MotionDto,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct MotionDto {
    style: String,
    pose_count: u32,
    detail: u32,
    linked: f64,
    randomness: f64,
    broken_line: bool,
    break_amount: f64,
    break_range: f64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LayerDto {
    id: String,
    name: String,
    kind: String,
    parent_group_id: Option<String>,
    clip_to_layer_below: bool,
    visible: bool,
    reference: bool,
    opacity: f64,
    blend_mode: String,
    initial_canvas_size: [u32; 2],
    wobble: Option<f64>,
    motion: Option<MotionDto>,
    strokes: Vec<StrokeDto>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct StrokeDto {
    seed: String,
    mode: String,
    color: String,
    width: f64,
    brush: BrushDto,
    points: Vec<PointDto>,
    clip_mask_id: Option<String>,
    visibility_clip: Option<[i32; 4]>,
    fill_mask_id: Option<String>,
    fill_coverage_id: Option<String>,
    pixel_selection: Option<PixelSelectionDto>,
    reframe: Option<ReframeDto>,
    image: Option<ImageDto>,
    clip_mask: Option<IgnoredAny>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct BrushDto {
    engine: String,
    tip: String,
    opacity: f64,
    flow: f64,
    hardness: f64,
    spacing: f64,
    scatter: f64,
    particle_size: f64,
    density: f64,
    size_dynamics: f64,
    opacity_dynamics: f64,
    size_jitter: f64,
    animated_jitter: bool,
    wobble_scale: Option<f64>,
    antialiasing: Option<bool>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PixelSelectionDto {
    mask_id: String,
    transform: [f64; 9],
    sampling: String,
    clear_source: bool,
    draw_destination: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReframeDto {
    mode: String,
    sampling: String,
    source_size: [u32; 2],
    target_size: [u32; 2],
    content_offset: [i32; 2],
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ImageDto {
    asset_id: String,
    transform: [f64; 9],
    sampling: String,
}

#[derive(Deserialize)]
struct ClipMaskDto {
    id: String,
    width: u32,
    height: u32,
    data: String,
}

#[derive(Deserialize)]
struct BinaryMaskDto {
    id: String,
    canvas: [u32; 2],
    bounds: [i32; 4],
    data: String,
}

#[derive(Deserialize)]
struct RasterAssetDto {
    id: String,
    size: [u32; 2],
    data: String,
}

/// `[x, y]` or `[x, y, pressure]`, without a list per point.
struct PointDto([f64; 3]);

impl<'de> Deserialize<'de> for PointDto {
    fn deserialize<D: de::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct PointVisitor;
        impl<'de> Visitor<'de> for PointVisitor {
            type Value = PointDto;

            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("[x, y] or [x, y, pressure]")
            }

            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<PointDto, A::Error> {
                let x = seq
                    .next_element()?
                    .ok_or_else(|| de::Error::invalid_length(0, &self))?;
                let y = seq
                    .next_element()?
                    .ok_or_else(|| de::Error::invalid_length(1, &self))?;
                let pressure = seq.next_element()?.unwrap_or(1.0);
                if seq.next_element::<IgnoredAny>()?.is_some() {
                    return Err(de::Error::invalid_length(4, &self));
                }
                Ok(PointDto([x, y, pressure]))
            }
        }
        deserializer.deserialize_seq(PointVisitor)
    }
}

/// An 8-bit selection mask, one byte per pixel, kept until a stroke turns it
/// into bits with its rectangle.
struct GrayMask {
    size: [u32; 2],
    pixels: Arc<[u8]>,
}

/// What a stroke's selection leaves of the canvas.
#[derive(Clone, Copy)]
enum Clip {
    Empty,
    Whole,
    Mask(MaskId),
}

/// An 8-bit mask's id ("" for none), the rectangle it is cut to and the
/// canvas it is on.
type ClipKey<'a> = (&'a str, Option<[i32; 4]>, [u32; 2]);

/// A mask's bounds and bits.
type Bits = ([i32; 4], Vec<u8>);

struct Importer<'a> {
    gray: HashMap<&'a str, GrayMask>,
    binary: HashMap<&'a str, ([u32; 2], Mask)>,
    assets: HashMap<&'a str, AssetId>,
    store: Store,
    clips: HashMap<ClipKey<'a>, Clip>,
    packed: HashMap<&'a str, MaskId>,
    next_stroke: u32,
    next_mask: u32,
}

impl<'a> Importer<'a> {
    fn new(root: &'a Root) -> Result<Self, LegacyError> {
        let mut decoded = 0u64;
        let mut gray = HashMap::new();
        for entry in &root.clip_masks {
            if !edge_ok(entry.width) || !edge_ok(entry.height) {
                return Err(damaged("a selection mask has an invalid size"));
            }
            let length = u64::from(entry.width) * u64::from(entry.height);
            decoded += length;
            if decoded > MASK_BYTES {
                return Err(LegacyError::TooLarge("selection masks".to_owned()));
            }
            let pixels = uncompress(&entry.data, length)?;
            if gray
                .insert(
                    entry.id.as_str(),
                    GrayMask {
                        size: [entry.width, entry.height],
                        pixels: pixels.into(),
                    },
                )
                .is_some()
            {
                return Err(damaged("a selection mask is listed twice"));
            }
        }
        let mut binary = HashMap::new();
        for entry in &root.binary_masks {
            let [canvas_width, canvas_height] = entry.canvas;
            let [x, y, width, height] = entry.bounds;
            if !edge_ok(canvas_width)
                || !edge_ok(canvas_height)
                || x < 0
                || y < 0
                || width <= 0
                || height <= 0
                || x as u64 + width as u64 > u64::from(canvas_width)
                || y as u64 + height as u64 > u64::from(canvas_height)
            {
                return Err(damaged("a binary mask has invalid bounds"));
            }
            let length = (Mask::row_bytes(width) * height as usize) as u64;
            decoded += length;
            if decoded > MASK_BYTES {
                return Err(LegacyError::TooLarge("binary masks".to_owned()));
            }
            let bits = uncompress(&entry.data, length)?;
            let mask = Mask {
                bounds: entry.bounds,
                bits: bits.into(),
            };
            if binary
                .insert(entry.id.as_str(), (entry.canvas, mask))
                .is_some()
            {
                return Err(damaged("a binary mask is listed twice"));
            }
        }
        let mut store = Store::default();
        let mut assets = HashMap::new();
        for entry in &root.raster_assets {
            let [width, height] = entry.size;
            let pixels = u64::from(width) * u64::from(height);
            if pixels == 0 || pixels > ugu_core::store::limits::ASSET_PIXELS {
                return Err(damaged("an image has an invalid size"));
            }
            let straight = uncompress(&entry.data, pixels * 4)?;
            let (id, asset) = import::from_rgba(entry.size, straight, [import::EDGE, import::EDGE])
                .map_err(|error| match error {
                    ImportError::TooLarge => LegacyError::Refused(Refusal::LargeImage),
                    other => damaged(other.to_string()),
                })?;
            // A shrunk image would no longer match its placement.
            if asset.size != entry.size {
                return Err(LegacyError::Refused(Refusal::LargeImage));
            }
            store.assets.insert(id, asset);
            if assets.insert(entry.id.as_str(), id).is_some() {
                return Err(damaged("an image is listed twice"));
            }
        }
        Ok(Self {
            gray,
            binary,
            assets,
            store,
            clips: HashMap::new(),
            packed: HashMap::new(),
            next_stroke: 1,
            next_mask: 1,
        })
    }

    fn document(mut self, root: &'a Root) -> Result<Imported, LegacyError> {
        let canvas = [root.canvas.width, root.canvas.height];
        if !canvas.iter().all(|&edge| edge_ok(edge)) {
            return Err(damaged("the canvas size is invalid"));
        }
        let background = Rgba8(color(&root.canvas.background)?);
        let animation = &root.animation;
        let wobble = Wobble {
            amount: animation.wobble as f32,
            motion: motion(&animation.motion)?,
        };
        if root.layers.len() > ugu_core::document::limits::LAYERS {
            return Err(LegacyError::TooLarge(format!(
                "{} layers",
                root.layers.len()
            )));
        }
        let mut index = HashMap::new();
        for (position, layer) in root.layers.iter().enumerate() {
            if index.insert(layer.id.as_str(), position).is_some() {
                return Err(damaged("two layers have the same id"));
            }
        }
        // Children come before their group; siblings keep their order.
        let mut children: Vec<Vec<usize>> = vec![Vec::new(); root.layers.len()];
        let mut top = Vec::new();
        for (position, layer) in root.layers.iter().enumerate() {
            match &layer.parent_group_id {
                None => top.push(position),
                Some(parent) => {
                    let &parent = index
                        .get(parent.as_str())
                        .ok_or_else(|| damaged("a layer's group is missing"))?;
                    if root.layers[parent].kind != "group" {
                        return Err(damaged("a layer's group is not a group"));
                    }
                    children[parent].push(position);
                }
            }
        }
        let mut ids = vec![None; root.layers.len()];
        let mut placed = 0;
        let layers = self.tree(root, &top, &children, &mut ids, &mut placed, 0)?;
        if placed != root.layers.len() {
            return Err(damaged("the layer groups form a loop"));
        }
        let active = root
            .active_layer_id
            .as_deref()
            .and_then(|id| index.get(id))
            .and_then(|&position| ids[position]);
        let document = Document {
            canvas,
            background,
            frames: animation.frames,
            frames_per_second: animation.fps as f32,
            wobble,
            // 2.2.13 has no pixel mode.
            pixel: false,
            layers,
            store: self.store,
        };
        match document.validate() {
            Ok(()) => Ok(Imported { document, active }),
            Err(DocumentError::Store(StoreError::TooManyBytes(_))) => {
                Err(LegacyError::Refused(Refusal::TooMuchData))
            }
            Err(error) => Err(LegacyError::Invalid(error)),
        }
    }

    fn tree(
        &mut self,
        root: &'a Root,
        positions: &[usize],
        children: &[Vec<usize>],
        ids: &mut [Option<LayerId>],
        placed: &mut usize,
        depth: usize,
    ) -> Result<Vec<Layer>, LegacyError> {
        // Deeper than the layer limit can only be a loop.
        if depth > ugu_core::document::limits::LAYERS {
            return Err(damaged("the layer groups form a loop"));
        }
        let mut layers = Vec::with_capacity(positions.len());
        for &position in positions {
            *placed += 1;
            let id = LayerId(*placed as u32);
            ids[position] = Some(id);
            let dto = &root.layers[position];
            if !(0.0..=1.0).contains(&dto.opacity) {
                return Err(damaged("a layer has an invalid opacity"));
            }
            let blend = blend(&dto.blend_mode)?;
            let kind = match dto.kind.as_str() {
                "group" => {
                    if !dto.strokes.is_empty() {
                        return Err(damaged("a group has strokes"));
                    }
                    LayerKind::Group(Group {
                        opacity: dto.opacity as f32,
                        blend,
                        clip_to_below: dto.clip_to_layer_below,
                        children: self.tree(
                            root,
                            &children[position],
                            children,
                            ids,
                            placed,
                            depth + 1,
                        )?,
                    })
                }
                "paint" => LayerKind::Paint(self.paint(dto, blend)?),
                _ => return Err(damaged("a layer has an invalid kind")),
            };
            if dto.name.trim().is_empty() {
                return Err(damaged("a layer has no name"));
            }
            layers.push(Layer {
                id,
                name: dto.name.clone(),
                visible: dto.visible,
                reference: dto.reference,
                kind,
            });
        }
        Ok(layers)
    }

    fn paint(&mut self, dto: &'a LayerDto, blend: Blend) -> Result<PaintLayer, LegacyError> {
        let wobble = match (dto.wobble, &dto.motion) {
            (None, None) => None,
            (Some(amount), Some(settings)) => Some(Wobble {
                amount: amount as f32,
                motion: motion(settings)?,
            }),
            _ => return Err(damaged("a layer has half a wobble override")),
        };
        let initial_size = dto.initial_canvas_size;
        let mut size = initial_size;
        // Split by merge boundaries.
        let mut parts: Vec<Vec<Op>> = vec![Vec::new()];
        for stroke in &dto.strokes {
            if stroke.mode == "compositeBoundary" {
                if !stroke.points.is_empty() {
                    return Err(damaged("a merge boundary has points"));
                }
                parts.push(Vec::new());
                continue;
            }
            if let Some(op) = self.op(stroke, &mut size)? {
                parts.last_mut().expect("one part").push(op);
            }
        }
        let last = parts.len() - 1;
        let mut ops = Vec::new();
        for (index, part) in parts.into_iter().enumerate() {
            if index == 0 || index == last {
                ops.extend(part);
                continue;
            }
            if part
                .iter()
                .any(|op| matches!(op, Op::Crop { .. } | Op::Resample { .. }))
            {
                return Err(LegacyError::Refused(Refusal::CanvasChangeInMerge));
            }
            if !part.is_empty() {
                // The layer's own wobble, so a section moves as the layer did.
                ops.push(Op::Isolated(Box::new(Section {
                    ops: part,
                    opacity: 1.0,
                    wobble,
                })));
            }
        }
        Ok(PaintLayer {
            ops,
            opacity: dto.opacity as f32,
            blend,
            clip_to_below: dto.clip_to_layer_below,
            wobble,
            initial_size,
        })
    }

    /// The operation for one 2.2.13 stroke on a canvas of `size`, or `None`
    /// when it can draw nothing.
    fn op(&mut self, dto: &'a StrokeDto, size: &mut [u32; 2]) -> Result<Option<Op>, LegacyError> {
        let without_points = matches!(dto.mode.as_str(), "pixelSelection" | "reframe" | "image");
        if without_points != dto.points.is_empty() {
            return Err(damaged("a stroke has an invalid point count"));
        }
        if dto.clip_mask.is_some() {
            return Err(damaged(
                "a stroke has an old clip mask in a current project",
            ));
        }
        if (dto.mode != "pixelSelection" && dto.pixel_selection.is_some())
            || (dto.mode != "reframe" && dto.reframe.is_some())
            || (dto.mode != "image" && dto.image.is_some())
            || (dto.mode != "fill"
                && (dto.fill_mask_id.is_some() || dto.fill_coverage_id.is_some()))
        {
            return Err(damaged("a stroke has data for another mode"));
        }
        let rgba = color(&dto.color)?;
        let op = match dto.mode.as_str() {
            "paint" | "erase" => {
                let Some(clip) = self.clip(dto, *size)? else {
                    return Ok(None);
                };
                let stroke = self.stroke(dto, rgba)?;
                if dto.mode == "paint" {
                    Op::Paint { stroke, clip }
                } else {
                    Op::Erase { stroke, clip }
                }
            }
            "fill" => {
                let (coverage, antialias) = match (&dto.fill_coverage_id, &dto.fill_mask_id) {
                    (Some(id), None) => (self.packed(id, *size)?, brush(&dto.brush)?.antialias),
                    // 2.2.13 always spreads a schema-5 fill mask by a pixel.
                    (None, Some(id)) => match self.fill_mask(id, *size)? {
                        Some(mask) => (mask, true),
                        None => return Ok(None),
                    },
                    (None, None) => return Err(LegacyError::Refused(Refusal::ProceduralFill)),
                    (Some(_), Some(_)) => return Err(damaged("a fill has two masks")),
                };
                let Some(clip) = self.clip(dto, *size)? else {
                    return Ok(None);
                };
                Op::Fill {
                    coverage,
                    color: Rgba8(rgba),
                    antialias,
                    clip,
                }
            }
            "image" => {
                let image = dto
                    .image
                    .as_ref()
                    .ok_or_else(|| damaged("an image has no data"))?;
                let asset = *self
                    .assets
                    .get(image.asset_id.as_str())
                    .ok_or_else(|| damaged("an image is missing"))?;
                Op::PlaceImage {
                    asset,
                    transform: affine(&image.transform)?,
                    sampling: sampling(&image.sampling)?,
                }
            }
            "pixelSelection" => {
                let selection = dto
                    .pixel_selection
                    .as_ref()
                    .ok_or_else(|| damaged("a selection operation has no data"))?;
                let mask = self.packed(&selection.mask_id, *size)?;
                let transform = affine(&selection.transform)?;
                let sampling = sampling(&selection.sampling)?;
                match (selection.clear_source, selection.draw_destination) {
                    (true, true) | (false, true) => Op::TransformSelection {
                        mask,
                        transform,
                        sampling,
                        keep_source: !selection.clear_source,
                    },
                    (true, false) => Op::ClearSelection { mask },
                    (false, false) => return Err(damaged("a selection operation does nothing")),
                }
            }
            "reframe" => {
                let reframe = dto
                    .reframe
                    .as_ref()
                    .ok_or_else(|| damaged("a reframe has no data"))?;
                if reframe.source_size != *size || !reframe.target_size.iter().all(|&e| edge_ok(e))
                {
                    return Err(damaged("a reframe does not follow the canvas"));
                }
                *size = reframe.target_size;
                match reframe.mode.as_str() {
                    "canvas" => Op::Crop {
                        offset: reframe.content_offset,
                        size: reframe.target_size,
                    },
                    "image" => Op::Resample {
                        size: reframe.target_size,
                        sampling: sampling(&reframe.sampling)?,
                    },
                    _ => return Err(damaged("a reframe has an invalid mode")),
                }
            }
            _ => return Err(damaged("a stroke has an invalid mode")),
        };
        Ok(Some(op))
    }

    fn stroke(&mut self, dto: &StrokeDto, rgba: [u8; 4]) -> Result<StrokeId, LegacyError> {
        if dto.seed.is_empty() || !dto.seed.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(damaged("a stroke has an invalid seed"));
        }
        let seed = dto
            .seed
            .parse()
            .map_err(|_| damaged("a stroke has an invalid seed"))?;
        let points: Vec<Point> = dto
            .points
            .iter()
            .map(|PointDto([x, y, pressure])| Point {
                x: *x as f32,
                y: *y as f32,
                pressure: *pressure as f32,
            })
            .collect();
        let id = StrokeId(self.next_stroke);
        self.next_stroke += 1;
        self.store.strokes.insert(
            id,
            Stroke {
                points: points.into(),
                color: Rgba8(rgba),
                width: dto.width as f32,
                brush: brush(&dto.brush)?,
                seed,
            },
        );
        Ok(id)
    }

    /// The stroke's selection mask and rectangle as one mask, `Some(None)`
    /// when they leave the whole canvas, `None` when they leave nothing.
    fn clip(
        &mut self,
        dto: &'a StrokeDto,
        size: [u32; 2],
    ) -> Result<Option<Option<MaskId>>, LegacyError> {
        let clip = match &dto.clip_mask_id {
            None if dto.visibility_clip.is_none() => Clip::Whole,
            None => self.masked("", dto.visibility_clip, size)?,
            Some(id) => self.masked(id, dto.visibility_clip, size)?,
        };
        Ok(match clip {
            Clip::Empty => None,
            Clip::Whole => Some(None),
            Clip::Mask(id) => Some(Some(id)),
        })
    }

    /// A schema-5 fill mask as coverage, `None` when it covers nothing.
    fn fill_mask(&mut self, id: &'a str, size: [u32; 2]) -> Result<Option<MaskId>, LegacyError> {
        let gray = self.gray_pixels(id, size)?;
        let (bounds, bits) = match bits(size, None, |x, y| gray[y * size[0] as usize + x] >= 128)? {
            Some(found) => found,
            None => return Ok(None),
        };
        Ok(Some(self.store_mask(bounds, bits)))
    }

    /// The 8-bit mask `id` (none for "") within `rect`, as bits: 2.2.13
    /// counts a pixel of 128 or more as inside.
    fn masked(
        &mut self,
        id: &'a str,
        rect: Option<[i32; 4]>,
        size: [u32; 2],
    ) -> Result<Clip, LegacyError> {
        let key = (id, rect, size);
        if let Some(&stored) = self.clips.get(&key) {
            return Ok(stored);
        }
        let found = if id.is_empty() {
            bits(size, rect, |_, _| true)?
        } else {
            let gray = self.gray_pixels(id, size)?;
            bits(size, rect, |x, y| gray[y * size[0] as usize + x] >= 128)?
        };
        let clip = match found {
            None => Clip::Empty,
            Some((bounds, bits)) => {
                let whole = bounds == [0, 0, size[0] as i32, size[1] as i32]
                    && bits_full(bounds[2], bounds[3], &bits);
                if whole {
                    Clip::Whole
                } else {
                    Clip::Mask(self.store_mask(bounds, bits))
                }
            }
        };
        self.clips.insert(key, clip);
        Ok(clip)
    }

    fn gray_pixels(&self, id: &str, size: [u32; 2]) -> Result<Arc<[u8]>, LegacyError> {
        let gray = self
            .gray
            .get(id)
            .ok_or_else(|| damaged("a stroke's mask is missing"))?;
        if gray.size != size {
            return Err(damaged("a stroke's mask does not match the canvas"));
        }
        Ok(gray.pixels.clone())
    }

    fn store_mask(&mut self, bounds: [i32; 4], bits: Vec<u8>) -> MaskId {
        let id = MaskId(self.next_mask);
        self.next_mask += 1;
        self.store.masks.insert(
            id,
            Mask {
                bounds,
                bits: bits.into(),
            },
        );
        id
    }

    fn packed(&mut self, id: &'a str, size: [u32; 2]) -> Result<MaskId, LegacyError> {
        if let Some(&stored) = self.packed.get(id) {
            return Ok(stored);
        }
        let (canvas, mask) = self
            .binary
            .get(id)
            .ok_or_else(|| damaged("a binary mask is missing"))?;
        if *canvas != size {
            return Err(damaged("a binary mask does not match the canvas"));
        }
        let stored = MaskId(self.next_mask);
        self.next_mask += 1;
        self.store.masks.insert(stored, mask.clone());
        self.packed.insert(id, stored);
        Ok(stored)
    }
}

/// The pixels `inside` within `rect` and the canvas, as a mask's bounds and
/// bits; `None` when there are none.
fn bits(
    [width, height]: [u32; 2],
    rect: Option<[i32; 4]>,
    inside: impl Fn(usize, usize) -> bool,
) -> Result<Option<Bits>, LegacyError> {
    let (width, height) = (width as i32, height as i32);
    let [mut left, mut top, mut right, mut bottom] = [0, 0, width, height];
    if let Some([x, y, w, h]) = rect {
        let edge = *ugu_core::document::limits::CANVAS_EDGE.end() as i32;
        if x < 0 || y < 0 || w <= 0 || h <= 0 || x > edge - w || y > edge - h {
            return Err(damaged("a stroke has an invalid rectangle"));
        }
        left = x;
        top = y;
        right = right.min(x + w);
        bottom = bottom.min(y + h);
    }
    let at = |x: i32, y: i32| inside(x as usize, y as usize);
    let (mut min_x, mut min_y, mut max_x, mut max_y) = (i32::MAX, i32::MAX, -1, -1);
    for y in top..bottom {
        for x in left..right {
            if at(x, y) {
                min_x = min_x.min(x);
                max_x = max_x.max(x);
                min_y = min_y.min(y);
                max_y = max_y.max(y);
            }
        }
    }
    if max_x < 0 {
        return Ok(None);
    }
    let [w, h] = [max_x - min_x + 1, max_y - min_y + 1];
    let row = Mask::row_bytes(w);
    let mut bits = vec![0u8; row * h as usize];
    for y in 0..h {
        for x in 0..w {
            if at(min_x + x, min_y + y) {
                bits[y as usize * row + x as usize / 8] |= 0x80 >> (x % 8);
            }
        }
    }
    Ok(Some(([min_x, min_y, w, h], bits)))
}

fn bits_full(width: i32, height: i32, bits: &[u8]) -> bool {
    let row = Mask::row_bytes(width);
    (0..height as usize)
        .all(|y| (0..width as usize).all(|x| bits[y * row + x / 8] & (0x80 >> (x % 8)) != 0))
}

fn edge_ok(edge: u32) -> bool {
    ugu_core::document::limits::CANVAS_EDGE.contains(&edge)
}

/// `#AARRGGBB` or `#RRGGBB` as straight RGBA.
fn color(text: &str) -> Result<[u8; 4], LegacyError> {
    let refused = || LegacyError::Refused(Refusal::Color(text.chars().take(32).collect()));
    let digits = text.strip_prefix('#').ok_or_else(refused)?;
    if !digits.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(refused());
    }
    let channel = |index: usize| u8::from_str_radix(&digits[index * 2..index * 2 + 2], 16);
    let parsed = match digits.len() {
        8 => [channel(1), channel(2), channel(3), channel(0)],
        6 => [channel(0), channel(1), channel(2), Ok(255)],
        _ => return Err(refused()),
    };
    let mut rgba = [0; 4];
    for (target, value) in rgba.iter_mut().zip(parsed) {
        *target = value.map_err(|_| refused())?;
    }
    Ok(rgba)
}

fn motion(dto: &MotionDto) -> Result<Motion, LegacyError> {
    let style = match dto.style.as_str() {
        "classic" => MotionStyle::Classic,
        "smooth" => MotionStyle::Smooth,
        "stepped" => MotionStyle::Stepped,
        _ => return Err(damaged("the motion style is invalid")),
    };
    Ok(Motion {
        style,
        poses: dto.pose_count,
        detail: dto.detail,
        linked: dto.linked as f32,
        randomness: dto.randomness as f32,
        broken: dto.broken_line,
        break_amount: dto.break_amount as f32,
        break_range: dto.break_range as f32,
    })
}

fn blend(name: &str) -> Result<Blend, LegacyError> {
    Ok(match name {
        "normal" => Blend::Normal,
        "multiply" => Blend::Multiply,
        "screen" => Blend::Screen,
        "overlay" => Blend::Overlay,
        _ => return Err(damaged("a layer has an invalid blend mode")),
    })
}

fn sampling(name: &str) -> Result<Sampling, LegacyError> {
    Ok(match name {
        "nearest" => Sampling::Nearest,
        "smooth" => Sampling::Smooth,
        _ => return Err(damaged("an invalid sampling")),
    })
}

fn brush(dto: &BrushDto) -> Result<Brush, LegacyError> {
    let engine = match dto.engine.as_str() {
        "line" => BrushEngine::Line,
        "airbrush" => BrushEngine::Airbrush,
        "spray" => BrushEngine::Spray,
        _ => return Err(damaged("a stroke has an invalid brush engine")),
    };
    let tip = match dto.tip.as_str() {
        "round" => TipShape::Round,
        "square" => TipShape::Square,
        _ => return Err(damaged("a stroke has an invalid brush tip")),
    };
    Ok(Brush {
        engine,
        tip,
        opacity: dto.opacity as f32,
        flow: dto.flow as f32,
        hardness: dto.hardness as f32,
        spacing: dto.spacing as f32,
        scatter: dto.scatter as f32,
        particle_size: dto.particle_size as f32,
        density: dto.density as f32,
        size_dynamics: dto.size_dynamics as f32,
        opacity_dynamics: dto.opacity_dynamics as f32,
        size_jitter: dto.size_jitter as f32,
        animated_jitter: dto.animated_jitter,
        wobble_scale: dto.wobble_scale.unwrap_or(1.0) as f32,
        antialias: dto.antialiasing.unwrap_or(false),
    })
}

/// Qt's `m11 m12 m13 m21 m22 m23 m31 m32 m33`; only affine ones exist.
fn affine(m: &[f64; 9]) -> Result<Affine, LegacyError> {
    if !m.iter().all(|value| value.is_finite())
        || m[2].abs() > 1e-12
        || m[5].abs() > 1e-12
        || (m[8] - 1.0).abs() > 1e-12
    {
        return Err(damaged("a transform is not affine"));
    }
    Ok(Affine([m[0], m[3], m[6], m[1], m[4], m[7]]))
}

/// Base64 of Qt's `qCompress`: a big-endian length, then a zlib stream.
fn uncompress(base64: &str, length: u64) -> Result<Vec<u8>, LegacyError> {
    let bytes = decode_base64(base64).ok_or_else(|| damaged("invalid base64 data"))?;
    let invalid = || damaged("invalid compressed data");
    let (prefix, stream) = bytes.split_first_chunk::<4>().ok_or_else(invalid)?;
    if u64::from(u32::from_be_bytes(*prefix)) != length {
        return Err(invalid());
    }
    let mut output = Vec::with_capacity(length as usize);
    flate2::read::ZlibDecoder::new(stream)
        .take(length + 1)
        .read_to_end(&mut output)
        .map_err(|_| invalid())?;
    if output.len() as u64 != length {
        return Err(invalid());
    }
    Ok(output)
}

fn decode_base64(text: &str) -> Option<Vec<u8>> {
    let text = text.as_bytes();
    if !text.len().is_multiple_of(4) {
        return None;
    }
    let value = |byte: u8| match byte {
        b'A'..=b'Z' => Some(byte - b'A'),
        b'a'..=b'z' => Some(byte - b'a' + 26),
        b'0'..=b'9' => Some(byte - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    };
    let mut output = Vec::with_capacity(text.len() / 4 * 3);
    for (index, chunk) in text.as_chunks::<4>().0.iter().enumerate() {
        let last = index + 1 == text.len() / 4;
        let padding = chunk.iter().rev().take_while(|&&byte| byte == b'=').count();
        if padding > 2 || (padding > 0 && !last) {
            return None;
        }
        let mut word = 0u32;
        for &byte in &chunk[..4 - padding] {
            word = word << 6 | u32::from(value(byte)?);
        }
        word <<= 6 * padding as u32;
        output.extend_from_slice(&word.to_be_bytes()[1..4 - padding]);
    }
    Some(output)
}

#[cfg(test)]
mod tests;

// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! Stroke points, masks and raster assets, stored once and referenced by id
//! from layer operations. Large arrays are shared, so copying a document or
//! keeping an undo state does not copy them.

use std::collections::HashMap;
use std::sync::Arc;

use crate::ops::{AssetId, MaskId, Rgba8, StrokeId};

pub mod limits {
    pub const POINTS_PER_STROKE: usize = 200_000;
    pub const POINTS: usize = 250_000;
    /// Stored coordinates, in document pixels.
    pub const COORDINATE: f32 = 32_767.0;
    pub const STROKE_WIDTH: std::ops::RangeInclusive<f32> = 0.25..=512.0;
    pub const WOBBLE_SCALE: std::ops::RangeInclusive<f32> = 0.0..=2.0;
    /// Dab spacing, in brush widths.
    pub const SPACING: std::ops::RangeInclusive<f32> = 0.02..=2.0;
    /// How far spray particles land from the stroke, in brush widths.
    pub const SCATTER: std::ops::RangeInclusive<f32> = 0.0..=2.0;
    /// Spray particle size, in brush widths.
    pub const PARTICLE_SIZE: std::ops::RangeInclusive<f32> = 0.01..=1.0;
    /// Spray particles per sample, in sixes.
    pub const DENSITY: std::ops::RangeInclusive<f32> = 0.05..=4.0;
    pub const ASSET_PIXELS: u64 = 4096 * 4096;
    /// Everything the store holds, as stored.
    pub const BYTES: u64 = 128 * 1024 * 1024;
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point {
    pub x: f32,
    pub y: f32,
    /// 0 to 1.
    pub pressure: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BrushEngine {
    Line,
    Airbrush,
    Spray,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TipShape {
    Round,
    /// Square ends and mitred corners for lines, square dabs and particles.
    Square,
}

/// The brush a stroke was drawn with: 2.2.13's `BrushSettings`, with its
/// defaults.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Brush {
    pub engine: BrushEngine,
    pub tip: TipShape,
    pub opacity: f32,
    /// Each airbrush dab's and spray particle's share of `opacity`.
    pub flow: f32,
    /// How much of an airbrush dab's radius is solid before it fades.
    pub hardness: f32,
    /// See `limits::SPACING`.
    pub spacing: f32,
    /// See `limits::SCATTER`.
    pub scatter: f32,
    /// See `limits::PARTICLE_SIZE`.
    pub particle_size: f32,
    /// See `limits::DENSITY`.
    pub density: f32,
    /// How much pressure narrows the stroke: the width is scaled by
    /// `1 - size_dynamics + pressure * size_dynamics`.
    pub size_dynamics: f32,
    /// How much pressure fades the stroke, in the same way.
    pub opacity_dynamics: f32,
    /// How much spray particle sizes vary.
    pub size_jitter: f32,
    /// Spray particles land elsewhere on every frame.
    pub animated_jitter: bool,
    /// Multiplies the layer's wobble amount for this stroke.
    pub wobble_scale: f32,
    pub antialias: bool,
}

impl Brush {
    pub const DEFAULT: Self = Self {
        engine: BrushEngine::Line,
        tip: TipShape::Round,
        opacity: 1.0,
        flow: 1.0,
        hardness: 1.0,
        spacing: 0.15,
        scatter: 0.0,
        particle_size: 0.08,
        density: 1.0,
        size_dynamics: 0.8,
        opacity_dynamics: 0.0,
        size_jitter: 0.0,
        animated_jitter: false,
        wobble_scale: 1.0,
        antialias: false,
    };
}

impl Default for Brush {
    fn default() -> Self {
        Self::DEFAULT
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Stroke {
    pub points: Arc<[Point]>,
    pub color: Rgba8,
    pub width: f32,
    pub brush: Brush,
    /// Fixes the stroke's motion; never changes once drawn.
    pub seed: u64,
}

/// One bit per pixel inside `bounds`, rows padded to whole bytes, the
/// leftmost pixel in the highest bit.
#[derive(Clone, Debug, PartialEq)]
pub struct Mask {
    /// Left, top, width, height in document pixels.
    pub bounds: [i32; 4],
    pub bits: Arc<[u8]>,
}

impl Mask {
    pub fn row_bytes(width: i32) -> usize {
        (width.max(0) as usize).div_ceil(8)
    }

    pub fn contains(&self, x: i32, y: i32) -> bool {
        let [left, top, width, height] = self.bounds;
        let (column, row) = (x - left, y - top);
        if column < 0 || row < 0 || column >= width || row >= height {
            return false;
        }
        let byte = self.bits[row as usize * Self::row_bytes(width) + column as usize / 8];
        byte & (0x80 >> (column % 8)) != 0
    }

    /// The edges between selected and other pixels as closed loops of pixel
    /// corners, the selection on the right of each step; straight runs are
    /// one step. Pixels touching only at a corner are kept apart.
    pub fn outline(&self) -> Vec<Vec<[i32; 2]>> {
        let [left, top, width, height] = self.bounds;
        let [width, height] = [width as usize, height as usize];
        let row_bytes = Self::row_bytes(width as i32);
        let bits = &self.bits;
        let set = |x: usize, y: usize| {
            // Corners sit between pixels, so neighbours go one past either
            // side; those wrap to large values and read as unset.
            x < width && y < height && bits[y * row_bytes + x / 8] & (0x80 >> (x % 8)) != 0
        };
        // Whether each edge along a row (the top edges of rows 0..=height)
        // was traced; loops are started only from those.
        let mut traced = vec![false; (height + 1) * width];
        let empty = vec![0u8; row_bytes];
        let row = |y: usize| {
            if y < height {
                &bits[y * row_bytes..(y + 1) * row_bytes]
            } else {
                &empty[..]
            }
        };
        let mut loops = Vec::new();
        // Every loop has an edge along a row, found a byte at a time.
        for y in 0..=height {
            let (above, below) = (row(y.wrapping_sub(1)), row(y));
            for (byte, (&a, &b)) in above.iter().zip(below).enumerate() {
                let mut changed = a ^ b;
                while changed != 0 {
                    let bit = changed.leading_zeros() as usize;
                    changed &= !(0x80 >> bit);
                    let x = byte * 8 + bit;
                    if x >= width || traced[y * width + x] {
                        continue;
                    }
                    let (start, direction) = if b & (0x80 >> bit) != 0 {
                        ([x, y], RIGHT)
                    } else {
                        ([x + 1, y], LEFT)
                    };
                    let mut corners = Vec::new();
                    let (mut at, mut going) = (start, direction);
                    loop {
                        match going {
                            RIGHT => traced[at[1] * width + at[0]] = true,
                            LEFT => traced[at[1] * width + at[0] - 1] = true,
                            _ => {}
                        }
                        at = [
                            (at[0] as isize + STEP[going][0]) as usize,
                            (at[1] as isize + STEP[going][1]) as usize,
                        ];
                        let [x, y] = at;
                        let [top_left, top_right, bottom_left, bottom_right] = [
                            set(x.wrapping_sub(1), y.wrapping_sub(1)),
                            set(x, y.wrapping_sub(1)),
                            set(x.wrapping_sub(1), y),
                            set(x, y),
                        ];
                        let leaves = [
                            bottom_right && !top_right,
                            bottom_left && !bottom_right,
                            top_left && !bottom_left,
                            top_right && !top_left,
                        ];
                        // A right turn first keeps pixels that meet at a
                        // corner in loops of their own.
                        let turned = [1, 0, 3]
                            .map(|turn| (going + turn) % 4)
                            .into_iter()
                            .find(|&next| leaves[next])
                            .expect("an edge leaves every corner reached");
                        if turned != going {
                            corners.push([left + x as i32, top + y as i32]);
                        }
                        going = turned;
                        if at == start && going == direction {
                            break;
                        }
                    }
                    loops.push(corners);
                }
            }
        }
        loops
    }
}

/// Directions along pixel edges, clockwise on screen (y down), and the step
/// each takes.
const RIGHT: usize = 0;
const LEFT: usize = 2;
const STEP: [[isize; 2]; 4] = [[1, 0], [0, 1], [-1, 0], [0, -1]];

/// A raster image as normalized PNG bytes; decoding is the renderer's job.
#[derive(Clone, Debug, PartialEq)]
pub struct Asset {
    pub size: [u32; 2],
    pub png: Arc<[u8]>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Store {
    pub strokes: HashMap<StrokeId, Stroke>,
    pub masks: HashMap<MaskId, Mask>,
    pub assets: HashMap<AssetId, Asset>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum StoreError {
    StrokePoints(StrokeId),
    StrokePoint(StrokeId),
    StrokeWidth(StrokeId),
    StrokeBrush(StrokeId),
    TooManyPoints(usize),
    MaskSize(MaskId),
    AssetSize(AssetId),
    TooManyBytes(u64),
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::StrokePoints(id) => write!(f, "stroke {} has no points or too many", id.0),
            Self::StrokePoint(id) => write!(f, "stroke {} has a point out of range", id.0),
            Self::StrokeWidth(id) => write!(f, "stroke {} has an invalid width", id.0),
            Self::StrokeBrush(id) => write!(f, "stroke {} has a brush setting out of range", id.0),
            Self::TooManyPoints(count) => write!(f, "{count} points is over the limit"),
            Self::MaskSize(id) => write!(f, "mask {} does not match its bounds", id.0),
            Self::AssetSize(_) => write!(f, "an image is empty or over the size limit"),
            Self::TooManyBytes(bytes) => write!(f, "{bytes} bytes of data is over the limit"),
        }
    }
}

impl std::error::Error for StoreError {}

impl Store {
    /// Bytes held, as stored: 12 per point, mask bits, PNG bytes.
    pub fn bytes(&self) -> u64 {
        let points: usize = self
            .strokes
            .values()
            .map(|stroke| stroke.points.len())
            .sum();
        let masks: usize = self.masks.values().map(|mask| mask.bits.len()).sum();
        let assets: usize = self.assets.values().map(|asset| asset.png.len()).sum();
        (points * 12 + masks + assets) as u64
    }

    /// Checks every item and the totals; for a document just read.
    pub fn validate(&self) -> Result<(), StoreError> {
        for (&id, stroke) in &self.strokes {
            check_stroke(id, stroke)?;
        }
        for (&id, mask) in &self.masks {
            check_mask(id, mask)?;
        }
        for (&id, asset) in &self.assets {
            check_asset(id, asset)?;
        }
        self.check_totals()
    }

    /// Checks the totals only, for a store whose items were checked as they
    /// were added.
    pub fn check_totals(&self) -> Result<(), StoreError> {
        let points: usize = self
            .strokes
            .values()
            .map(|stroke| stroke.points.len())
            .sum();
        if points > limits::POINTS {
            return Err(StoreError::TooManyPoints(points));
        }
        let bytes = self.bytes();
        if bytes > limits::BYTES {
            return Err(StoreError::TooManyBytes(bytes));
        }
        Ok(())
    }
}

pub fn check_stroke(id: StrokeId, stroke: &Stroke) -> Result<(), StoreError> {
    if stroke.points.is_empty() || stroke.points.len() > limits::POINTS_PER_STROKE {
        return Err(StoreError::StrokePoints(id));
    }
    let in_range = |value: f32| value.abs() <= limits::COORDINATE;
    if !stroke.points.iter().all(|point| {
        in_range(point.x) && in_range(point.y) && (0.0..=1.0).contains(&point.pressure)
    }) {
        return Err(StoreError::StrokePoint(id));
    }
    if !limits::STROKE_WIDTH.contains(&stroke.width) {
        return Err(StoreError::StrokeWidth(id));
    }
    if !brush_in_range(&stroke.brush) {
        return Err(StoreError::StrokeBrush(id));
    }
    Ok(())
}

/// 2.2.13's `isValidBrushSettings`.
pub fn brush_in_range(brush: &Brush) -> bool {
    let unit = 0.0..=1.0;
    unit.contains(&brush.opacity)
        && brush.flow > 0.0
        && brush.flow <= 1.0
        && unit.contains(&brush.hardness)
        && limits::SPACING.contains(&brush.spacing)
        && limits::SCATTER.contains(&brush.scatter)
        && limits::PARTICLE_SIZE.contains(&brush.particle_size)
        && limits::DENSITY.contains(&brush.density)
        && unit.contains(&brush.size_dynamics)
        && unit.contains(&brush.opacity_dynamics)
        && unit.contains(&brush.size_jitter)
        && limits::WOBBLE_SCALE.contains(&brush.wobble_scale)
}

pub fn check_mask(id: MaskId, mask: &Mask) -> Result<(), StoreError> {
    let [_, _, width, height] = mask.bounds;
    let expected = (height.max(0) as usize).checked_mul(Mask::row_bytes(width));
    if width <= 0 || height <= 0 || expected != Some(mask.bits.len()) {
        return Err(StoreError::MaskSize(id));
    }
    Ok(())
}

pub fn check_asset(id: AssetId, asset: &Asset) -> Result<(), StoreError> {
    let pixels = u64::from(asset.size[0]) * u64::from(asset.size[1]);
    if pixels == 0 || pixels > limits::ASSET_PIXELS || asset.png.is_empty() {
        return Err(StoreError::AssetSize(id));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stroke(points: Vec<Point>) -> Stroke {
        Stroke {
            points: points.into(),
            color: Rgba8([0, 0, 0, 255]),
            width: 6.0,
            brush: Brush::default(),
            seed: 7,
        }
    }

    fn point(x: f32, y: f32) -> Point {
        Point {
            x,
            y,
            pressure: 0.5,
        }
    }

    #[test]
    fn copying_a_store_shares_its_arrays() {
        let mut store = Store::default();
        store
            .strokes
            .insert(StrokeId(1), stroke(vec![point(1.0, 2.0); 1000]));
        let copy = store.clone();
        assert!(Arc::ptr_eq(
            &store.strokes[&StrokeId(1)].points,
            &copy.strokes[&StrokeId(1)].points
        ));
        assert_eq!(store.bytes(), 12_000);
    }

    #[test]
    fn points_must_be_finite_and_in_range() {
        for bad in [
            point(f32::NAN, 0.0),
            point(40_000.0, 0.0),
            Point {
                x: 0.0,
                y: 0.0,
                pressure: 1.5,
            },
        ] {
            let mut store = Store::default();
            store
                .strokes
                .insert(StrokeId(1), stroke(vec![point(0.0, 0.0), bad]));
            assert_eq!(store.validate(), Err(StoreError::StrokePoint(StrokeId(1))));
        }
        let mut empty = Store::default();
        empty.strokes.insert(StrokeId(1), stroke(vec![]));
        assert_eq!(empty.validate(), Err(StoreError::StrokePoints(StrokeId(1))));
    }

    #[test]
    fn points_are_limited_per_stroke_and_in_total() {
        let mut store = Store::default();
        store.strokes.insert(
            StrokeId(1),
            stroke(vec![point(0.0, 0.0); limits::POINTS_PER_STROKE]),
        );
        assert_eq!(store.validate(), Ok(()));
        store
            .strokes
            .insert(StrokeId(2), stroke(vec![point(0.0, 0.0); 60_000]));
        assert_eq!(store.validate(), Err(StoreError::TooManyPoints(260_000)));
    }

    #[test]
    fn mask_bits_follow_the_bounds() {
        // 10 wide: two bytes a row; pixel 1 of row 0 and pixel 9 of row 1.
        let mask = Mask {
            bounds: [5, 5, 10, 2],
            bits: vec![0b0100_0000, 0, 0, 0b0100_0000].into(),
        };
        assert!(mask.contains(6, 5));
        assert!(mask.contains(14, 6));
        assert!(!mask.contains(5, 5));
        assert!(!mask.contains(15, 6));
        let mut store = Store::default();
        store.masks.insert(MaskId(1), mask);
        assert_eq!(store.validate(), Ok(()));
        store.masks.insert(
            MaskId(2),
            Mask {
                bounds: [0, 0, 10, 2],
                bits: vec![0; 3].into(),
            },
        );
        assert_eq!(store.validate(), Err(StoreError::MaskSize(MaskId(2))));
    }

    #[test]
    fn assets_are_limited_in_pixels() {
        let mut store = Store::default();
        let id = AssetId([1; 32]);
        store.assets.insert(
            id,
            Asset {
                size: [4097, 4096],
                png: vec![1].into(),
            },
        );
        assert_eq!(store.validate(), Err(StoreError::AssetSize(id)));
    }
}

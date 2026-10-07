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

/// The brush a stroke was drawn with. M2 settles the full set of fields.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Brush {
    pub engine: BrushEngine,
    pub opacity: f32,
    pub hardness: f32,
    pub antialias: bool,
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
}

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
            Self::StrokeWidth(id) => write!(f, "stroke {} has an invalid width or brush", id.0),
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
    let unit = 0.0..=1.0;
    if !limits::STROKE_WIDTH.contains(&stroke.width)
        || !unit.contains(&stroke.brush.opacity)
        || !unit.contains(&stroke.brush.hardness)
    {
        return Err(StoreError::StrokeWidth(id));
    }
    Ok(())
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
            brush: Brush {
                engine: BrushEngine::Line,
                opacity: 1.0,
                hardness: 1.0,
                antialias: false,
            },
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

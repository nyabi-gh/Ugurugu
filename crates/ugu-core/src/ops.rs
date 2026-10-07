// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! Ordered layer operations. A paint layer is evaluated per frame by applying
//! its operations in order to a transparent surface, so each operation acts on
//! the result of the ones before it and never on later ones.

/// A stroke's points, brush and seed, stored once and referenced by id.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct StrokeId(pub u32);

/// A binary mask with bounds, stored once and referenced by id.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct MaskId(pub u32);

/// A raster asset, named by the SHA-256 of its normalized PNG.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct AssetId(pub [u8; 32]);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sampling {
    Nearest,
    Smooth,
}

/// Row-major 2×3 affine transform in document pixels: x' = a·x + b·y + c,
/// y' = d·x + e·y + f.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Affine(pub [f64; 6]);

impl Affine {
    pub const IDENTITY: Self = Self([1.0, 0.0, 0.0, 0.0, 1.0, 0.0]);

    pub fn translation(x: f64, y: f64) -> Self {
        Self([1.0, 0.0, x, 0.0, 1.0, y])
    }
}

/// Straight-alpha sRGB colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rgba8(pub [u8; 4]);

/// How much and how a layer's or section's strokes move. M2 adds the motion
/// style fields; M0 needs only the override to exist.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Wobble {
    pub amount: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Op {
    /// Draws a stroke, moved by the frame's motion.
    Paint {
        stroke: StrokeId,
        clip: Option<MaskId>,
    },
    /// Removes coverage under a stroke from what is already there. Strokes
    /// drawn later are not affected.
    Erase {
        stroke: StrokeId,
        clip: Option<MaskId>,
    },
    /// Fills a coverage mask fixed when the fill was made. The fill does not
    /// move with motion and is not flood-filled again per frame.
    Fill {
        coverage: MaskId,
        color: Rgba8,
        antialias: bool,
        clip: Option<MaskId>,
    },
    PlaceImage {
        asset: AssetId,
        transform: Affine,
        sampling: Sampling,
    },
    /// Cuts the masked part of this frame's result so far and draws it
    /// transformed on top. Leaves the source in place when `keep_source`.
    TransformSelection {
        mask: MaskId,
        transform: Affine,
        sampling: Sampling,
        keep_source: bool,
    },
    /// Clears the masked part of this frame's result so far.
    ClearSelection { mask: MaskId },
    /// Changes the canvas: content keeps its pixels and moves by `offset`.
    Crop { offset: [i32; 2], size: [u32; 2] },
    /// Resizes the image: content so far is resampled to `size`.
    Resample { size: [u32; 2], sampling: Sampling },
    /// Evaluates its operations on a fresh transparent surface and draws the
    /// result over what is there. Merging makes these, so an eraser from a
    /// merged layer reaches only its own section. A section never changes the
    /// canvas.
    Isolated(Box<Section>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Section {
    pub ops: Vec<Op>,
    pub opacity: f32,
    /// The wobble of strokes in this section. Like a layer's, `None` follows
    /// the document, so a merged layer keeps moving as it did.
    pub wobble: Option<Wobble>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Blend {
    Normal,
    Multiply,
    Screen,
    Overlay,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PaintLayer {
    pub ops: Vec<Op>,
    pub opacity: f32,
    pub blend: Blend,
    pub clip_to_below: bool,
    pub wobble: Option<Wobble>,
    /// The canvas size before the first operation.
    pub initial_size: [u32; 2],
}

impl PaintLayer {
    /// The canvas size after the last operation.
    pub fn final_size(&self) -> [u32; 2] {
        final_size(&self.ops, self.initial_size)
    }
}

fn final_size(ops: &[Op], mut size: [u32; 2]) -> [u32; 2] {
    for op in ops {
        match op {
            Op::Crop { size: next, .. } | Op::Resample { size: next, .. } => size = *next,
            _ => {}
        }
    }
    size
}

fn changes_canvas(ops: &[Op]) -> bool {
    ops.iter().any(|op| match op {
        Op::Crop { .. } | Op::Resample { .. } => true,
        Op::Isolated(section) => changes_canvas(&section.ops),
        _ => false,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MergeRefusal {
    /// Blend modes other than Normal act on the layers below, which the merged
    /// layer would no longer see separately.
    Blend,
    /// A clipping relation would change what is clipped.
    Clipping,
    /// The upper layer starts on a different canvas than the lower one ends.
    CanvasEpoch,
    /// A crop or resample in the upper layer would also move the lower layer.
    CanvasChange,
}

/// Merges `above` into `below`, keeping the appearance of every frame.
///
/// Each layer becomes an isolated section that keeps its opacity and wobble,
/// and the merged layer is opaque Normal. Operations added after the merge
/// act on both sections' result.
pub fn merge_down(below: &PaintLayer, above: &PaintLayer) -> Result<PaintLayer, MergeRefusal> {
    if below.blend != Blend::Normal || above.blend != Blend::Normal {
        return Err(MergeRefusal::Blend);
    }
    if below.clip_to_below || above.clip_to_below {
        return Err(MergeRefusal::Clipping);
    }
    if below.final_size() != above.initial_size {
        return Err(MergeRefusal::CanvasEpoch);
    }
    if changes_canvas(&above.ops) || (below.opacity != 1.0 && changes_canvas(&below.ops)) {
        return Err(MergeRefusal::CanvasChange);
    }
    let section = |layer: &PaintLayer| {
        Op::Isolated(Box::new(Section {
            ops: layer.ops.clone(),
            opacity: layer.opacity,
            wobble: layer.wobble,
        }))
    };
    // An opaque lower layer needs no section; its operations stay as they are,
    // canvas changes included.
    let mut ops = if below.opacity == 1.0 {
        below.ops.clone()
    } else {
        vec![section(below)]
    };
    ops.push(section(above));
    Ok(PaintLayer {
        ops,
        opacity: 1.0,
        blend: Blend::Normal,
        clip_to_below: false,
        wobble: below.wobble,
        initial_size: below.initial_size,
    })
}

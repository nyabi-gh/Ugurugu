// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! The document: canvas, animation settings and the layer tree.
//!
//! Layers form a tree, so a parent always exists and cycles cannot be built;
//! validation checks only what the types cannot: limits, unique ids and the
//! canvas each paint layer ends on.

use crate::ops::{Blend, Motion, Op, PaintLayer, Rgba8, Wobble};
use crate::store::{Store, StoreError};

/// Limits a document must stay within. The same as 2.2.13's, plus the depth
/// of isolated sections that merging creates.
pub mod limits {
    pub const CANVAS_EDGE: std::ops::RangeInclusive<u32> = 1..=4096;
    pub const FRAMES: std::ops::RangeInclusive<u32> = 2..=60;
    pub const FRAMES_PER_SECOND: std::ops::RangeInclusive<f32> = 1.0..=50.0;
    pub const WOBBLE_AMOUNT: std::ops::RangeInclusive<f32> = 0.0..=12.0;
    /// Poses in a motion loop; a loop uses at most one per frame.
    pub const MOTION_POSES: std::ops::RangeInclusive<u32> = 1..=*FRAMES.end();
    pub const MOTION_DETAIL: std::ops::RangeInclusive<u32> = 1..=24;
    pub const BREAK_RANGE: std::ops::RangeInclusive<f32> = 2.0..=256.0;
    pub const LAYERS: usize = 256;
    /// Nesting of groups; a top-level layer is at depth 1.
    pub const LAYER_DEPTH: usize = 8;
    pub const LAYER_NAME_CHARS: usize = 256;
    /// Operations in all layers, isolated sections included.
    pub const OPERATIONS: usize = 20_000;
    pub const SECTION_DEPTH: usize = 16;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct LayerId(pub u32);

#[derive(Clone, Debug, PartialEq)]
pub struct Document {
    pub canvas: [u32; 2],
    pub background: Rgba8,
    pub frames: u32,
    pub frames_per_second: f32,
    pub wobble: Wobble,
    /// Bottom first.
    pub layers: Vec<Layer>,
    pub store: Store,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Layer {
    pub id: LayerId,
    pub name: String,
    pub visible: bool,
    /// Used as the reference for fills and selections, not exported.
    pub reference: bool,
    pub kind: LayerKind,
}

#[derive(Clone, Debug, PartialEq)]
pub enum LayerKind {
    Paint(PaintLayer),
    Group(Group),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Group {
    pub opacity: f32,
    pub blend: Blend,
    pub clip_to_below: bool,
    /// Bottom first.
    pub children: Vec<Layer>,
}

impl Layer {
    pub fn clip_to_below(&self) -> bool {
        match &self.kind {
            LayerKind::Paint(paint) => paint.clip_to_below,
            LayerKind::Group(group) => group.clip_to_below,
        }
    }

    pub fn opacity(&self) -> f32 {
        match &self.kind {
            LayerKind::Paint(paint) => paint.opacity,
            LayerKind::Group(group) => group.opacity,
        }
    }

    pub fn blend(&self) -> Blend {
        match &self.kind {
            LayerKind::Paint(paint) => paint.blend,
            LayerKind::Group(group) => group.blend,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum DocumentError {
    Canvas([u32; 2]),
    Frames(u32),
    FramesPerSecond(f32),
    Wobble(f32),
    Motion(Motion),
    Opacity(LayerId, f32),
    TooManyLayers(usize),
    TooDeep(LayerId),
    DuplicateLayer(LayerId),
    LayerName(LayerId),
    /// A paint layer ends on a canvas other than the document's.
    LayerCanvas(LayerId, [u32; 2]),
    TooManyOperations(usize),
    /// An isolated section is nested too deeply or changes the canvas.
    Section(LayerId),
    /// An operation refers to a stroke, mask or image that is not stored.
    MissingData(LayerId),
    Store(StoreError),
}

impl std::fmt::Display for DocumentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Canvas(size) => write!(f, "canvas {}x{} is outside the limits", size[0], size[1]),
            Self::Frames(frames) => write!(f, "{frames} frames is outside the limits"),
            Self::FramesPerSecond(fps) => {
                write!(f, "{fps} frames per second is outside the limits")
            }
            Self::Wobble(amount) => write!(f, "wobble {amount} is outside the limits"),
            Self::Motion(motion) => write!(f, "motion {motion:?} is outside the limits"),
            Self::Opacity(id, opacity) => write!(f, "layer {} has opacity {opacity}", id.0),
            Self::TooManyLayers(count) => write!(f, "{count} layers is over the limit"),
            Self::TooDeep(id) => write!(f, "layer {} is nested too deeply", id.0),
            Self::DuplicateLayer(id) => write!(f, "layer id {} is used twice", id.0),
            Self::LayerName(id) => write!(f, "layer {} has a name over the limit", id.0),
            Self::LayerCanvas(id, size) => {
                write!(f, "layer {} ends on a {}x{} canvas", id.0, size[0], size[1])
            }
            Self::TooManyOperations(count) => write!(f, "{count} operations is over the limit"),
            Self::Section(id) => write!(f, "layer {} has an invalid isolated section", id.0),
            Self::MissingData(id) => write!(f, "layer {} refers to data that is not stored", id.0),
            Self::Store(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for DocumentError {}

impl Document {
    /// A white canvas with one empty paint layer.
    pub fn new(canvas: [u32; 2]) -> Self {
        Self {
            canvas,
            background: Rgba8([255, 255, 255, 255]),
            frames: 30,
            frames_per_second: 25.0,
            wobble: Wobble::classic(1.6),
            layers: vec![Layer {
                id: LayerId(1),
                name: "Layer 1".to_owned(),
                visible: true,
                reference: false,
                kind: LayerKind::Paint(PaintLayer {
                    ops: Vec::new(),
                    opacity: 1.0,
                    blend: Blend::Normal,
                    clip_to_below: false,
                    wobble: None,
                    initial_size: canvas,
                }),
            }],
            store: Store::default(),
        }
    }

    /// Checks everything, every stored point included; for a document just
    /// read.
    pub fn validate(&self) -> Result<(), DocumentError> {
        self.store.validate().map_err(DocumentError::Store)?;
        self.check_structure()
    }

    /// Checks settings, the layer tree, operations, references and store
    /// totals, but not each stored item, which edits check as they add them.
    pub fn check_structure(&self) -> Result<(), DocumentError> {
        if !self
            .canvas
            .iter()
            .all(|edge| limits::CANVAS_EDGE.contains(edge))
        {
            return Err(DocumentError::Canvas(self.canvas));
        }
        if !limits::FRAMES.contains(&self.frames) {
            return Err(DocumentError::Frames(self.frames));
        }
        if !limits::FRAMES_PER_SECOND.contains(&self.frames_per_second) {
            return Err(DocumentError::FramesPerSecond(self.frames_per_second));
        }
        check_wobble(self.wobble)?;
        self.store.check_totals().map_err(DocumentError::Store)?;
        let mut walk = Walk {
            canvas: self.canvas,
            store: &self.store,
            ids: std::collections::HashSet::new(),
            operations: 0,
        };
        walk.layers(&self.layers, 1)?;
        if walk.ids.len() > limits::LAYERS {
            return Err(DocumentError::TooManyLayers(walk.ids.len()));
        }
        Ok(())
    }
}

fn check_wobble(wobble: Wobble) -> Result<(), DocumentError> {
    // `contains` is false for NaN.
    if !limits::WOBBLE_AMOUNT.contains(&wobble.amount) {
        return Err(DocumentError::Wobble(wobble.amount));
    }
    let motion = wobble.motion;
    let share = 0.0..=1.0;
    if limits::MOTION_POSES.contains(&motion.poses)
        && limits::MOTION_DETAIL.contains(&motion.detail)
        && share.contains(&motion.linked)
        && share.contains(&motion.randomness)
        && share.contains(&motion.break_amount)
        && limits::BREAK_RANGE.contains(&motion.break_range)
    {
        Ok(())
    } else {
        Err(DocumentError::Motion(motion))
    }
}

fn check_opacity(id: LayerId, opacity: f32) -> Result<(), DocumentError> {
    if (0.0..=1.0).contains(&opacity) {
        Ok(())
    } else {
        Err(DocumentError::Opacity(id, opacity))
    }
}

struct Walk<'a> {
    canvas: [u32; 2],
    store: &'a Store,
    ids: std::collections::HashSet<LayerId>,
    operations: usize,
}

impl Walk<'_> {
    fn layers(&mut self, layers: &[Layer], depth: usize) -> Result<(), DocumentError> {
        for layer in layers {
            if depth > limits::LAYER_DEPTH {
                return Err(DocumentError::TooDeep(layer.id));
            }
            if !self.ids.insert(layer.id) {
                return Err(DocumentError::DuplicateLayer(layer.id));
            }
            // Checked as it grows, so a huge tree is not walked to the end.
            if self.ids.len() > limits::LAYERS {
                return Err(DocumentError::TooManyLayers(self.ids.len()));
            }
            if layer.name.chars().count() > limits::LAYER_NAME_CHARS {
                return Err(DocumentError::LayerName(layer.id));
            }
            match &layer.kind {
                LayerKind::Group(group) => {
                    check_opacity(layer.id, group.opacity)?;
                    self.layers(&group.children, depth + 1)?;
                }
                LayerKind::Paint(paint) => self.paint(layer.id, paint)?,
            }
        }
        Ok(())
    }

    fn paint(&mut self, id: LayerId, paint: &PaintLayer) -> Result<(), DocumentError> {
        check_opacity(id, paint.opacity)?;
        if let Some(wobble) = paint.wobble {
            check_wobble(wobble)?;
        }
        if !paint
            .initial_size
            .iter()
            .all(|edge| limits::CANVAS_EDGE.contains(edge))
        {
            return Err(DocumentError::LayerCanvas(id, paint.initial_size));
        }
        let mut size = paint.initial_size;
        for op in &paint.ops {
            match op {
                Op::Crop { size: next, .. } | Op::Resample { size: next, .. } => {
                    if !next.iter().all(|edge| limits::CANVAS_EDGE.contains(edge)) {
                        return Err(DocumentError::LayerCanvas(id, *next));
                    }
                    size = *next;
                }
                _ => {}
            }
        }
        if size != self.canvas {
            return Err(DocumentError::LayerCanvas(id, size));
        }
        self.ops(id, &paint.ops, 0)
    }

    fn ops(&mut self, id: LayerId, ops: &[Op], section_depth: usize) -> Result<(), DocumentError> {
        self.operations += ops.len();
        if self.operations > limits::OPERATIONS {
            return Err(DocumentError::TooManyOperations(self.operations));
        }
        for op in ops {
            let stored = match op {
                Op::Paint { stroke, clip } | Op::Erase { stroke, clip } => {
                    self.store.strokes.contains_key(stroke)
                        && clip.is_none_or(|mask| self.store.masks.contains_key(&mask))
                }
                Op::Fill { coverage, clip, .. } => {
                    self.store.masks.contains_key(coverage)
                        && clip.is_none_or(|mask| self.store.masks.contains_key(&mask))
                }
                Op::PlaceImage { asset, .. } => self.store.assets.contains_key(asset),
                Op::TransformSelection { mask, .. } | Op::ClearSelection { mask } => {
                    self.store.masks.contains_key(mask)
                }
                Op::Crop { .. } | Op::Resample { .. } | Op::Isolated(_) => true,
            };
            if !stored {
                return Err(DocumentError::MissingData(id));
            }
            if let Op::Isolated(section) = op {
                let canvas_change = section
                    .ops
                    .iter()
                    .any(|op| matches!(op, Op::Crop { .. } | Op::Resample { .. }));
                if section_depth + 1 > limits::SECTION_DEPTH || canvas_change {
                    return Err(DocumentError::Section(id));
                }
                check_opacity(id, section.opacity)?;
                if let Some(wobble) = section.wobble {
                    check_wobble(wobble)?;
                }
                self.ops(id, &section.ops, section_depth + 1)?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ops::{MaskId, Sampling, Section, StrokeId};
    use crate::store::{Brush, BrushEngine, Point, Stroke};

    fn paint_layer(id: u32, ops: Vec<Op>, size: [u32; 2]) -> Layer {
        Layer {
            id: LayerId(id),
            name: format!("Layer {id}"),
            visible: true,
            reference: false,
            kind: LayerKind::Paint(PaintLayer {
                ops,
                opacity: 1.0,
                blend: Blend::Normal,
                clip_to_below: false,
                wobble: None,
                initial_size: size,
            }),
        }
    }

    fn group(id: u32, children: Vec<Layer>) -> Layer {
        Layer {
            id: LayerId(id),
            name: format!("Group {id}"),
            visible: true,
            reference: false,
            kind: LayerKind::Group(Group {
                opacity: 1.0,
                blend: Blend::Normal,
                clip_to_below: false,
                children,
            }),
        }
    }

    #[test]
    fn a_new_document_is_valid() {
        assert_eq!(Document::new([1024, 768]).validate(), Ok(()));
    }

    #[test]
    fn document_settings_must_be_within_limits() {
        let base = Document::new([64, 64]);
        let mut wide = Document::new([4097, 64]);
        assert_eq!(wide.validate(), Err(DocumentError::Canvas([4097, 64])));
        wide.canvas = [0, 64];
        assert_eq!(wide.validate(), Err(DocumentError::Canvas([0, 64])));
        let frames = Document {
            frames: 61,
            ..base.clone()
        };
        assert_eq!(frames.validate(), Err(DocumentError::Frames(61)));
        let fps = Document {
            frames_per_second: f32::NAN,
            ..base.clone()
        };
        assert!(matches!(
            fps.validate(),
            Err(DocumentError::FramesPerSecond(_))
        ));
        let wobble = Document {
            wobble: Wobble::classic(12.5),
            ..base.clone()
        };
        assert_eq!(wobble.validate(), Err(DocumentError::Wobble(12.5)));
        let at = |change: fn(&mut Motion)| {
            let mut document = base.clone();
            change(&mut document.wobble.motion);
            document.validate()
        };
        assert_eq!(at(|motion| motion.poses = 60), Ok(()));
        assert_eq!(at(|motion| motion.break_range = 2.0), Ok(()));
        let outside: [fn(&mut Motion); 7] = [
            |motion| motion.poses = 0,
            |motion| motion.poses = 61,
            |motion| motion.detail = 25,
            |motion| motion.linked = 1.5,
            |motion| motion.randomness = f32::NAN,
            |motion| motion.break_amount = -0.1,
            |motion| motion.break_range = 1.0,
        ];
        for change in outside {
            assert!(matches!(at(change), Err(DocumentError::Motion(_))));
        }
    }

    #[test]
    fn layer_ids_are_unique_across_the_tree() {
        let mut document = Document::new([64, 64]);
        document
            .layers
            .push(group(2, vec![paint_layer(1, vec![], [64, 64])]));
        assert_eq!(
            document.validate(),
            Err(DocumentError::DuplicateLayer(LayerId(1)))
        );
    }

    #[test]
    fn groups_nest_at_most_eight_deep() {
        let mut nested = paint_layer(100, vec![], [64, 64]);
        for id in 1..limits::LAYER_DEPTH as u32 {
            nested = group(id, vec![nested]);
        }
        let mut document = Document {
            layers: vec![nested.clone()],
            ..Document::new([64, 64])
        };
        assert_eq!(document.validate(), Ok(()));
        document.layers = vec![group(99, vec![nested])];
        assert_eq!(
            document.validate(),
            Err(DocumentError::TooDeep(LayerId(100)))
        );
    }

    #[test]
    fn layer_count_and_names_are_limited() {
        let layers = (1..=limits::LAYERS as u32 + 1)
            .map(|id| paint_layer(id, vec![], [64, 64]))
            .collect();
        let document = Document {
            layers,
            ..Document::new([64, 64])
        };
        assert_eq!(
            document.validate(),
            Err(DocumentError::TooManyLayers(limits::LAYERS + 1))
        );
        let mut named = Document::new([64, 64]);
        named.layers[0].name = "가".repeat(limits::LAYER_NAME_CHARS + 1);
        assert_eq!(named.validate(), Err(DocumentError::LayerName(LayerId(1))));
    }

    #[test]
    fn paint_layers_must_end_on_the_document_canvas() {
        let mut document = Document::new([64, 64]);
        document.layers.push(paint_layer(2, vec![], [32, 32]));
        assert_eq!(
            document.validate(),
            Err(DocumentError::LayerCanvas(LayerId(2), [32, 32]))
        );
        document.layers[1] = paint_layer(
            2,
            vec![Op::Resample {
                size: [64, 64],
                sampling: Sampling::Smooth,
            }],
            [32, 32],
        );
        assert_eq!(document.validate(), Ok(()));
    }

    fn store_with_stroke() -> Store {
        let mut store = Store::default();
        store.strokes.insert(
            StrokeId(0),
            Stroke {
                points: vec![Point {
                    x: 1.0,
                    y: 1.0,
                    pressure: 1.0,
                }]
                .into(),
                color: Rgba8([0, 0, 0, 255]),
                width: 6.0,
                brush: Brush {
                    engine: BrushEngine::Line,
                    opacity: 1.0,
                    hardness: 1.0,
                    antialias: false,
                    size_dynamics: 0.8,
                    wobble_scale: 1.0,
                    ..Brush::default()
                },
                seed: 1,
            },
        );
        store
    }

    #[test]
    fn operations_must_refer_to_stored_data() {
        let mut document = Document::new([64, 64]);
        let paint = Op::Paint {
            stroke: StrokeId(0),
            clip: None,
        };
        document.layers = vec![paint_layer(1, vec![paint.clone()], [64, 64])];
        assert_eq!(
            document.validate(),
            Err(DocumentError::MissingData(LayerId(1)))
        );
        document.store = store_with_stroke();
        assert_eq!(document.validate(), Ok(()));
        let clipped = Op::Paint {
            stroke: StrokeId(0),
            clip: Some(MaskId(3)),
        };
        document.layers = vec![paint_layer(1, vec![clipped], [64, 64])];
        assert_eq!(
            document.validate(),
            Err(DocumentError::MissingData(LayerId(1)))
        );
    }

    #[test]
    fn operations_are_counted_inside_sections() {
        let paint = Op::Paint {
            stroke: StrokeId(0),
            clip: None,
        };
        let section = Op::Isolated(Box::new(Section {
            ops: vec![paint.clone(); limits::OPERATIONS],
            opacity: 1.0,
            wobble: None,
        }));
        let mut document = Document::new([64, 64]);
        document.store = store_with_stroke();
        document.layers = vec![paint_layer(1, vec![section], [64, 64])];
        assert_eq!(
            document.validate(),
            Err(DocumentError::TooManyOperations(limits::OPERATIONS + 1))
        );
    }

    #[test]
    fn sections_are_limited_in_depth_and_keep_the_canvas() {
        let mut ops = vec![];
        for _ in 0..limits::SECTION_DEPTH {
            ops = vec![Op::Isolated(Box::new(Section {
                ops,
                opacity: 1.0,
                wobble: None,
            }))];
        }
        let mut document = Document::new([64, 64]);
        document.layers = vec![paint_layer(1, ops.clone(), [64, 64])];
        assert_eq!(document.validate(), Ok(()));
        let deeper = vec![Op::Isolated(Box::new(Section {
            ops,
            opacity: 1.0,
            wobble: None,
        }))];
        document.layers = vec![paint_layer(1, deeper, [64, 64])];
        assert_eq!(document.validate(), Err(DocumentError::Section(LayerId(1))));
        let cropping = Op::Isolated(Box::new(Section {
            ops: vec![Op::Crop {
                offset: [0, 0],
                size: [64, 64],
            }],
            opacity: 1.0,
            wobble: None,
        }));
        document.layers = vec![paint_layer(1, vec![cropping], [64, 64])];
        assert_eq!(document.validate(), Err(DocumentError::Section(LayerId(1))));
    }
}

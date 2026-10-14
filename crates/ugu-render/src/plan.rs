// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! The order in which a frame is put together from layer surfaces, made once
//! per layer structure and followed by every render: the editing canvas,
//! playback and export.
//!
//! Groups are always isolated: their children are put together on a fresh
//! transparent surface, which then goes over what is below with the group's
//! blend mode and opacity. A clipped layer or group is cut to the alpha of
//! the nearest unclipped sibling below it (its base) times the base's
//! opacity. Clipped layers whose base is not shown are left out, and a
//! hidden clipped layer does not change the base, as in 2.2.13.
//!
//! A plan is made for one frame: an animation folder shows only the child
//! exposed then, and a keyed layer is moved by its pose and its groups'
//! poses then. Groups pass their poses on to the layers in them, so every
//! render puts moved layer surfaces together the same way.

use ugu_core::document::{Document, Layer, LayerId, LayerKind};
use ugu_core::motion::frame_in_cycle;
use ugu_core::ops::{Affine, Blend, Op, Wobble};
use ugu_core::store::Store;

use crate::document::Purpose;
use crate::transform::Moved;

/// How a layer's or group's surface goes over what is below it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Composite {
    pub blend: Blend,
    pub opacity: f32,
    /// Cut to the current base; otherwise this becomes the base.
    pub clipped: bool,
    /// Where a paint layer's pixels go on this frame; `None` where they
    /// stay, and always for a group, whose layers take its move.
    pub moved: Option<Moved>,
}

impl Composite {
    /// Normal at full opacity, unclipped and in place.
    pub const PLAIN: Self = Self {
        blend: Blend::Normal,
        opacity: 1.0,
        clipped: false,
        moved: None,
    };
}

#[derive(Clone, Debug, PartialEq)]
pub enum Step {
    /// Starts a group's surface.
    Begin(LayerId),
    /// Puts the group begun last over the surface below it.
    End(LayerId, Composite),
    /// Puts a paint layer's surface over the current one.
    Paint(LayerId, Composite),
}

/// Which layers a tool that reads the frame sees.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reference {
    /// This paint layer alone, as it is without its visibility, opacity,
    /// blend mode, clipping and group.
    Layer(LayerId),
    /// The paint layers marked as references, put together.
    Marked,
    /// Every shown layer put together.
    Visible,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RenderPlan {
    pub purpose: Purpose,
    pub steps: Vec<Step>,
    /// The paint layers drawn, in step order, with whether they change from
    /// frame to frame.
    pub layers: Vec<(LayerId, bool)>,
    /// The frame within the cycle the plan was made for.
    pub frame: u32,
    /// Whether it differs from frame to frame for keys or folders, so it
    /// must only be drawn on `frame`.
    pub animated: bool,
    /// Paint layers in animation folders that this frame does not draw:
    /// other frames do, so their surfaces are worth keeping.
    pub cels: Vec<LayerId>,
}

impl RenderPlan {
    pub fn new(document: &Document, purpose: Purpose, frame: i64) -> Self {
        Self::with(document, purpose, false, frame)
    }

    /// What a tool reading `reference` on `frame` sees, drawn over
    /// transparency. Has no layers when there is nothing to read: no such
    /// paint layer, or no marked layer shown. A layer on its own is read
    /// where it is drawn, without its keys.
    pub fn reference(document: &Document, reference: Reference, frame: i64) -> Self {
        match reference {
            Reference::Layer(id) => {
                let mut plan = Self {
                    purpose: Purpose::Reference,
                    steps: Vec::new(),
                    layers: Vec::new(),
                    frame: frame_in_cycle(frame, document.frames),
                    animated: false,
                    cels: Vec::new(),
                };
                if let Some(LayerKind::Paint(paint)) = document.layer(id).map(|layer| &layer.kind) {
                    let wobble = paint.wobble.unwrap_or(document.wobble);
                    let moves = moves(&paint.ops, &document.store, document.wobble, wobble);
                    plan.layers.push((id, moves));
                    plan.steps.push(Step::Paint(id, Composite::PLAIN));
                }
                plan
            }
            Reference::Marked => Self::with(document, Purpose::Reference, true, frame),
            Reference::Visible => Self::with(document, Purpose::Reference, false, frame),
        }
    }

    fn with(document: &Document, purpose: Purpose, references: bool, frame: i64) -> Self {
        let mut plan = Self {
            purpose,
            steps: Vec::new(),
            layers: Vec::new(),
            frame: frame_in_cycle(frame, document.frames),
            animated: false,
            cels: Vec::new(),
        };
        let top: Vec<&Layer> = document.layers.iter().collect();
        plan.add(&top, document, references, None);
        plan.cels
            .retain(|id| plan.layers.iter().all(|(drawn, _)| drawn != id));
        plan
    }

    /// `id` alone on `frame`, where its keys and its groups' keys put it,
    /// at full opacity, Normal and unclipped, over transparency: a frame of
    /// an onion skin. Has no layers when `id` is missing or not shown.
    pub fn alone(document: &Document, id: LayerId, frame: i64) -> Self {
        let mut plan = Self {
            purpose: Purpose::Reference,
            steps: Vec::new(),
            layers: Vec::new(),
            frame: frame_in_cycle(frame, document.frames),
            animated: false,
            cels: Vec::new(),
        };
        let (Some(layer), Some(path)) = (document.layer(id), document.ancestors(id)) else {
            return plan;
        };
        if !layer.visible {
            return plan;
        }
        let outer = path
            .iter()
            .rev()
            .filter_map(|group| group.keys().at(plan.frame, document.frames))
            .reduce(|inner, outer| inner.then(outer));
        plan.add_one(layer, Composite::PLAIN, document, false, outer);
        plan.cels
            .retain(|id| plan.layers.iter().all(|(drawn, _)| drawn != id));
        plan
    }

    /// The plan for surfaces drawn at 1/`shrink` of the canvas size.
    pub fn scaled(&self, shrink: u32) -> Self {
        let scale = |composite: &Composite| Composite {
            moved: composite.moved.map(|moved| moved.scaled(shrink)),
            ..*composite
        };
        Self {
            steps: self
                .steps
                .iter()
                .map(|step| match step {
                    Step::Begin(id) => Step::Begin(*id),
                    Step::End(id, composite) => Step::End(*id, scale(composite)),
                    Step::Paint(id, composite) => Step::Paint(*id, scale(composite)),
                })
                .collect(),
            ..self.clone()
        }
    }

    /// The plan with every layer where it is drawn.
    pub fn in_place(&self) -> Self {
        let mut plan = self.clone();
        for step in &mut plan.steps {
            if let Step::Paint(_, composite) = step {
                composite.moved = None;
            }
        }
        plan
    }

    /// What the layers go over, premultiplied: transparency for a tool
    /// reading the frame, the document's background otherwise.
    pub fn background(&self, document: &Document) -> [u8; 4] {
        if self.purpose == Purpose::Reference {
            [0; 4]
        } else {
            crate::compose::premultiplied(document.background.0)
        }
    }

    /// Whether paint layer `id` is drawn, and whether it moves.
    pub fn moves(&self, id: LayerId) -> Option<bool> {
        self.layers
            .iter()
            .find(|(layer, _)| *layer == id)
            .map(|(_, moves)| *moves)
    }

    fn shown(&self, layer: &Layer, references: bool) -> bool {
        let (opacity, paint) = match &layer.kind {
            LayerKind::Paint(paint) => (paint.opacity, true),
            LayerKind::Group(group) => (group.opacity, false),
        };
        layer.visible
            && opacity > 0.0
            && !(self.purpose == Purpose::Export && layer.reference)
            && !(references && paint && !layer.reference)
    }

    /// Adds `layers`, held by groups that move them by `outer`.
    fn add(
        &mut self,
        layers: &[&Layer],
        document: &Document,
        references: bool,
        outer: Option<Affine>,
    ) {
        let mut has_base = false;
        for layer in layers {
            let (blend, opacity, clipped) = match &layer.kind {
                LayerKind::Paint(paint) => (paint.blend, paint.opacity, paint.clip_to_below),
                LayerKind::Group(group) => (group.blend, group.opacity, group.clip_to_below),
            };
            if !self.shown(layer, references) {
                if !clipped {
                    has_base = false;
                }
                continue;
            }
            if clipped && !has_base {
                continue;
            }
            has_base |= !clipped;
            let composite = Composite {
                blend,
                opacity,
                clipped,
                moved: None,
            };
            self.add_one(layer, composite, document, references, outer);
        }
    }

    /// Adds `layer`, going over what is below as `composite` says, held by
    /// groups that move it by `outer`.
    fn add_one(
        &mut self,
        layer: &Layer,
        mut composite: Composite,
        document: &Document,
        references: bool,
        outer: Option<Affine>,
    ) {
        let own = layer.keys().at(self.frame, document.frames);
        self.animated |= !layer.keys().is_empty();
        let moved = match (own, outer) {
            (Some(own), Some(outer)) => Some(own.then(outer)),
            (own, outer) => own.or(outer),
        };
        match &layer.kind {
            LayerKind::Paint(paint) => {
                let wobble = paint.wobble.unwrap_or(document.wobble);
                let moves = moves(&paint.ops, &document.store, document.wobble, wobble);
                composite.moved = moved.and_then(|moved| Moved::new(moved, document.pixel));
                self.layers.push((layer.id, moves));
                self.steps.push(Step::Paint(layer.id, composite));
            }
            LayerKind::Group(group) => {
                if group.is_folder() {
                    self.animated = true;
                    collect_paint(&group.children, &mut self.cels);
                }
                let shown: Vec<&Layer> = group
                    .children
                    .iter()
                    .filter(|child| group.shows(child.id, self.frame))
                    .collect();
                self.steps.push(Step::Begin(layer.id));
                self.add(&shown, document, references, moved);
                self.steps.push(Step::End(layer.id, composite));
            }
        }
    }
}

/// Adds the paint layers in `layers`, inside groups too, to `out`.
fn collect_paint(layers: &[Layer], out: &mut Vec<LayerId>) {
    for layer in layers {
        match &layer.kind {
            LayerKind::Paint(_) => out.push(layer.id),
            LayerKind::Group(group) => collect_paint(&group.children, out),
        }
    }
}

/// Whether any stroke in `ops` moves with the frame.
fn moves(ops: &[Op], store: &Store, document: Wobble, wobble: Wobble) -> bool {
    ops.iter().any(|op| match op {
        Op::Paint { stroke, .. } | Op::Erase { stroke, .. } => {
            store.strokes.get(stroke).is_some_and(|stroke| {
                wobble.motion.broken
                    || (wobble.amount > 0.0 && stroke.brush.wobble_scale > 0.0)
                    || stroke.brush.scatters_each_frame()
            })
        }
        Op::Isolated(section) => moves(
            &section.ops,
            store,
            document,
            section.wobble.unwrap_or(document),
        ),
        _ => false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use ugu_core::document::Group;
    use ugu_core::ops::{PaintLayer, Rgba8, Section, StrokeId};
    use ugu_core::store::{Brush, BrushEngine, Point, Stroke};

    fn paint(id: u32) -> Layer {
        Layer {
            id: LayerId(id),
            name: format!("{id}"),
            visible: true,
            reference: false,
            kind: LayerKind::Paint(PaintLayer {
                ops: Vec::new(),
                opacity: 1.0,
                blend: Blend::Normal,
                clip_to_below: false,
                wobble: None,
                initial_size: [8, 8],
                keys: Default::default(),
            }),
        }
    }

    fn group(id: u32, children: Vec<Layer>) -> Layer {
        Layer {
            id: LayerId(id),
            name: format!("{id}"),
            visible: true,
            reference: false,
            kind: LayerKind::Group(Group {
                opacity: 1.0,
                blend: Blend::Normal,
                clip_to_below: false,
                children,
                cels: None,
                keys: Default::default(),
            }),
        }
    }

    fn clipped(mut layer: Layer) -> Layer {
        match &mut layer.kind {
            LayerKind::Paint(paint) => paint.clip_to_below = true,
            LayerKind::Group(group) => group.clip_to_below = true,
        }
        layer
    }

    fn hidden(mut layer: Layer) -> Layer {
        layer.visible = false;
        layer
    }

    fn document(layers: Vec<Layer>) -> Document {
        let mut document = Document::new([8, 8]);
        document.layers = layers;
        document
    }

    /// The paint layers drawn, and whether each is clipped.
    fn drawn(layers: Vec<Layer>, purpose: Purpose) -> Vec<(u32, bool)> {
        RenderPlan::new(&document(layers), purpose, 0)
            .steps
            .iter()
            .filter_map(|step| match step {
                Step::Paint(id, composite) => Some((id.0, composite.clipped)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn clipped_layers_in_a_row_share_the_base_below_them() {
        let layers = vec![paint(1), clipped(paint(2)), clipped(paint(3)), paint(4)];
        assert_eq!(
            drawn(layers, Purpose::Display),
            [(1, false), (2, true), (3, true), (4, false)]
        );
    }

    #[test]
    fn clipped_layers_without_a_shown_base_are_left_out() {
        // At the bottom, over a hidden base, and over a transparent one.
        let mut faded = paint(4);
        if let LayerKind::Paint(paint) = &mut faded.kind {
            paint.opacity = 0.0;
        }
        let layers = vec![
            clipped(paint(1)),
            hidden(paint(2)),
            clipped(paint(3)),
            faded,
            clipped(paint(5)),
            paint(6),
            clipped(paint(7)),
        ];
        assert_eq!(drawn(layers, Purpose::Display), [(6, false), (7, true)]);
    }

    #[test]
    fn a_hidden_clipped_layer_keeps_the_base() {
        let layers = vec![paint(1), hidden(clipped(paint(2))), clipped(paint(3))];
        assert_eq!(drawn(layers, Purpose::Display), [(1, false), (3, true)]);
    }

    #[test]
    fn groups_are_bases_and_can_be_clipped() {
        let layers = vec![
            group(10, vec![clipped(paint(1)), paint(2), clipped(paint(3))]),
            clipped(group(11, vec![paint(4)])),
        ];
        let plan = RenderPlan::new(&document(layers), Purpose::Display, 0);
        let normal = |clipped| Composite {
            blend: Blend::Normal,
            opacity: 1.0,
            clipped,
            moved: None,
        };
        assert_eq!(
            plan.steps,
            [
                Step::Begin(LayerId(10)),
                Step::Paint(LayerId(2), normal(false)),
                Step::Paint(LayerId(3), normal(true)),
                Step::End(LayerId(10), normal(false)),
                Step::Begin(LayerId(11)),
                Step::Paint(LayerId(4), normal(false)),
                Step::End(LayerId(11), normal(true)),
            ]
        );
    }

    #[test]
    fn reference_layers_are_left_out_of_exports() {
        let mut reference = group(10, vec![paint(1)]);
        reference.reference = true;
        let layers = vec![reference, clipped(paint(2)), paint(3)];
        assert_eq!(
            drawn(layers.clone(), Purpose::Display),
            [(1, false), (2, true), (3, false)]
        );
        assert_eq!(drawn(layers, Purpose::Export), [(3, false)]);
    }

    fn with_stroke(document: &mut Document, wobble_scale: f32) -> Op {
        let id = StrokeId(document.store.strokes.len() as u32);
        document.store.strokes.insert(
            id,
            Stroke {
                points: Arc::from(vec![Point {
                    x: 1.0,
                    y: 1.0,
                    pressure: 1.0,
                }]),
                color: Rgba8([0, 0, 0, 255]),
                width: 2.0,
                brush: Brush {
                    engine: BrushEngine::Line,
                    opacity: 1.0,
                    hardness: 1.0,
                    antialias: true,
                    size_dynamics: 0.8,
                    wobble_scale,
                    ..Brush::default()
                },
                seed: 1,
            },
        );
        Op::Paint {
            stroke: id,
            clip: None,
        }
    }

    #[test]
    fn layers_without_motion_are_marked_still() {
        let mut document = document(vec![paint(1), paint(2), paint(3), paint(4)]);
        document.wobble = ugu_core::ops::Wobble::classic(1.6);
        let moving = with_stroke(&mut document, 1.0);
        let unscaled = with_stroke(&mut document, 0.0);
        let set = |document: &mut Document, index: usize, update: &dyn Fn(&mut PaintLayer)| {
            if let LayerKind::Paint(paint) = &mut document.layers[index].kind {
                update(paint);
            }
        };
        set(&mut document, 0, &|paint| paint.ops = vec![moving.clone()]);
        set(&mut document, 1, &|paint| {
            paint.ops = vec![moving.clone()];
            paint.wobble = Some(Wobble::classic(0.0));
        });
        set(&mut document, 2, &|paint| {
            paint.ops = vec![unscaled.clone()]
        });
        // A merged section keeps its own motion inside a still layer.
        set(&mut document, 3, &|paint| {
            paint.wobble = Some(Wobble::classic(0.0));
            paint.ops = vec![Op::Isolated(Box::new(Section {
                ops: vec![moving.clone()],
                opacity: 1.0,
                wobble: None,
            }))];
        });
        let plan = RenderPlan::new(&document, Purpose::Display, 0);
        assert_eq!(
            plan.layers,
            [
                (LayerId(1), true),
                (LayerId(2), false),
                (LayerId(3), false),
                (LayerId(4), true)
            ]
        );
    }

    fn keyed(mut layer: Layer, x: f64) -> Layer {
        layer.keys_mut().set(ugu_core::keys::Key {
            frame: 0,
            pose: ugu_core::keys::Pose {
                offset: [x, 0.0],
                ..ugu_core::keys::Pose::REST
            },
            ease: ugu_core::keys::Ease::Linear,
        });
        layer
    }

    fn moved_by(plan: &RenderPlan) -> Vec<(u32, Option<f64>)> {
        plan.steps
            .iter()
            .filter_map(|step| match step {
                Step::Paint(id, composite) => {
                    Some((id.0, composite.moved.map(|moved| moved.forward.0[2])))
                }
                Step::End(_, composite) => {
                    assert!(composite.moved.is_none(), "groups pass their moves on");
                    None
                }
                Step::Begin(_) => None,
            })
            .collect()
    }

    #[test]
    fn a_group_key_moves_each_layer_in_it_after_its_own() {
        let layers = vec![
            paint(1),
            keyed(group(10, vec![keyed(paint(2), 3.0), paint(3)]), 10.0),
        ];
        let plan = RenderPlan::new(&document(layers.clone()), Purpose::Display, 0);
        assert_eq!(
            moved_by(&plan),
            [(1, None), (2, Some(13.0)), (3, Some(10.0))]
        );
        assert!(plan.animated);
        assert!(!RenderPlan::new(&document(vec![paint(1)]), Purpose::Display, 0).animated);
        // In place, and pixel mode reading whole pixels.
        assert_eq!(
            moved_by(&plan.in_place()),
            [(1, None), (2, None), (3, None)]
        );
        let mut pixel = document(layers);
        pixel.pixel = true;
        let plan = RenderPlan::new(&pixel, Purpose::Display, 0);
        assert!(plan.steps.iter().all(|step| match step {
            Step::Paint(_, composite) => composite.moved.is_none_or(|moved| moved.nearest),
            _ => true,
        }));
        assert_eq!(moved_by(&plan.scaled(2))[1], (2, Some(6.5)));
    }

    #[test]
    fn an_animation_folder_draws_the_exposed_cel_and_keeps_the_others() {
        use ugu_core::keys::Exposure;
        let mut folder = group(10, vec![paint(1), group(11, vec![paint(2)]), paint(3)]);
        if let LayerKind::Group(content) = &mut folder.kind {
            content.cels = Some(vec![
                Exposure {
                    frame: 0,
                    cel: Some(LayerId(11)),
                },
                Exposure {
                    frame: 4,
                    cel: Some(LayerId(3)),
                },
                Exposure {
                    frame: 6,
                    cel: None,
                },
            ]);
        }
        let document = document(vec![paint(5), folder]);
        let at = |frame| {
            let plan = RenderPlan::new(&document, Purpose::Display, frame);
            let drawn: Vec<u32> = plan.layers.iter().map(|(id, _)| id.0).collect();
            let kept: Vec<u32> = plan.cels.iter().map(|id| id.0).collect();
            (drawn, kept)
        };
        assert_eq!(at(1), (vec![5, 2], vec![1, 3]));
        assert_eq!(at(4), (vec![5, 3], vec![1, 2]));
        assert_eq!(at(7), (vec![5], vec![1, 2, 3]));
        // Frames loop.
        assert_eq!(at(30 + 4), at(4));
    }

    #[test]
    fn a_layer_alone_keeps_its_moves_and_drops_how_it_is_put_together() {
        let mut inner = keyed(group(11, vec![paint(2), hidden(paint(3))]), 4.0);
        if let LayerKind::Group(content) = &mut inner.kind {
            content.opacity = 0.3;
            content.blend = Blend::Screen;
        }
        let document = document(vec![paint(1), keyed(group(10, vec![inner]), 10.0)]);
        let plan = RenderPlan::alone(&document, LayerId(11), 0);
        assert_eq!(plan.purpose, Purpose::Reference);
        assert_eq!(plan.steps.first(), Some(&Step::Begin(LayerId(11))));
        assert_eq!(
            plan.steps.last(),
            Some(&Step::End(LayerId(11), Composite::PLAIN))
        );
        assert_eq!(moved_by(&plan), [(2, Some(14.0))]);
        assert!(
            RenderPlan::alone(&document, LayerId(3), 0)
                .layers
                .is_empty()
        );
        assert!(
            RenderPlan::alone(&document, LayerId(99), 0)
                .layers
                .is_empty()
        );
    }
}

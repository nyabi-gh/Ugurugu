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

use ugu_core::document::{Document, Layer, LayerId, LayerKind};
use ugu_core::ops::{Blend, Op, Wobble};
use ugu_core::store::Store;

use crate::document::Purpose;

/// How a layer's or group's surface goes over what is below it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Composite {
    pub blend: Blend,
    pub opacity: f32,
    /// Cut to the current base; otherwise this becomes the base.
    pub clipped: bool,
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

#[derive(Clone, Debug, PartialEq)]
pub struct RenderPlan {
    pub purpose: Purpose,
    pub steps: Vec<Step>,
    /// The paint layers drawn, in step order, with whether they change from
    /// frame to frame.
    pub layers: Vec<(LayerId, bool)>,
}

impl RenderPlan {
    pub fn new(document: &Document, purpose: Purpose) -> Self {
        let mut plan = Self {
            purpose,
            steps: Vec::new(),
            layers: Vec::new(),
        };
        plan.add(&document.layers, document);
        plan
    }

    /// Whether paint layer `id` is drawn, and whether it moves.
    pub fn moves(&self, id: LayerId) -> Option<bool> {
        self.layers
            .iter()
            .find(|(layer, _)| *layer == id)
            .map(|(_, moves)| *moves)
    }

    fn shown(&self, layer: &Layer) -> bool {
        let opacity = match &layer.kind {
            LayerKind::Paint(paint) => paint.opacity,
            LayerKind::Group(group) => group.opacity,
        };
        layer.visible && opacity > 0.0 && !(self.purpose == Purpose::Export && layer.reference)
    }

    fn add(&mut self, layers: &[Layer], document: &Document) {
        let mut has_base = false;
        for layer in layers {
            let (blend, opacity, clipped) = match &layer.kind {
                LayerKind::Paint(paint) => (paint.blend, paint.opacity, paint.clip_to_below),
                LayerKind::Group(group) => (group.blend, group.opacity, group.clip_to_below),
            };
            if !self.shown(layer) {
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
            };
            match &layer.kind {
                LayerKind::Paint(paint) => {
                    let wobble = paint.wobble.unwrap_or(document.wobble);
                    let moves = moves(&paint.ops, &document.store, document.wobble, wobble);
                    self.layers.push((layer.id, moves));
                    self.steps.push(Step::Paint(layer.id, composite));
                }
                LayerKind::Group(group) => {
                    self.steps.push(Step::Begin(layer.id));
                    self.add(&group.children, document);
                    self.steps.push(Step::End(layer.id, composite));
                }
            }
        }
    }
}

/// Whether any stroke in `ops` moves with the frame.
fn moves(ops: &[Op], store: &Store, document: Wobble, wobble: Wobble) -> bool {
    ops.iter().any(|op| match op {
        Op::Paint { stroke, .. } | Op::Erase { stroke, .. } => {
            wobble.amount > 0.0
                && store
                    .strokes
                    .get(stroke)
                    .is_some_and(|stroke| stroke.brush.wobble_scale > 0.0)
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
        RenderPlan::new(&document(layers), purpose)
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
        let plan = RenderPlan::new(&document(layers), Purpose::Display);
        let normal = |clipped| Composite {
            blend: Blend::Normal,
            opacity: 1.0,
            clipped,
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
        let plan = RenderPlan::new(&document, Purpose::Display);
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
}

// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! Copying the selected part of a paint layer and pasting it as a new
//! layer. A copy keeps the layer's operations, so pasted strokes keep
//! moving, and ends with a clear of everything outside the selection, as
//! 2.2.13's copy does. It carries the stored data its operations use, so it
//! pastes into another document too: a different canvas is reached by a
//! crop at the end, the way a canvas change moves every layer.

use std::collections::HashMap;
use std::sync::Arc;

use crate::document::{Document, Layer, LayerId, LayerKind};
use crate::edit::Change;
use crate::ops::{Affine, AssetId, MaskId, Op, PaintLayer, Sampling, StrokeId};
use crate::selection::Selection;
use crate::store::{Asset, Store};

/// The selected part of a paint layer, as it was copied.
#[derive(Clone, Debug, PartialEq)]
pub struct Clip {
    pub layer: PaintLayer,
    /// The canvas copied from, which the layer ends on.
    pub canvas: [u32; 2],
    /// What the layer's operations use, by their ids in `layer`.
    pub store: Store,
    /// The selection copied.
    pub selection: Arc<Selection>,
}

/// Copies `selection`'s part of paint layer `layer`; `None` for another
/// kind of layer.
pub fn copy(document: &Document, layer: LayerId, selection: Arc<Selection>) -> Option<Clip> {
    let Some(LayerKind::Paint(paint)) = document.layer(layer).map(|layer| &layer.kind) else {
        return None;
    };
    let mut store = Store::default();
    let mut ops = paint.ops.clone();
    for op in &ops {
        each_reference(op, &mut |reference| match reference {
            Reference::Stroke(id) => {
                if let Some(stroke) = document.store.strokes.get(&id) {
                    store.strokes.insert(id, stroke.clone());
                }
            }
            Reference::Mask(id) => {
                if let Some(mask) = document.store.masks.get(&id) {
                    store.masks.insert(id, mask.clone());
                }
            }
            Reference::Asset(id) => {
                if let Some(asset) = document.store.assets.get(&id) {
                    store.assets.insert(id, asset.clone());
                }
            }
        });
    }
    if let Some(outside) = selection.invert() {
        let id = MaskId(store.masks.keys().map(|id| id.0 + 1).max().unwrap_or(0));
        store.masks.insert(id, outside.mask().clone());
        ops.push(Op::ClearSelection { mask: id });
    }
    Some(Clip {
        layer: PaintLayer {
            ops,
            clip_to_below: false,
            ..paint.clone()
        },
        canvas: document.canvas,
        store,
        selection,
    })
}

/// A new paint layer at `index` in `parent` (`None` for the top level)
/// drawing `clip`, and the selection it covers on this canvas (`None` when
/// it falls outside). Stored data equal to what this document has under the
/// same id is shared; the rest is added under new ids.
pub fn paste(
    document: &Document,
    clip: &Clip,
    parent: Option<LayerId>,
    index: usize,
    name: String,
) -> (LayerId, Vec<Change>, Option<Selection>) {
    let mut changes = Vec::new();
    let store = &document.store;
    let mut next_stroke = store.strokes.keys().map(|id| id.0 + 1).max().unwrap_or(0);
    let mut strokes = HashMap::new();
    let mut sorted: Vec<_> = clip.store.strokes.iter().collect();
    sorted.sort_by_key(|(id, _)| **id);
    for (&id, stroke) in sorted {
        let to = if store.strokes.get(&id) == Some(stroke) {
            id
        } else {
            let new = StrokeId(next_stroke);
            next_stroke += 1;
            changes.push(Change::InsertStroke(new, stroke.clone()));
            new
        };
        strokes.insert(id, to);
    }
    let mut next_mask = store.masks.keys().map(|id| id.0 + 1).max().unwrap_or(0);
    let mut masks = HashMap::new();
    let mut sorted: Vec<_> = clip.store.masks.iter().collect();
    sorted.sort_by_key(|(id, _)| **id);
    for (&id, mask) in sorted {
        let to = if store.masks.get(&id) == Some(mask) {
            id
        } else {
            let new = MaskId(next_mask);
            next_mask += 1;
            changes.push(Change::InsertMask(new, mask.clone()));
            new
        };
        masks.insert(id, to);
    }
    let mut sorted: Vec<_> = clip.store.assets.iter().collect();
    sorted.sort_by_key(|(id, _)| **id);
    for (&id, asset) in sorted {
        if !store.assets.contains_key(&id) {
            changes.push(Change::InsertAsset(id, asset.clone()));
        }
    }
    let mut ops: Vec<Op> = clip
        .layer
        .ops
        .iter()
        .map(|op| renamed(op, &strokes, &masks))
        .collect();
    let selection = if clip.canvas == document.canvas {
        Some(clip.selection.as_ref().clone())
    } else {
        ops.push(Op::Crop {
            offset: [0, 0],
            size: document.canvas,
        });
        clip.selection.cropped([0, 0], document.canvas)
    };
    let id = crate::command::next_layer_id(document);
    changes.push(Change::InsertLayer {
        parent,
        index,
        layer: Layer {
            id,
            name,
            visible: true,
            reference: false,
            kind: LayerKind::Paint(PaintLayer {
                ops,
                ..clip.layer.clone()
            }),
        },
    });
    (id, changes, selection)
}

/// A new paint layer at `index` in `parent` showing `asset` at its own size
/// in the middle of the canvas, and the selection it covers.
pub fn place_image(
    document: &Document,
    id: AssetId,
    asset: Asset,
    parent: Option<LayerId>,
    index: usize,
    name: String,
) -> (LayerId, Vec<Change>, Option<Selection>) {
    let canvas = document.canvas.map(i64::from);
    let size = asset.size.map(i64::from);
    // Whole pixels, so placing it does not resample it.
    let at = [0, 1].map(|axis| (canvas[axis] - size[axis]) / 2);
    let mut changes = Vec::new();
    if !document.store.assets.contains_key(&id) {
        changes.push(Change::InsertAsset(id, asset));
    }
    let layer = crate::command::next_layer_id(document);
    changes.push(Change::InsertLayer {
        parent,
        index,
        layer: Layer {
            id: layer,
            name,
            visible: true,
            reference: false,
            kind: LayerKind::Paint(PaintLayer {
                ops: vec![Op::PlaceImage {
                    asset: id,
                    transform: Affine::translation(at[0] as f64, at[1] as f64),
                    sampling: Sampling::Smooth,
                }],
                opacity: 1.0,
                blend: crate::ops::Blend::Normal,
                clip_to_below: false,
                wobble: None,
                initial_size: document.canvas,
            }),
        },
    });
    let corner = at.map(|value| value as f64);
    let selection = Selection::of_shape(
        &crate::selection::Shape::Rectangle(
            corner,
            [corner[0] + size[0] as f64, corner[1] + size[1] as f64],
        ),
        document.canvas,
    );
    (layer, changes, selection)
}

enum Reference {
    Stroke(StrokeId),
    Mask(MaskId),
    Asset(AssetId),
}

fn each_reference(op: &Op, each: &mut impl FnMut(Reference)) {
    match op {
        Op::Paint { stroke, clip } | Op::Erase { stroke, clip } => {
            each(Reference::Stroke(*stroke));
            if let Some(clip) = clip {
                each(Reference::Mask(*clip));
            }
        }
        Op::Fill { coverage, clip, .. } => {
            each(Reference::Mask(*coverage));
            if let Some(clip) = clip {
                each(Reference::Mask(*clip));
            }
        }
        Op::PlaceImage { asset, .. } => each(Reference::Asset(*asset)),
        Op::TransformSelection { mask, .. } | Op::ClearSelection { mask } => {
            each(Reference::Mask(*mask));
        }
        Op::Crop { .. } | Op::Resample { .. } => {}
        Op::Isolated(section) => {
            for op in &section.ops {
                each_reference(op, each);
            }
        }
    }
}

/// `op` with its stroke and mask ids replaced as the maps say.
fn renamed(op: &Op, strokes: &HashMap<StrokeId, StrokeId>, masks: &HashMap<MaskId, MaskId>) -> Op {
    let stroke = |id: &StrokeId| strokes.get(id).copied().unwrap_or(*id);
    let mask = |id: &MaskId| masks.get(id).copied().unwrap_or(*id);
    match op {
        Op::Paint { stroke: id, clip } => Op::Paint {
            stroke: stroke(id),
            clip: clip.as_ref().map(mask),
        },
        Op::Erase { stroke: id, clip } => Op::Erase {
            stroke: stroke(id),
            clip: clip.as_ref().map(mask),
        },
        Op::Fill {
            coverage,
            color,
            antialias,
            clip,
        } => Op::Fill {
            coverage: mask(coverage),
            color: *color,
            antialias: *antialias,
            clip: clip.as_ref().map(mask),
        },
        Op::TransformSelection {
            mask: id,
            transform,
            sampling,
            keep_source,
        } => Op::TransformSelection {
            mask: mask(id),
            transform: *transform,
            sampling: *sampling,
            keep_source: *keep_source,
        },
        Op::ClearSelection { mask: id } => Op::ClearSelection { mask: mask(id) },
        Op::Isolated(section) => {
            let mut section = section.as_ref().clone();
            section.ops = section
                .ops
                .iter()
                .map(|op| renamed(op, strokes, masks))
                .collect();
            Op::Isolated(Box::new(section))
        }
        Op::PlaceImage { .. } | Op::Crop { .. } | Op::Resample { .. } => op.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command;
    use crate::history::History;
    use crate::ops::{Rgba8, Wobble};
    use crate::selection::Shape;
    use crate::store::{Brush, Point, Stroke};

    fn stroke(y: f32) -> Stroke {
        Stroke {
            points: Arc::from(
                (0..10)
                    .map(|step| Point {
                        x: step as f32 * 10.0,
                        y,
                        pressure: 1.0,
                    })
                    .collect::<Vec<_>>(),
            ),
            color: Rgba8([10, 20, 30, 255]),
            width: 4.0,
            brush: Brush::default(),
            seed: 1,
        }
    }

    fn drawn(canvas: [u32; 2]) -> History {
        let mut history = History::new(Document::new(canvas), false);
        for y in [10.0, 40.0] {
            history
                .edit("Draw", |document| {
                    command::draw(document, LayerId(1), stroke(y), false, None)
                })
                .unwrap();
        }
        history
            .edit("Wobble", |document| {
                command::update_layer(document, LayerId(1), |layer| {
                    if let LayerKind::Paint(paint) = &mut layer.kind {
                        paint.wobble = Some(Wobble::classic(3.0));
                        paint.opacity = 0.5;
                        paint.clip_to_below = true;
                    }
                })
                .unwrap()
            })
            .unwrap();
        history
    }

    fn top(selection: [f64; 4], canvas: [u32; 2]) -> Arc<Selection> {
        Arc::new(
            Selection::of_shape(
                &Shape::Rectangle([selection[0], selection[1]], [selection[2], selection[3]]),
                canvas,
            )
            .unwrap(),
        )
    }

    fn paint_of(document: &Document, id: LayerId) -> &PaintLayer {
        match document.layer(id).map(|layer| &layer.kind) {
            Some(LayerKind::Paint(paint)) => paint,
            _ => panic!("a paint layer"),
        }
    }

    #[test]
    fn a_pasted_copy_keeps_its_strokes_moving_and_clears_outside_the_selection() {
        let mut history = drawn([100, 60]);
        let selection = top([0.0, 0.0, 100.0, 25.0], [100, 60]);
        let clip = copy(history.document(), LayerId(1), selection.clone()).unwrap();
        assert!(copy(history.document(), LayerId(9), selection.clone()).is_none());
        let mut pasted = None;
        history
            .edit("Paste", |document| {
                let (id, changes, selection) =
                    paste(document, &clip, None, 1, "Layer 2".to_owned());
                pasted = Some((id, selection));
                changes
            })
            .unwrap();
        let (id, selection_after) = pasted.unwrap();
        assert_eq!(selection_after.as_ref(), Some(selection.as_ref()));
        let document = history.document();
        let paint = paint_of(document, id);
        // Same document: the strokes are shared, the outside clear is added.
        let original = paint_of(document, LayerId(1));
        assert_eq!(paint.ops[..2], original.ops[..]);
        let Op::ClearSelection { mask } = paint.ops[2] else {
            panic!("a clear outside the selection");
        };
        assert_eq!(
            document.store.masks[&mask],
            *selection.invert().unwrap().mask()
        );
        assert_eq!(paint.wobble, Some(Wobble::classic(3.0)));
        assert_eq!(paint.opacity, 0.5);
        assert!(!paint.clip_to_below);
        assert_eq!(document.store.strokes.len(), 2);
    }

    #[test]
    fn a_copy_pastes_into_another_document_with_its_data_and_canvas() {
        let source = drawn([100, 60]);
        let clip = copy(
            source.document(),
            LayerId(1),
            top([0.0, 0.0, 100.0, 25.0], [100, 60]),
        )
        .unwrap();
        // The other document already uses stroke id 0 for something else.
        let mut other = History::new(Document::new([80, 80]), false);
        other
            .edit("Draw", |document| {
                command::draw(document, LayerId(1), stroke(70.0), false, None)
            })
            .unwrap();
        let mut pasted = None;
        other
            .edit("Paste", |document| {
                let (id, changes, selection) =
                    paste(document, &clip, None, 1, "Layer 2".to_owned());
                pasted = Some((id, selection));
                changes
            })
            .unwrap();
        let (id, selection) = pasted.unwrap();
        let document = other.document();
        let paint = paint_of(document, id);
        assert_eq!(paint.initial_size, [100, 60]);
        assert_eq!(paint.final_size(), [80, 80]);
        assert_eq!(
            paint.ops.last(),
            Some(&Op::Crop {
                offset: [0, 0],
                size: [80, 80]
            })
        );
        let Op::Paint { stroke: first, .. } = paint.ops[0] else {
            panic!("a stroke");
        };
        assert_ne!(first, StrokeId(0));
        assert_eq!(document.store.strokes[&first], stroke(10.0));
        assert_eq!(document.store.strokes[&StrokeId(0)], stroke(70.0));
        // The selection is the part of the copied one on this canvas.
        assert_eq!(selection.unwrap().mask().bounds, [0, 0, 80, 25]);
    }

    #[test]
    fn an_image_is_placed_whole_in_the_middle() {
        let document = Document::new([100, 60]);
        let asset = Asset {
            size: [41, 20],
            png: Arc::from(&b"png"[..]),
        };
        let id = AssetId([3; 32]);
        let (layer, changes, selection) =
            place_image(&document, id, asset.clone(), None, 1, "Image".to_owned());
        assert_eq!(layer, LayerId(2));
        assert_eq!(selection.unwrap().mask().bounds, [29, 20, 41, 20]);
        let [
            Change::InsertAsset(added, _),
            Change::InsertLayer { layer, .. },
        ] = &changes[..]
        else {
            panic!("the asset, then the layer");
        };
        assert_eq!(*added, id);
        let LayerKind::Paint(paint) = &layer.kind else {
            panic!("a paint layer");
        };
        assert_eq!(
            paint.ops,
            [Op::PlaceImage {
                asset: id,
                transform: Affine::translation(29.0, 20.0),
                sampling: Sampling::Smooth,
            }]
        );
    }
}

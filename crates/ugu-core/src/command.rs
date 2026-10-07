// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! User-level edits, built as lists of changes for `edit::commit`.
//!
//! Builders only read the document; nothing changes until the list is
//! committed, which checks it and applies all of it or none.

use crate::document::{Document, Layer, LayerId, LayerKind};
use crate::edit::{Change, EditError};
use crate::ops::{self, Blend, MaskId, Op, PaintLayer, Sampling, StrokeId};
use crate::store::Stroke;

/// A stroke id not in use. Ids freed by undo may be reused, because a new
/// edit clears the redo history that held them.
pub fn next_stroke_id(document: &Document) -> StrokeId {
    StrokeId(
        document
            .store
            .strokes
            .keys()
            .map(|id| id.0 + 1)
            .max()
            .unwrap_or(0),
    )
}

pub fn next_layer_id(document: &Document) -> LayerId {
    fn max(layers: &[Layer]) -> u32 {
        layers
            .iter()
            .map(|layer| match &layer.kind {
                LayerKind::Group(group) => layer.id.0.max(max(&group.children)),
                LayerKind::Paint(_) => layer.id.0,
            })
            .max()
            .unwrap_or(0)
    }
    LayerId(max(&document.layers) + 1)
}

/// Draws (or erases with) `stroke` on top of `layer`'s operations.
pub fn draw(
    document: &Document,
    layer: LayerId,
    stroke: Stroke,
    erase: bool,
    clip: Option<MaskId>,
) -> Vec<Change> {
    let id = next_stroke_id(document);
    let index = match document.layer(layer).map(|layer| &layer.kind) {
        Some(LayerKind::Paint(paint)) => paint.ops.len(),
        // The commit reports the missing or wrong layer.
        _ => 0,
    };
    let op = if erase {
        Op::Erase { stroke: id, clip }
    } else {
        Op::Paint { stroke: id, clip }
    };
    vec![
        Change::InsertStroke(id, stroke),
        Change::InsertOp { layer, index, op },
    ]
}

/// An empty paint layer at `index` in `parent` (`None` for the top level).
pub fn add_paint_layer(
    document: &Document,
    parent: Option<LayerId>,
    index: usize,
    name: String,
) -> (LayerId, Vec<Change>) {
    let id = next_layer_id(document);
    let layer = Layer {
        id,
        name,
        visible: true,
        reference: false,
        kind: LayerKind::Paint(PaintLayer {
            ops: Vec::new(),
            opacity: 1.0,
            blend: Blend::Normal,
            clip_to_below: false,
            wobble: None,
            initial_size: document.canvas,
        }),
    };
    (
        id,
        vec![Change::InsertLayer {
            parent,
            index,
            layer,
        }],
    )
}

pub fn remove_layer(id: LayerId) -> Vec<Change> {
    vec![Change::RemoveLayer(id)]
}

/// Moves `id` to `index` in `parent`, counted after it is taken out.
pub fn move_layer(
    document: &Document,
    id: LayerId,
    parent: Option<LayerId>,
    index: usize,
) -> Result<Vec<Change>, EditError> {
    let layer = document
        .layer(id)
        .ok_or(EditError::NoSuchLayer(id))?
        .clone();
    Ok(vec![
        Change::RemoveLayer(id),
        Change::InsertLayer {
            parent,
            index,
            layer,
        },
    ])
}

/// Changes a layer's own properties through `update`, which gets a copy.
pub fn update_layer(
    document: &Document,
    id: LayerId,
    update: impl FnOnce(&mut Layer),
) -> Result<Vec<Change>, EditError> {
    let mut layer = document
        .layer(id)
        .ok_or(EditError::NoSuchLayer(id))?
        .clone();
    update(&mut layer);
    Ok(vec![Change::ReplaceLayer(layer)])
}

/// Merges `above` into the paint layer right below it in the same group,
/// keeping every frame's appearance (ADR section 3). The merged layer keeps
/// the lower layer's id and name.
pub fn merge_down(document: &Document, above: LayerId) -> Result<Vec<Change>, EditError> {
    let (parent, index) = document
        .position(above)
        .ok_or(EditError::NoSuchLayer(above))?;
    let siblings = match parent {
        None => &document.layers,
        Some(group) => match &document
            .layer(group)
            .ok_or(EditError::NoSuchLayer(group))?
            .kind
        {
            LayerKind::Group(group) => &group.children,
            LayerKind::Paint(_) => return Err(EditError::NotGroup(group)),
        },
    };
    let upper = &siblings[index];
    let lower = index
        .checked_sub(1)
        .map(|below| &siblings[below])
        .ok_or(EditError::CannotMerge)?;
    let (LayerKind::Paint(upper_paint), LayerKind::Paint(lower_paint)) = (&upper.kind, &lower.kind)
    else {
        return Err(EditError::CannotMerge);
    };
    if upper.visible != lower.visible || upper.reference != lower.reference {
        return Err(EditError::CannotMerge);
    }
    let merged = ops::merge_down(lower_paint, upper_paint).map_err(EditError::Merge)?;
    let mut merged_layer = lower.clone();
    merged_layer.kind = LayerKind::Paint(merged);
    Ok(vec![
        Change::ReplaceLayer(merged_layer),
        Change::RemoveLayer(above),
    ])
}

/// Changes the canvas size; content keeps its pixels and moves by `offset`.
pub fn crop_canvas(document: &Document, offset: [i32; 2], size: [u32; 2]) -> Vec<Change> {
    canvas_change(document, size, Op::Crop { offset, size })
}

/// Resizes the image: everything drawn so far is resampled to `size`.
pub fn resample_image(document: &Document, size: [u32; 2], sampling: Sampling) -> Vec<Change> {
    canvas_change(document, size, Op::Resample { size, sampling })
}

fn canvas_change(document: &Document, size: [u32; 2], op: Op) -> Vec<Change> {
    fn each_paint(layers: &[Layer], changes: &mut Vec<Change>, op: &Op) {
        for layer in layers {
            match &layer.kind {
                LayerKind::Paint(paint) => changes.push(Change::InsertOp {
                    layer: layer.id,
                    index: paint.ops.len(),
                    op: op.clone(),
                }),
                LayerKind::Group(group) => each_paint(&group.children, changes, op),
            }
        }
    }
    let mut settings = document.settings();
    settings.canvas = size;
    let mut changes = vec![Change::SetSettings(settings)];
    each_paint(&document.layers, &mut changes, &op);
    changes
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::Group;
    use crate::edit::{Outcome, commit};
    use crate::ops::Rgba8;
    use crate::store::{Brush, BrushEngine, Point};

    fn stroke(x: f32) -> Stroke {
        Stroke {
            points: vec![Point {
                x,
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
            },
            seed: 1,
        }
    }

    /// Builds an edit from the document as it is, then commits it.
    fn run(
        document: &mut Document,
        build: impl FnOnce(&Document) -> Vec<Change>,
    ) -> Result<Outcome, EditError> {
        let changes = build(document);
        commit(document, changes)
    }

    fn undo(document: &mut Document, outcome: Outcome) {
        let Outcome::Committed(undo) = outcome else {
            panic!("nothing was committed");
        };
        assert!(matches!(commit(document, undo), Ok(Outcome::Committed(_))));
    }

    #[test]
    fn a_drawn_stroke_undoes_to_the_same_document() {
        let mut document = Document::new([64, 64]);
        let before = document.clone();
        let outcome = run(&mut document, |document| {
            draw(document, LayerId(1), stroke(2.0), false, None)
        })
        .unwrap();
        assert_eq!(document.validate(), Ok(()));
        assert_ne!(document, before);
        undo(&mut document, outcome);
        assert_eq!(document, before);
    }

    #[test]
    fn a_failing_edit_leaves_the_document_as_it_was() {
        let mut document = Document::new([64, 64]);
        let before = document.clone();
        // An invalid stroke fails on its own change.
        let bad = draw(&document, LayerId(1), stroke(f32::NAN), false, None);
        assert!(commit(&mut document, bad).is_err());
        assert_eq!(document, before);
        // A valid stroke followed by an operation on a missing layer.
        let mut changes = draw(&document, LayerId(1), stroke(2.0), false, None);
        changes.push(Change::InsertOp {
            layer: LayerId(9),
            index: 0,
            op: Op::ClearSelection { mask: MaskId(0) },
        });
        assert_eq!(
            commit(&mut document, changes),
            Err(EditError::NoSuchLayer(LayerId(9)))
        );
        assert_eq!(document, before);
    }

    #[test]
    fn data_still_in_use_cannot_be_removed() {
        let mut document = Document::new([64, 64]);
        run(&mut document, |document| {
            draw(document, LayerId(1), stroke(2.0), false, None)
        })
        .unwrap();
        let before = document.clone();
        let result = commit(&mut document, vec![Change::RemoveStroke(StrokeId(0))]);
        assert!(matches!(result, Err(EditError::Document(_))));
        assert_eq!(document, before);
    }

    #[test]
    fn an_edit_that_changes_nothing_is_not_committed() {
        let mut document = Document::new([64, 64]);
        let same = update_layer(&document, LayerId(1), |_| {}).unwrap();
        assert_eq!(commit(&mut document, same), Ok(Outcome::NoChange));
        let renamed =
            update_layer(&document, LayerId(1), |layer| layer.name = "Ink".to_owned()).unwrap();
        assert!(matches!(
            commit(&mut document, renamed),
            Ok(Outcome::Committed(_))
        ));
    }

    #[test]
    fn merging_down_commits_and_undoes() {
        let mut document = Document::new([64, 64]);
        let (upper, changes) = add_paint_layer(&document, None, 1, "Upper".to_owned());
        commit(&mut document, changes).unwrap();
        run(&mut document, |document| {
            draw(document, upper, stroke(3.0), false, None)
        })
        .unwrap();
        run(&mut document, |document| {
            update_layer(document, upper, |layer| {
                if let LayerKind::Paint(paint) = &mut layer.kind {
                    paint.opacity = 0.5;
                }
            })
            .unwrap()
        })
        .unwrap();
        let before = document.clone();
        let outcome = run(&mut document, |document| {
            merge_down(document, upper).unwrap()
        })
        .unwrap();
        assert_eq!(document.layers.len(), 1);
        assert_eq!(document.layers[0].id, LayerId(1));
        assert_eq!(document.validate(), Ok(()));
        undo(&mut document, outcome);
        assert_eq!(document, before);
        // The bottom layer has nothing below it.
        assert_eq!(
            merge_down(&document, LayerId(1)),
            Err(EditError::CannotMerge)
        );
    }

    #[test]
    fn cropping_reaches_paint_layers_inside_groups() {
        let mut document = Document::new([64, 64]);
        let nested = Layer {
            id: LayerId(3),
            name: "Inner".to_owned(),
            visible: true,
            reference: false,
            kind: LayerKind::Paint(PaintLayer {
                ops: Vec::new(),
                opacity: 1.0,
                blend: Blend::Normal,
                clip_to_below: false,
                wobble: None,
                initial_size: [64, 64],
            }),
        };
        let group = Layer {
            id: LayerId(2),
            name: "Group".to_owned(),
            visible: true,
            reference: false,
            kind: LayerKind::Group(Group {
                opacity: 1.0,
                blend: Blend::Normal,
                clip_to_below: false,
                children: vec![nested],
            }),
        };
        commit(
            &mut document,
            vec![Change::InsertLayer {
                parent: None,
                index: 1,
                layer: group,
            }],
        )
        .unwrap();
        let before = document.clone();
        let outcome = run(&mut document, |document| {
            crop_canvas(document, [8, 8], [80, 48])
        })
        .unwrap();
        assert_eq!(document.canvas, [80, 48]);
        let LayerKind::Paint(inner) = &document.layer(LayerId(3)).unwrap().kind else {
            unreachable!()
        };
        assert_eq!(inner.final_size(), [80, 48]);
        undo(&mut document, outcome);
        assert_eq!(document, before);
    }

    #[test]
    fn moving_a_layer_too_deep_is_refused() {
        let mut document = Document::new([64, 64]);
        let mut parent = None;
        for id in 2..=9 {
            let group = Layer {
                id: LayerId(id),
                name: format!("Group {id}"),
                visible: true,
                reference: false,
                kind: LayerKind::Group(Group {
                    opacity: 1.0,
                    blend: Blend::Normal,
                    clip_to_below: false,
                    children: Vec::new(),
                }),
            };
            let index = if parent.is_none() { 1 } else { 0 };
            commit(
                &mut document,
                vec![Change::InsertLayer {
                    parent,
                    index,
                    layer: group,
                }],
            )
            .unwrap();
            parent = Some(LayerId(id));
        }
        let before = document.clone();
        // Group 9 sits at depth 8, so a layer inside it would be at depth 9.
        let changes = move_layer(&document, LayerId(1), Some(LayerId(9)), 0).unwrap();
        assert!(matches!(
            commit(&mut document, changes),
            Err(EditError::Document(_))
        ));
        assert_eq!(document, before);
    }
}

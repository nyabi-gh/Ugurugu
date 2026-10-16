// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! User-level edits, built as lists of changes for `edit::commit`.
//!
//! Builders only read the document; nothing changes until the list is
//! committed, which checks it and applies all of it or none.

use crate::document::{Document, Group, Layer, LayerId, LayerKind};
use crate::edit::{Change, EditError, Settings};
use crate::keys::{self, Exposure, Keys};
use crate::ops::{self, Blend, MaskId, Op, PaintLayer, Rgba8, Sampling, StrokeId};
use crate::store::{FreeIds, Store, Stroke};

/// Stroke ids not in use. Ids freed by undo may be reused, because a new
/// edit clears the redo history that held them.
pub fn free_stroke_ids(store: &Store) -> FreeIds<impl Iterator<Item = u32> + Clone + '_> {
    FreeIds::new(store.strokes.keys().map(|id| id.0))
}

pub fn free_mask_ids(store: &Store) -> FreeIds<impl Iterator<Item = u32> + Clone + '_> {
    FreeIds::new(store.masks.keys().map(|id| id.0))
}

pub fn next_stroke_id(document: &Document) -> StrokeId {
    StrokeId(free_stroke_ids(&document.store).take())
}

pub fn next_layer_id(document: &Document) -> LayerId {
    fn ids(layers: &[Layer], out: &mut Vec<u32>) {
        for layer in layers {
            out.push(layer.id.0);
            if let LayerKind::Group(group) = &layer.kind {
                ids(&group.children, out);
            }
        }
    }
    // 0 stays unused, as when ids were one past the largest or 0.
    let mut used = vec![0];
    ids(&document.layers, &mut used);
    LayerId(FreeIds::new(used.into_iter()).take())
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
    let index = top_of(document, layer);
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

/// Fills `coverage`, a stored mask, with `color` on top of `layer`'s
/// operations, behind what is there when `behind`.
pub fn fill(
    document: &Document,
    layer: LayerId,
    coverage: MaskId,
    color: Rgba8,
    antialias: bool,
    clip: Option<MaskId>,
    behind: bool,
) -> Vec<Change> {
    let index = top_of(document, layer);
    let op = Op::Fill {
        coverage,
        color,
        antialias,
        clip,
        behind,
    };
    vec![Change::InsertOp { layer, index, op }]
}

/// Moves `mask`'s part of `layer` by `transform`, leaving the source in
/// place when `keep_source`, on top of its operations.
pub fn transform_selection(
    document: &Document,
    layer: LayerId,
    mask: MaskId,
    transform: ops::Affine,
    sampling: Sampling,
    keep_source: bool,
) -> Vec<Change> {
    let op = Op::TransformSelection {
        mask,
        transform,
        sampling,
        keep_source,
    };
    vec![Change::InsertOp {
        layer,
        index: top_of(document, layer),
        op,
    }]
}

/// Clears `mask`'s part of `layer`, on top of its operations.
pub fn clear_selection(document: &Document, layer: LayerId, mask: MaskId) -> Vec<Change> {
    vec![Change::InsertOp {
        layer,
        index: top_of(document, layer),
        op: Op::ClearSelection { mask },
    }]
}

/// Where an operation goes on top of `layer`'s; the commit reports a missing
/// or wrong layer.
fn top_of(document: &Document, layer: LayerId) -> usize {
    match document.layer(layer).map(|layer| &layer.kind) {
        Some(LayerKind::Paint(paint)) => paint.ops.len(),
        _ => 0,
    }
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
            keys: Keys::default(),
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

/// Removes `id`, and its exposures when it is a cel.
pub fn remove_layer(document: &Document, id: LayerId) -> Vec<Change> {
    let mut changes: Vec<Change> = unexpose(document, id, None).into_iter().collect();
    changes.push(Change::RemoveLayer(id));
    changes
}

/// When `id` is a cel, its folder with the exposures of `id` shown as
/// `instead`, or taken away with `None`.
fn unexpose(document: &Document, id: LayerId, instead: Option<LayerId>) -> Option<Change> {
    let (Some(parent), _) = document.position(id)? else {
        return None;
    };
    let mut folder = document.layer(parent)?.clone();
    let LayerKind::Group(Group {
        cels: Some(cels), ..
    }) = &mut folder.kind
    else {
        return None;
    };
    let before = cels.clone();
    match instead {
        Some(other) => {
            for exposure in cels.iter_mut().filter(|each| each.cel == Some(id)) {
                exposure.cel = Some(other);
            }
        }
        None => cels.retain(|exposure| exposure.cel != Some(id)),
    }
    (*cels != before).then_some(Change::ReplaceLayer(folder))
}

/// A copy of the paint layer `id` right above it, named `name`. The copy
/// draws the same stored strokes, masks and images as the original; a later
/// edit of either stores what it changes under new ids.
pub fn duplicate_layer(
    document: &Document,
    id: LayerId,
    name: String,
) -> Result<(LayerId, Vec<Change>), EditError> {
    let (parent, index) = document.position(id).ok_or(EditError::NoSuchLayer(id))?;
    let source = document.layer(id).ok_or(EditError::NoSuchLayer(id))?;
    if !matches!(source.kind, LayerKind::Paint(_)) {
        return Err(EditError::NotPaintLayer(id));
    }
    let copy = next_layer_id(document);
    let layer = Layer {
        id: copy,
        name,
        ..source.clone()
    };
    Ok((
        copy,
        vec![Change::InsertLayer {
            parent,
            index: index + 1,
            layer,
        }],
    ))
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
    let leaves = document.position(id).map(|(from, _)| from) != Some(parent);
    let mut changes: Vec<Change> = unexpose(document, id, None)
        .filter(|_| leaves)
        .into_iter()
        .collect();
    changes.extend([
        Change::RemoveLayer(id),
        Change::InsertLayer {
            parent,
            index,
            layer,
        },
    ]);
    Ok(changes)
}

/// Puts `child` in a new, empty-styled group that takes its place. A
/// clipped `child` would have no base inside it, so the group is clipped in
/// its stead.
pub fn wrap_in_group(
    document: &Document,
    child: LayerId,
    name: String,
) -> Result<(LayerId, Vec<Change>), EditError> {
    let (parent, index) = document
        .position(child)
        .ok_or(EditError::NoSuchLayer(child))?;
    let id = next_layer_id(document);
    let mut inside = document.layer(child).expect("found above").clone();
    let clip_to_below = std::mem::take(match &mut inside.kind {
        LayerKind::Paint(paint) => &mut paint.clip_to_below,
        LayerKind::Group(group) => &mut group.clip_to_below,
    });
    let group = Layer {
        id,
        name,
        visible: true,
        reference: false,
        kind: LayerKind::Group(Group {
            opacity: 1.0,
            blend: Blend::Normal,
            clip_to_below,
            children: vec![inside],
            keys: Keys::default(),
            cels: None,
        }),
    };
    // A cel's group takes its place on the frames it was shown.
    let repointed = unexpose(document, child, Some(id));
    let mut changes: Vec<Change> = repointed.into_iter().collect();
    changes.extend([
        Change::RemoveLayer(child),
        Change::InsertLayer {
            parent,
            index,
            layer: group,
        },
    ]);
    Ok((id, changes))
}

/// Puts `child` in a new animation folder that takes its place and shows
/// it from the first frame.
pub fn make_animation_folder(
    document: &Document,
    child: LayerId,
    name: String,
) -> Result<(LayerId, Vec<Change>), EditError> {
    let (id, mut changes) = wrap_in_group(document, child, name)?;
    let Some(Change::InsertLayer { layer, .. }) = changes.last_mut() else {
        unreachable!("the group goes in last");
    };
    if let LayerKind::Group(group) = &mut layer.kind {
        group.cels = Some(vec![Exposure {
            frame: 0,
            cel: Some(child),
        }]);
    }
    Ok((id, changes))
}

/// An empty paint cel `name` on top of `folder`, shown from `frame`.
pub fn new_cel(
    document: &Document,
    folder: LayerId,
    frame: u32,
    name: String,
) -> Result<(LayerId, Vec<Change>), EditError> {
    let mut layer = document
        .layer(folder)
        .ok_or(EditError::NoSuchLayer(folder))?
        .clone();
    let LayerKind::Group(Group {
        cels: Some(cels),
        children,
        ..
    }) = &mut layer.kind
    else {
        return Err(EditError::NotGroup(folder));
    };
    let (id, added) = add_paint_layer(document, None, 0, name);
    let Some(Change::InsertLayer { layer: cel, .. }) = added.into_iter().next() else {
        unreachable!("one layer is added");
    };
    children.push(cel);
    keys::expose(cels, frame, Some(id));
    Ok((id, vec![Change::ReplaceLayer(layer)]))
}

/// Changes the document's settings; keys and exposures past a shorter loop
/// go with it.
pub fn set_settings(document: &Document, settings: Settings) -> Vec<Change> {
    let mut changes = Vec::new();
    if settings.frames < document.frames {
        each_layer(&document.layers, &mut |layer| {
            if let Some(trimmed) = keys::trimmed(layer, settings.frames) {
                changes.push(Change::ReplaceLayer(trimmed));
                return false;
            }
            true
        });
    }
    changes.push(Change::SetSettings(settings));
    changes
}

/// Visits layers outermost first; `visit` returns whether to go into a
/// group.
fn each_layer(layers: &[Layer], visit: &mut impl FnMut(&Layer) -> bool) {
    for layer in layers {
        if visit(layer)
            && let LayerKind::Group(group) = &layer.kind
        {
            each_layer(&group.children, visit);
        }
    }
}

/// Puts `group`'s children in its place, in the same order. The group's own
/// opacity, blend mode, visibility and reference flag go with it; the
/// children of a clipped group come out clipped to its base, as
/// `wrap_in_group` puts a clipped layer back.
pub fn ungroup(document: &Document, group: LayerId) -> Result<Vec<Change>, EditError> {
    let (parent, index) = document
        .position(group)
        .ok_or(EditError::NoSuchLayer(group))?;
    let Some(LayerKind::Group(content)) = document.layer(group).map(|layer| &layer.kind) else {
        return Err(EditError::NotGroup(group));
    };
    let mut changes: Vec<Change> = unexpose(document, group, None).into_iter().collect();
    changes.push(Change::RemoveLayer(group));
    changes.extend(content.children.iter().enumerate().map(|(offset, child)| {
        let mut layer = child.clone();
        if content.clip_to_below {
            match &mut layer.kind {
                LayerKind::Paint(paint) => paint.clip_to_below = true,
                LayerKind::Group(group) => group.clip_to_below = true,
            }
        }
        Change::InsertLayer {
            parent,
            index: index + offset,
            layer,
        }
    }));
    Ok(changes)
}

/// Moves `id` to the top of `group`, or with `None` to the top level right
/// above the top-level layer that holds it.
pub fn move_to_group(
    document: &Document,
    id: LayerId,
    group: Option<LayerId>,
) -> Result<Vec<Change>, EditError> {
    let index = match group {
        Some(group) => match document.layer(group).map(|layer| &layer.kind) {
            // Taking `id` out cannot change this count unless it is a child,
            // and then it is put back on top all the same.
            Some(LayerKind::Group(content)) => content.children.len().saturating_sub(usize::from(
                content.children.iter().any(|child| child.id == id),
            )),
            Some(LayerKind::Paint(_)) => return Err(EditError::NotGroup(group)),
            None => return Err(EditError::NoSuchLayer(group)),
        },
        None => {
            let top = document
                .layers
                .iter()
                .position(|layer| layer.id == id || contains(layer, id))
                .ok_or(EditError::NoSuchLayer(id))?;
            if document.layers[top].id == id {
                return Ok(Vec::new());
            }
            top + 1
        }
    };
    if document.position(id) == Some((group, index)) {
        return Ok(Vec::new());
    }
    move_layer(document, id, group, index)
}

fn contains(layer: &Layer, id: LayerId) -> bool {
    match &layer.kind {
        LayerKind::Group(group) => group
            .children
            .iter()
            .any(|child| child.id == id || contains(child, id)),
        LayerKind::Paint(_) => false,
    }
}

/// The layers `parent` holds, bottom first; `None` for the top level.
pub fn siblings(document: &Document, parent: Option<LayerId>) -> Option<&[Layer]> {
    match parent {
        None => Some(&document.layers),
        Some(id) => match &document.layer(id)?.kind {
            LayerKind::Group(group) => Some(&group.children),
            LayerKind::Paint(_) => None,
        },
    }
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
    let siblings = siblings(document, parent).expect("a layer's container is a group");
    let upper = &siblings[index];
    let lower = index
        .checked_sub(1)
        .map(|below| &siblings[below])
        .ok_or(EditError::CannotMerge)?;
    let (LayerKind::Paint(upper_paint), LayerKind::Paint(lower_paint)) = (&upper.kind, &lower.kind)
    else {
        return Err(EditError::CannotMerge);
    };
    if parent
        .and_then(|parent| document.layer(parent)?.cels())
        .is_some()
    {
        return Err(EditError::Merge(ops::MergeRefusal::Animated));
    }
    if upper.visible != lower.visible || upper.reference != lower.reference {
        return Err(EditError::CannotMerge);
    }
    // A layer clipped to `above` would then be clipped to both.
    if siblings.get(index + 1).is_some_and(Layer::clip_to_below) {
        return Err(EditError::Merge(ops::MergeRefusal::Clipping));
    }
    let moved = [lower_paint, upper_paint]
        .iter()
        .map(|paint| ops::fixed_move(&paint.keys).map(|moved| moved.is_some()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(EditError::Merge)?
        .contains(&true);
    let mut around = parent;
    while let Some(group) = around {
        if moved
            && document
                .layer(group)
                .is_some_and(|group| !group.keys().is_empty())
        {
            return Err(EditError::Merge(ops::MergeRefusal::Animated));
        }
        around = document.position(group).and_then(|(parent, _)| parent);
    }
    let all = MaskId(free_mask_ids(&document.store).take());
    let merged =
        ops::merge_down(lower_paint, upper_paint, all, document.pixel).map_err(EditError::Merge)?;
    let mut merged_layer = lower.clone();
    merged_layer.kind = LayerKind::Paint(merged);
    let mut changes = Vec::new();
    if moved {
        let whole = crate::selection::Selection::all(document.canvas)
            .ok_or(EditError::CannotMerge)?
            .mask()
            .clone();
        changes.push(Change::InsertMask(all, whole));
    }
    changes.extend([
        Change::ReplaceLayer(merged_layer),
        Change::RemoveLayer(above),
    ]);
    Ok(changes)
}

/// Changes the canvas size; content keeps its pixels and moves by `offset`.
pub fn crop_canvas(document: &Document, offset: [i32; 2], size: [u32; 2]) -> Vec<Change> {
    let shift = offset.map(f64::from);
    let point = |[x, y]: [f64; 2]| [x + shift[0], y + shift[1]];
    let mut changes = rekeyed(document, &point, &|offset| offset);
    changes.extend(canvas_change(document, size, Op::Crop { offset, size }));
    changes
}

/// Resizes the image: everything drawn so far is resampled to `size`.
pub fn resample_image(document: &Document, size: [u32; 2], sampling: Sampling) -> Vec<Change> {
    let factor: [f64; 2] =
        std::array::from_fn(|axis| f64::from(size[axis]) / f64::from(document.canvas[axis]));
    let scale = |[x, y]: [f64; 2]| [x * factor[0], y * factor[1]];
    let mut changes = rekeyed(document, &scale, &scale);
    changes.extend(canvas_change(
        document,
        size,
        Op::Resample { size, sampling },
    ));
    changes
}

/// The keyed layers with their pivots moved by `point` and their key
/// offsets by `offset`, as what they move goes with a canvas change.
fn rekeyed(
    document: &Document,
    point: &dyn Fn([f64; 2]) -> [f64; 2],
    offset: &dyn Fn([f64; 2]) -> [f64; 2],
) -> Vec<Change> {
    let mut changes = Vec::new();
    each_layer(&document.layers, &mut |layer| {
        if !layer.keys().is_empty() {
            let mut copy = layer.clone();
            let keys = copy.keys_mut();
            keys.pivot = point(keys.pivot);
            for key in &mut keys.keys {
                key.pose.offset = offset(key.pose.offset);
            }
            changes.push(Change::ReplaceLayer(copy));
        }
        true
    });
    changes
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
                size_dynamics: 0.8,
                wobble_scale: 1.0,
                ..Brush::default()
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
    fn ids_at_the_end_of_their_range_leave_new_ones_to_take() {
        let mut document = Document::new([64, 64]);
        document.layers[0].id = LayerId(u32::MAX);
        for x in [u32::MAX - 1, u32::MAX] {
            document.store.strokes.insert(StrokeId(x), stroke(1.0));
        }
        for _ in 0..2 {
            run(&mut document, |document| {
                add_paint_layer(document, None, 0, String::new()).1
            })
            .unwrap();
        }
        let layer = document.layers[0].id;
        for _ in 0..2 {
            run(&mut document, |document| {
                draw(document, layer, stroke(2.0), false, None)
            })
            .unwrap();
        }
        assert_eq!(document.layers.len(), 3);
        assert_eq!(document.store.strokes.len(), 4);
        assert_eq!(document.validate(), Ok(()));
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
                keys: Default::default(),
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
                cels: None,
                keys: Default::default(),
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
        let deepest = crate::document::limits::LAYER_DEPTH as u32 + 1;
        for id in 2..=deepest {
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
                    cels: None,
                    keys: Default::default(),
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
        // The deepest group is at the depth limit, so a layer inside it would
        // be one deeper.
        let changes = move_layer(&document, LayerId(1), Some(LayerId(deepest)), 0).unwrap();
        assert!(matches!(
            commit(&mut document, changes),
            Err(EditError::Document(_))
        ));
        assert_eq!(document, before);
    }

    /// Layers 1, 2, 3 bottom to top.
    fn three_layers() -> Document {
        let mut document = Document::new([64, 64]);
        for index in 1..3 {
            let (_, changes) = add_paint_layer(&document, None, index, format!("L{index}"));
            commit(&mut document, changes).unwrap();
        }
        document
    }

    fn ids(layers: &[Layer]) -> Vec<u32> {
        layers.iter().map(|layer| layer.id.0).collect()
    }

    #[test]
    fn a_group_made_around_a_layer_takes_its_place_and_ungroups_back() {
        let mut document = three_layers();
        let before = document.clone();
        let (group, changes) = wrap_in_group(&document, LayerId(2), "Group 1".to_owned()).unwrap();
        let wrapped = commit(&mut document, changes).unwrap();
        assert_eq!(ids(&document.layers), [1, group.0, 3]);
        assert_eq!(document.position(LayerId(2)), Some((Some(group), 0)));
        undo(&mut document, wrapped);
        assert_eq!(document, before);

        let (group, changes) = wrap_in_group(&document, LayerId(2), "Group 1".to_owned()).unwrap();
        commit(&mut document, changes).unwrap();
        let moved = run(&mut document, |document| {
            move_to_group(document, LayerId(3), Some(group)).unwrap()
        })
        .unwrap();
        assert_eq!(ids(&document.layers), [1, group.0]);
        let ungrouped = run(&mut document, |document| ungroup(document, group).unwrap()).unwrap();
        assert_eq!(ids(&document.layers), [1, 2, 3]);
        assert_eq!(document.validate(), Ok(()));
        undo(&mut document, ungrouped);
        undo(&mut document, moved);
        assert_eq!(ids(&document.layers), [1, group.0, 3]);
        assert_eq!(
            ungroup(&document, LayerId(1)),
            Err(EditError::NotGroup(LayerId(1)))
        );
    }

    #[test]
    fn a_clipped_layer_put_in_a_group_or_folder_hands_its_clipping_to_it() {
        let mut document = three_layers();
        run(&mut document, |document| {
            update_layer(document, LayerId(2), |layer| {
                if let LayerKind::Paint(paint) = &mut layer.kind {
                    paint.clip_to_below = true;
                }
            })
            .unwrap()
        })
        .unwrap();
        let before = document.clone();
        let wrap = |document: &Document, folder: bool| {
            let name = "G".to_owned();
            if folder {
                make_animation_folder(document, LayerId(2), name).unwrap()
            } else {
                wrap_in_group(document, LayerId(2), name).unwrap()
            }
        };
        for folder in [false, true] {
            let (group, changes) = wrap(&document, folder);
            let outcome = commit(&mut document, changes).unwrap();
            // Inside, the layer has no base below it and would not be drawn.
            assert!(document.layer(group).unwrap().clip_to_below());
            assert!(!document.layer(LayerId(2)).unwrap().clip_to_below());
            undo(&mut document, outcome);
            assert_eq!(document, before);
        }
        // Ungrouping hands it back, to every child.
        let (group, changes) = wrap(&document, false);
        commit(&mut document, changes).unwrap();
        run(&mut document, |document| {
            move_to_group(document, LayerId(1), Some(group)).unwrap()
        })
        .unwrap();
        run(&mut document, |document| ungroup(document, group).unwrap()).unwrap();
        assert_eq!(ids(&document.layers), [2, 1, 3]);
        assert!(document.layers[..2].iter().all(Layer::clip_to_below));
    }

    #[test]
    fn layers_move_to_the_top_of_a_group_and_out_above_their_top_ancestor() {
        let mut document = three_layers();
        let (outer, changes) = wrap_in_group(&document, LayerId(2), "Outer".to_owned()).unwrap();
        commit(&mut document, changes).unwrap();
        let (inner, changes) = wrap_in_group(&document, LayerId(2), "Inner".to_owned()).unwrap();
        commit(&mut document, changes).unwrap();
        // 1, Outer [Inner [2]], 3
        run(&mut document, |document| {
            move_to_group(document, LayerId(1), Some(inner)).unwrap()
        })
        .unwrap();
        assert_eq!(document.position(LayerId(1)), Some((Some(inner), 1)));
        run(&mut document, |document| {
            move_to_group(document, LayerId(2), None).unwrap()
        })
        .unwrap();
        assert_eq!(ids(&document.layers), [outer.0, 2, 3]);
        // Already on top of its group: nothing changes.
        let unchanged = run(&mut document, |document| {
            move_to_group(document, LayerId(1), Some(inner)).unwrap()
        });
        assert_eq!(unchanged, Ok(Outcome::NoChange));
        // A group cannot go into itself or what it holds.
        let before = document.clone();
        for target in [outer, inner] {
            let changes = move_to_group(&document, outer, Some(target)).unwrap();
            assert!(commit(&mut document, changes).is_err());
            assert_eq!(document, before);
        }
    }

    #[test]
    fn a_layer_with_a_clipped_layer_on_it_is_not_merged_down() {
        let mut document = three_layers();
        run(&mut document, |document| {
            update_layer(document, LayerId(3), |layer| {
                if let LayerKind::Paint(paint) = &mut layer.kind {
                    paint.clip_to_below = true;
                }
            })
            .unwrap()
        })
        .unwrap();
        assert_eq!(
            merge_down(&document, LayerId(2)),
            Err(EditError::Merge(ops::MergeRefusal::Clipping))
        );
    }

    fn key(frame: u32, x: f64) -> crate::keys::Key {
        crate::keys::Key {
            frame,
            pose: crate::keys::Pose {
                offset: [x, 0.0],
                ..crate::keys::Pose::REST
            },
            ease: crate::keys::Ease::Linear,
        }
    }

    /// Layers 1, 2, 3 with 2 and 3 in a folder showing 2 from frame 0 and 3
    /// from frame 5.
    fn with_folder() -> (Document, LayerId) {
        let mut document = three_layers();
        let (folder, changes) =
            make_animation_folder(&document, LayerId(2), "Walk".to_owned()).unwrap();
        commit(&mut document, changes).unwrap();
        let changes = move_to_group(&document, LayerId(3), Some(folder)).unwrap();
        commit(&mut document, changes).unwrap();
        let changes = update_layer(&document, folder, |layer| {
            if let LayerKind::Group(group) = &mut layer.kind {
                keys::expose(group.cels.as_mut().unwrap(), 5, Some(LayerId(3)));
            }
        })
        .unwrap();
        commit(&mut document, changes).unwrap();
        (document, folder)
    }

    fn cels(document: &Document, folder: LayerId) -> Vec<(u32, Option<u32>)> {
        document
            .layer(folder)
            .unwrap()
            .cels()
            .unwrap()
            .iter()
            .map(|exposure| (exposure.frame, exposure.cel.map(|id| id.0)))
            .collect()
    }

    #[test]
    fn a_new_folder_shows_its_layer_and_new_cels_show_from_their_frame() {
        let (mut document, folder) = with_folder();
        assert_eq!(cels(&document, folder), [(0, Some(2)), (5, Some(3))]);
        let before = document.clone();
        let (cel, changes) = new_cel(&document, folder, 8, "Cel".to_owned()).unwrap();
        let outcome = commit(&mut document, changes).unwrap();
        assert_eq!(document.position(cel), Some((Some(folder), 2)));
        assert_eq!(
            cels(&document, folder),
            [(0, Some(2)), (5, Some(3)), (8, Some(cel.0))]
        );
        undo(&mut document, outcome);
        assert_eq!(document, before);
        assert_eq!(
            new_cel(&document, LayerId(1), 0, String::new()),
            Err(EditError::NotGroup(LayerId(1)))
        );
    }

    #[test]
    fn a_cel_taken_out_of_its_folder_takes_its_exposures_with_it() {
        let (document, folder) = with_folder();
        let mut removed = document.clone();
        let outcome = run(&mut removed, |document| remove_layer(document, LayerId(3))).unwrap();
        assert_eq!(cels(&removed, folder), [(0, Some(2))]);
        undo(&mut removed, outcome);
        assert_eq!(removed, document);

        let mut moved = document.clone();
        run(&mut moved, |document| {
            move_to_group(document, LayerId(3), None).unwrap()
        })
        .unwrap();
        assert_eq!(cels(&moved, folder), [(0, Some(2))]);
        // Moving within the folder keeps them.
        let mut reordered = document.clone();
        run(&mut reordered, |document| {
            move_layer(document, LayerId(3), Some(folder), 0).unwrap()
        })
        .unwrap();
        assert_eq!(cels(&reordered, folder), [(0, Some(2)), (5, Some(3))]);
    }

    #[test]
    fn a_cel_put_in_a_group_is_shown_through_the_group() {
        let (mut document, folder) = with_folder();
        let (group, changes) = wrap_in_group(&document, LayerId(3), "G".to_owned()).unwrap();
        commit(&mut document, changes).unwrap();
        assert_eq!(cels(&document, folder), [(0, Some(2)), (5, Some(group.0))]);
        run(&mut document, |document| ungroup(document, group).unwrap()).unwrap();
        assert_eq!(cels(&document, folder), [(0, Some(2))]);
        assert_eq!(document.validate(), Ok(()));
    }

    #[test]
    fn layers_moving_between_frames_and_cels_are_not_merged_down() {
        let (document, _) = with_folder();
        assert_eq!(
            merge_down(&document, LayerId(3)),
            Err(EditError::Merge(ops::MergeRefusal::Animated))
        );
        let mut keyed = three_layers();
        run(&mut keyed, |document| {
            update_layer(document, LayerId(2), |layer| {
                layer.keys_mut().set(key(0, 3.0));
                layer.keys_mut().set(key(4, 3.0));
            })
            .unwrap()
        })
        .unwrap();
        // Put in one place on every frame, the move is kept in the merge.
        let mut placed = keyed.clone();
        run(&mut placed, |document| {
            merge_down(document, LayerId(2)).unwrap()
        })
        .unwrap();
        let LayerKind::Paint(merged) = &placed.layer(LayerId(1)).unwrap().kind else {
            panic!("a paint layer");
        };
        assert!(merged.keys.is_empty());
        let Some(Op::Isolated(section)) = merged.ops.last() else {
            panic!("a section");
        };
        assert!(matches!(
            section.ops.last(),
            Some(Op::TransformSelection { transform, .. }) if *transform == ops::Affine::translation(3.0, 0.0)
        ));
        assert_eq!(placed.validate(), Ok(()));
        run(&mut keyed, |document| {
            update_layer(document, LayerId(2), |layer| {
                layer.keys_mut().set(key(4, 9.0))
            })
            .unwrap()
        })
        .unwrap();
        for upper in [LayerId(2), LayerId(3)] {
            assert_eq!(
                merge_down(&keyed, upper),
                Err(EditError::Merge(ops::MergeRefusal::Animated))
            );
        }
    }

    #[test]
    fn a_layer_put_in_one_place_is_not_merged_in_a_group_that_keys_move() {
        let mut document = three_layers();
        run(&mut document, |document| {
            update_layer(document, LayerId(2), |layer| {
                layer.keys_mut().set(key(0, 0.5));
            })
            .unwrap()
        })
        .unwrap();
        let (group, changes) = wrap_in_group(&document, LayerId(2), "G".to_owned()).unwrap();
        commit(&mut document, changes).unwrap();
        run(&mut document, |document| {
            move_layer(document, LayerId(1), Some(group), 0).unwrap()
        })
        .unwrap();
        // The group stays: the move is the layer's own, kept in the merge.
        assert!(merge_down(&document, LayerId(2)).is_ok());
        run(&mut document, |document| {
            update_layer(document, group, |layer| {
                layer.keys_mut().set(key(0, 0.5));
            })
            .unwrap()
        })
        .unwrap();
        assert_eq!(
            merge_down(&document, LayerId(2)),
            Err(EditError::Merge(ops::MergeRefusal::Animated))
        );
    }

    #[test]
    fn fewer_frames_drop_the_keys_and_exposures_past_them_in_one_step() {
        let (mut document, folder) = with_folder();
        run(&mut document, |document| {
            update_layer(document, LayerId(1), |layer| {
                layer.keys_mut().set(key(0, 1.0));
                layer.keys_mut().set(key(20, 2.0));
            })
            .unwrap()
        })
        .unwrap();
        let before = document.clone();
        let mut settings = document.settings();
        settings.frames = 5;
        let outcome = run(&mut document, |document| set_settings(document, settings)).unwrap();
        assert_eq!(document.frames, 5);
        assert_eq!(cels(&document, folder), [(0, Some(2))]);
        assert_eq!(document.layer(LayerId(1)).unwrap().keys().keys.len(), 1);
        undo(&mut document, outcome);
        assert_eq!(document, before);
    }

    #[test]
    fn keys_past_the_last_frame_or_showing_another_layer_are_invalid() {
        let mut document = three_layers();
        document.layers[0].keys_mut().set(key(30, 1.0));
        assert_eq!(
            document.validate(),
            Err(crate::document::DocumentError::Animation(LayerId(1)))
        );
        let (mut document, folder) = with_folder();
        let changes = update_layer(&document, folder, |layer| {
            if let LayerKind::Group(group) = &mut layer.kind {
                keys::expose(group.cels.as_mut().unwrap(), 9, Some(LayerId(1)));
            }
        })
        .unwrap();
        assert!(matches!(
            commit(&mut document, changes),
            Err(EditError::Document(_))
        ));
        let mut wild = three_layers();
        let mut far = key(0, 40_000.0);
        far.pose.scale = 1.0;
        wild.layers[0].keys_mut().set(far);
        assert!(wild.validate().is_err());
    }

    #[test]
    fn a_canvas_change_carries_pivots_and_moves_with_what_is_drawn() {
        let mut document = three_layers();
        run(&mut document, |document| {
            update_layer(document, LayerId(1), |layer| {
                layer.keys_mut().pivot = [10.0, 20.0];
                layer.keys_mut().set(key(0, 4.0));
            })
            .unwrap()
        })
        .unwrap();
        let mut cropped = document.clone();
        run(&mut cropped, |document| {
            crop_canvas(document, [3, -2], [64, 64])
        })
        .unwrap();
        let keys = cropped.layer(LayerId(1)).unwrap().keys().clone();
        assert_eq!(keys.pivot, [13.0, 18.0]);
        assert_eq!(keys.keys[0].pose.offset, [4.0, 0.0]);
        run(&mut document, |document| {
            resample_image(document, [128, 32], Sampling::Smooth)
        })
        .unwrap();
        let keys = document.layer(LayerId(1)).unwrap().keys().clone();
        assert_eq!(keys.pivot, [20.0, 10.0]);
        assert_eq!(keys.keys[0].pose.offset, [8.0, 0.0]);
        assert_eq!(document.validate(), Ok(()));
    }
}

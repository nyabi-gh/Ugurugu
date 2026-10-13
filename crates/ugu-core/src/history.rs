// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! Undo, redo and grouped edits over one document, and the selection.
//!
//! An undo entry holds the changes that undo it; large data in them is shared,
//! so a long history copies nothing big. Each document state has an id, so
//! returning to the saved state by undo or redo is clean, not dirty.
//!
//! The selection is not part of the document, but each change to it is an
//! undo step of its own, as in 2.2.13; such a step leaves the document state
//! as it is, so it never makes the document dirty.
//!
//! The oldest steps are dropped once there are `limits::STEPS` of them or
//! what they hold apart from the document passes `limits::BYTES`, so a long
//! session does not keep growing (2.2.13 keeps 64 steps).

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::document::{Document, Layer, LayerId, LayerKind};
use crate::edit::{Change, EditError, Outcome, commit};
use crate::ops::Op;
use crate::selection::Selection;

pub mod limits {
    /// Undo steps kept.
    pub const STEPS: usize = 500;
    /// What undo steps hold apart from the document: earlier selections, and
    /// the strokes, masks, images and layers they would bring back.
    pub const BYTES: usize = 256 * 1024 * 1024;
}

/// Identifies a document state; equal ids mean equal content.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct StateId(u64);

/// Unique in the process, so revisions of different documents never meet.
fn next_revision() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

/// Identifies what each paint layer's own pixels are made from: its
/// operations, wobble and canvas, and the document's wobble and canvas. Its
/// opacity, blend mode, clipping, visibility and place do not count, as they
/// only change how the pixels are put together.
#[derive(Clone, Debug, PartialEq)]
pub struct LayerRevisions {
    settings: u64,
    layers: HashMap<LayerId, u64>,
}

impl Default for LayerRevisions {
    fn default() -> Self {
        Self {
            settings: next_revision(),
            layers: HashMap::new(),
        }
    }
}

impl LayerRevisions {
    /// Equal values mean the layer's pixels are the same.
    pub fn of(&self, id: LayerId) -> u64 {
        self.layers
            .get(&id)
            .copied()
            .unwrap_or(0)
            .max(self.settings)
    }

    fn touch(&mut self, layer: &Layer) {
        let revision = next_revision();
        each(layer, &mut |id| {
            self.layers.insert(id, revision);
        });
    }

    /// Takes note of `undo`, the changes that would undo what was just
    /// applied to `document`.
    fn note(&mut self, document: &Document, undo: &[Change]) {
        for change in undo {
            match change {
                Change::InsertOp { layer, .. } | Change::RemoveOp { layer, .. } => {
                    self.layers.insert(*layer, next_revision());
                }
                Change::InsertLayer { layer, .. } => self.touch(layer),
                Change::RemoveLayer(id) => {
                    if let Some(layer) = document.layer(*id) {
                        self.touch(layer);
                    }
                }
                Change::ReplaceLayer(before) => self.compare(document, before),
                Change::SetSettings(before) => {
                    let after = document.settings();
                    // Motions place their poses within the frames.
                    if before.wobble != after.wobble
                        || before.canvas != after.canvas
                        || before.frames != after.frames
                    {
                        self.settings = next_revision();
                    }
                }
                Change::InsertStroke(..)
                | Change::RemoveStroke(_)
                | Change::InsertMask(..)
                | Change::RemoveMask(_)
                | Change::InsertAsset(..)
                | Change::RemoveAsset(_) => {}
            }
        }
    }

    /// Touches the paint layers in `after`'s place whose pixels differ from
    /// those of `before`.
    fn compare(&mut self, document: &Document, before: &Layer) {
        let Some(after) = document.layer(before.id) else {
            return;
        };
        match (&before.kind, &after.kind) {
            (LayerKind::Paint(old), LayerKind::Paint(new)) => {
                if old.ops != new.ops
                    || old.wobble != new.wobble
                    || old.initial_size != new.initial_size
                {
                    self.layers.insert(after.id, next_revision());
                }
            }
            (LayerKind::Group(old), LayerKind::Group(new)) => {
                for child in &new.children {
                    match old.children.iter().find(|each| each.id == child.id) {
                        Some(previous) => self.compare(document, previous),
                        None => self.touch(child),
                    }
                }
            }
            _ => self.touch(after),
        }
    }
}

fn each(layer: &Layer, visit: &mut impl FnMut(LayerId)) {
    visit(layer.id);
    if let LayerKind::Group(group) = &layer.kind {
        for child in &group.children {
            each(child, visit);
        }
    }
}

struct Entry {
    /// What it holds apart from the document, from `held`.
    bytes: usize,
    label: String,
    /// Applied to go back to `before` (undo) or forward to `after` (redo).
    changes: Vec<Change>,
    /// The selection to restore with them; `None` when the step leaves the
    /// selection alone.
    selection: Option<Option<Arc<Selection>>>,
    before: StateId,
    after: StateId,
}

pub struct History {
    document: Document,
    selection: Option<Arc<Selection>>,
    undo: VecDeque<Entry>,
    redo: Vec<Entry>,
    /// `bytes` of the entries in `undo`.
    undo_bytes: usize,
    state: StateId,
    next_state: u64,
    saved: Option<StateId>,
    revision: u64,
    layers: LayerRevisions,
}

impl History {
    /// Starts from `document` as saved; `saved: false` starts it as changed.
    pub fn new(document: Document, saved: bool) -> Self {
        let state = StateId(0);
        Self {
            document,
            selection: None,
            undo: VecDeque::new(),
            redo: Vec::new(),
            undo_bytes: 0,
            state,
            next_state: 1,
            saved: saved.then_some(state),
            revision: 0,
            layers: LayerRevisions::default(),
        }
    }

    pub fn layer_revisions(&self) -> &LayerRevisions {
        &self.layers
    }

    pub fn document(&self) -> &Document {
        &self.document
    }

    pub fn into_document(self) -> Document {
        self.document
    }

    pub fn selection(&self) -> Option<&Arc<Selection>> {
        self.selection.as_ref()
    }

    /// Makes `selection` the selection as one undo step; returns `false`,
    /// recording nothing, when it is the selection already.
    pub fn select(&mut self, label: &str, selection: Option<Selection>) -> bool {
        if self.selection.as_deref() == selection.as_ref() {
            return false;
        }
        let previous = std::mem::replace(&mut self.selection, selection.map(Arc::new));
        self.redo.clear();
        self.push_undo(Entry {
            bytes: 0,
            label: label.to_owned(),
            changes: Vec::new(),
            selection: Some(previous),
            before: self.state,
            after: self.state,
        });
        true
    }

    pub fn state(&self) -> StateId {
        self.state
    }

    /// Goes up by one on every commit, undo and redo, never down, so a result
    /// computed for an older revision can be recognised and dropped.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn is_dirty(&self) -> bool {
        self.saved != Some(self.state)
    }

    /// Records that the document as it was in `state` is on disk. A save
    /// takes its snapshot first, so edits made during the save stay dirty.
    pub fn mark_saved(&mut self, state: StateId) {
        self.saved = Some(state);
    }

    pub fn undo_label(&self) -> Option<&str> {
        self.undo.back().map(|entry| entry.label.as_str())
    }

    pub fn redo_label(&self) -> Option<&str> {
        self.redo.last().map(|entry| entry.label.as_str())
    }

    /// Commits one edit built from the current document.
    pub fn edit(
        &mut self,
        label: &str,
        build: impl FnOnce(&Document) -> Vec<Change>,
    ) -> Result<Outcome, EditError> {
        self.group(label, |group| group.apply(build))
    }

    /// Commits several edits as one undo step. Each step sees the document
    /// left by the one before; if any fails, all are undone and the error
    /// returned.
    pub fn group(
        &mut self,
        label: &str,
        steps: impl FnOnce(&mut Group<'_>) -> Result<(), EditError>,
    ) -> Result<Outcome, EditError> {
        let mut group = Group {
            document: &mut self.document,
            selection: &mut self.selection,
            previous: None,
            undo: Vec::new(),
        };
        if let Err(error) = steps(&mut group) {
            group.roll_back();
            return Err(error);
        }
        let Group { undo, previous, .. } = group;
        if undo.is_empty() && previous.is_none() {
            return Ok(Outcome::NoChange);
        }
        let changes: Vec<Change> = undo.into_iter().rev().flatten().collect();
        let before = self.state;
        if !changes.is_empty() {
            self.layers.note(&self.document, &changes);
            self.state = StateId(self.next_state);
            self.next_state += 1;
            self.revision += 1;
        }
        self.redo.clear();
        self.push_undo(Entry {
            bytes: 0,
            label: label.to_owned(),
            changes: changes.clone(),
            selection: previous,
            before,
            after: self.state,
        });
        Ok(Outcome::Committed(changes))
    }

    /// Returns `false` when there is nothing to undo.
    pub fn undo(&mut self) -> Result<bool, EditError> {
        let Some(mut entry) = self.undo.pop_back() else {
            return Ok(false);
        };
        self.undo_bytes -= entry.bytes;
        let applied = self.reapply(&mut entry, true);
        match applied {
            Ok(()) => self.redo.push(entry),
            Err(_) => self.push_undo(entry),
        }
        applied.map(|()| true)
    }

    /// Returns `false` when there is nothing to redo.
    pub fn redo(&mut self) -> Result<bool, EditError> {
        let Some(mut entry) = self.redo.pop() else {
            return Ok(false);
        };
        let applied = self.reapply(&mut entry, false);
        match applied {
            Ok(()) => self.push_undo(entry),
            Err(_) => self.redo.push(entry),
        }
        applied.map(|()| true)
    }

    /// Adds the newest undo step, dropping the oldest beyond the limits.
    fn push_undo(&mut self, mut entry: Entry) {
        entry.bytes = held(&entry);
        self.undo_bytes += entry.bytes;
        self.undo.push_back(entry);
        while self.undo.len() > limits::STEPS
            || (self.undo_bytes > limits::BYTES && self.undo.len() > 1)
        {
            let dropped = self.undo.pop_front().expect("more than one");
            self.undo_bytes -= dropped.bytes;
        }
    }

    /// Applies an entry's changes and turns it into the entry that reverses
    /// them; on failure the entry is left as it was.
    fn reapply(&mut self, entry: &mut Entry, backwards: bool) -> Result<(), EditError> {
        let reverse = match commit(&mut self.document, entry.changes.clone())? {
            Outcome::Committed(reverse) => reverse,
            Outcome::NoChange => Vec::new(),
        };
        if !reverse.is_empty() {
            self.layers.note(&self.document, &reverse);
            self.revision += 1;
        }
        self.state = if backwards { entry.before } else { entry.after };
        entry.changes = reverse;
        if let Some(selection) = entry.selection.as_mut() {
            std::mem::swap(selection, &mut self.selection);
        }
        Ok(())
    }
}

/// What an undo step holds apart from the document, in bytes, roughly.
fn held(entry: &Entry) -> usize {
    let selection = match &entry.selection {
        Some(Some(selection)) => selection.bytes(),
        _ => 0,
    };
    selection
        + entry
            .changes
            .iter()
            .map(|change| match change {
                Change::InsertStroke(_, stroke) => stroke.points.len() * 12,
                Change::InsertMask(_, mask) => mask.bits.len(),
                Change::InsertAsset(_, asset) => asset.png.len(),
                Change::InsertLayer { layer, .. } | Change::ReplaceLayer(layer) => {
                    operations(layer) * std::mem::size_of::<Op>()
                }
                _ => 0,
            })
            .sum::<usize>()
}

fn operations(layer: &Layer) -> usize {
    fn count(ops: &[Op]) -> usize {
        ops.iter()
            .map(|op| match op {
                Op::Isolated(section) => 1 + count(&section.ops),
                _ => 1,
            })
            .sum()
    }
    match &layer.kind {
        LayerKind::Paint(paint) => count(&paint.ops),
        LayerKind::Group(group) => group.children.iter().map(operations).sum(),
    }
}

/// Edits being grouped into one undo step.
pub struct Group<'a> {
    document: &'a mut Document,
    selection: &'a mut Option<Arc<Selection>>,
    /// The selection before the group changed it.
    previous: Option<Option<Arc<Selection>>>,
    /// Undo changes of each applied step, in order applied.
    undo: Vec<Vec<Change>>,
}

impl Group<'_> {
    pub fn document(&self) -> &Document {
        self.document
    }

    pub fn selection(&self) -> Option<&Arc<Selection>> {
        self.selection.as_ref()
    }

    /// Changes the selection with the edits, in the same undo step.
    pub fn select(&mut self, selection: Option<Selection>) {
        if self.selection.as_deref() == selection.as_ref() {
            return;
        }
        let previous = std::mem::replace(self.selection, selection.map(Arc::new));
        self.previous.get_or_insert(previous);
    }

    pub fn apply(&mut self, build: impl FnOnce(&Document) -> Vec<Change>) -> Result<(), EditError> {
        let changes = build(self.document);
        if let Outcome::Committed(undo) = commit(self.document, changes)? {
            self.undo.push(undo);
        }
        Ok(())
    }

    fn roll_back(self) {
        if let Some(previous) = self.previous {
            *self.selection = previous;
        }
        for undo in self.undo.into_iter().rev() {
            if let Err(error) = commit(self.document, undo) {
                unreachable!("undoing an applied step failed: {error}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::{add_paint_layer, draw, update_layer};
    use crate::ops::Rgba8;
    use crate::ops::{PaintLayer, Wobble};
    use crate::store::{Brush, BrushEngine, Point, Stroke};

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

    fn draw_at(history: &mut History, x: f32) {
        history
            .edit("Draw stroke", |document| {
                draw(document, LayerId(1), stroke(x), false, None)
            })
            .unwrap();
    }

    #[test]
    fn undo_and_redo_walk_the_states() {
        let mut history = History::new(Document::new([64, 64]), true);
        let empty = history.document().clone();
        draw_at(&mut history, 1.0);
        let one = history.document().clone();
        draw_at(&mut history, 2.0);
        let two = history.document().clone();
        assert!(history.undo().unwrap());
        assert_eq!(history.document(), &one);
        assert!(history.undo().unwrap());
        assert_eq!(history.document(), &empty);
        assert!(!history.undo().unwrap());
        assert!(history.redo().unwrap());
        assert!(history.redo().unwrap());
        assert_eq!(history.document(), &two);
        assert!(!history.redo().unwrap());
    }

    fn undo_count(history: &mut History) -> usize {
        let mut count = 0;
        while history.undo().unwrap() {
            count += 1;
        }
        count
    }

    #[test]
    fn only_the_newest_steps_are_kept() {
        let mut history = History::new(Document::new([64, 64]), true);
        for step in 0..limits::STEPS + 20 {
            draw_at(&mut history, step as f32 % 60.0);
        }
        let all = history.document().clone();
        assert_eq!(undo_count(&mut history), limits::STEPS);
        // The 20 oldest strokes stay, as they can no longer be undone.
        let LayerKind::Paint(paint) = &history.document().layers[0].kind else {
            panic!("a paint layer");
        };
        assert_eq!(paint.ops.len(), 20);
        while history.redo().unwrap() {}
        assert_eq!(history.document(), &all);
    }

    #[test]
    fn steps_holding_large_selections_are_dropped_past_the_byte_limit() {
        let canvas = [4096, 4096];
        let mut history = History::new(Document::new(canvas), true);
        let whole = Selection::all(canvas).unwrap();
        // Each step keeps the selection before it: 2MiB for the whole canvas.
        for step in 0..300 {
            let next = if step % 2 == 0 {
                Some(whole.clone())
            } else {
                None
            };
            assert!(history.select("Select", next));
        }
        assert!(history.undo_bytes <= limits::BYTES);
        let kept = undo_count(&mut history);
        // Every other step holds a whole-canvas selection.
        assert!(
            kept >= 2 * (limits::BYTES / whole.bytes()) - 1 && kept < 300,
            "{kept}"
        );
    }

    #[test]
    fn a_new_edit_clears_redo() {
        let mut history = History::new(Document::new([64, 64]), true);
        draw_at(&mut history, 1.0);
        history.undo().unwrap();
        assert_eq!(history.redo_label(), Some("Draw stroke"));
        draw_at(&mut history, 3.0);
        assert_eq!(history.redo_label(), None);
    }

    #[test]
    fn returning_to_the_saved_state_is_clean() {
        let mut history = History::new(Document::new([64, 64]), false);
        assert!(history.is_dirty());
        draw_at(&mut history, 1.0);
        history.mark_saved(history.state());
        assert!(!history.is_dirty());
        draw_at(&mut history, 2.0);
        assert!(history.is_dirty());
        history.undo().unwrap();
        assert!(!history.is_dirty());
        history.undo().unwrap();
        assert!(history.is_dirty());
        history.redo().unwrap();
        assert!(!history.is_dirty());
    }

    #[test]
    fn a_save_snapshot_keeps_later_edits_dirty() {
        let mut history = History::new(Document::new([64, 64]), true);
        draw_at(&mut history, 1.0);
        let snapshot = history.state();
        // An edit lands while the snapshot is being written.
        draw_at(&mut history, 2.0);
        history.mark_saved(snapshot);
        assert!(history.is_dirty());
    }

    #[test]
    fn the_revision_only_goes_up() {
        let mut history = History::new(Document::new([64, 64]), true);
        draw_at(&mut history, 1.0);
        history.undo().unwrap();
        history.redo().unwrap();
        assert_eq!(history.revision(), 3);
    }

    #[test]
    fn a_failing_group_undoes_its_earlier_steps() {
        let mut history = History::new(Document::new([64, 64]), true);
        let before = history.document().clone();
        let result = history.group("Add and draw", |group| {
            let (id, changes) = add_paint_layer(group.document(), None, 1, "New".to_owned());
            group.apply(|_| changes)?;
            group.apply(|document| draw(document, id, stroke(1.0), false, None))?;
            // Fails: the stroke is not finite.
            group.apply(|document| draw(document, id, stroke(f32::NAN), false, None))
        });
        assert!(result.is_err());
        assert_eq!(history.document(), &before);
        assert_eq!(history.undo_label(), None);
        assert_eq!(history.revision(), 0);
    }

    #[test]
    fn a_group_is_one_undo_step() {
        let mut history = History::new(Document::new([64, 64]), true);
        let before = history.document().clone();
        history
            .group("Add and draw", |group| {
                let (id, changes) = add_paint_layer(group.document(), None, 1, "New".to_owned());
                group.apply(|_| changes)?;
                group.apply(|document| draw(document, id, stroke(1.0), false, None))
            })
            .unwrap();
        history.undo().unwrap();
        assert_eq!(history.document(), &before);
    }

    #[test]
    fn an_edit_that_changes_nothing_leaves_no_undo_step() {
        let mut history = History::new(Document::new([64, 64]), true);
        let outcome = history
            .edit("Rename", |document| {
                update_layer(document, LayerId(1), |_| {}).unwrap()
            })
            .unwrap();
        assert_eq!(outcome, Outcome::NoChange);
        assert_eq!(history.undo_label(), None);
        assert_eq!(history.revision(), 0);
    }

    /// Commit cost on a document at the operation limit with a long history.
    /// Run with `cargo test --release -p ugu-core -- --ignored --nocapture`.
    #[test]
    #[ignore = "timing, not a check"]
    fn commit_cost_at_the_limit() {
        let mut history = History::new(Document::new([2048, 2048]), true);
        for index in 0..crate::document::limits::OPERATIONS - 1 {
            draw_at(&mut history, (index % 2000) as f32);
        }
        let mut times = Vec::new();
        for _ in 0..50 {
            let started = std::time::Instant::now();
            draw_at(&mut history, 5.0);
            times.push(started.elapsed().as_secs_f64() * 1000.0);
            history.undo().unwrap();
        }
        times.sort_by(f64::total_cmp);
        println!(
            "commit at {} operations, {} undo entries: p50 {:.3} ms, p95 {:.3} ms, max {:.3} ms",
            crate::document::limits::OPERATIONS,
            crate::document::limits::OPERATIONS - 1,
            times[25],
            times[47],
            times[49]
        );
    }

    #[test]
    fn layer_revisions_follow_what_each_layers_pixels_are_made_from() {
        let mut history = History::new(Document::new([64, 64]), false);
        history
            .edit("Add layer", |document| {
                add_paint_layer(document, None, 1, "Second".to_owned()).1
            })
            .unwrap();
        let (first, second) = (LayerId(1), LayerId(2));
        let revisions = |history: &History| {
            let layers = history.layer_revisions();
            (layers.of(first), layers.of(second))
        };
        let start = revisions(&history);

        draw_at(&mut history, 4.0);
        let drawn = revisions(&history);
        assert_ne!(drawn.0, start.0);
        assert_eq!(drawn.1, start.1);

        // How the layer is put together does not change its pixels.
        let set = |history: &mut History, update: &dyn Fn(&mut PaintLayer)| {
            history
                .edit("Layer", |document| {
                    update_layer(document, second, |layer| {
                        if let LayerKind::Paint(paint) = &mut layer.kind {
                            update(paint);
                        }
                    })
                    .unwrap()
                })
                .unwrap();
        };
        set(&mut history, &|paint| {
            paint.opacity = 0.5;
            paint.blend = crate::ops::Blend::Multiply;
        });
        assert_eq!(revisions(&history), drawn);
        set(&mut history, &|paint| {
            paint.wobble = Some(Wobble::classic(0.0))
        });
        let still = revisions(&history);
        assert_eq!(still.0, drawn.0);
        assert_ne!(still.1, drawn.1);

        // Undo makes a new revision rather than going back to an old one.
        for _ in 0..3 {
            history.undo().unwrap();
        }
        let undone = revisions(&history);
        assert!(undone.0 != start.0 && undone.0 != drawn.0);
        assert!(undone.1 != still.1 && undone.1 != drawn.1);

        let mut settings = history.document().settings();
        settings.frames_per_second = 24.0;
        history
            .edit("Speed", |_| vec![Change::SetSettings(settings.clone())])
            .unwrap();
        assert_eq!(revisions(&history), undone);
        settings.frames = 12;
        history
            .edit("Frames", |_| vec![Change::SetSettings(settings.clone())])
            .unwrap();
        let framed = revisions(&history);
        assert!(framed.0 > undone.0 && framed.1 > undone.1);
        settings.wobble = Wobble::classic(3.0);
        history
            .edit("Wobble", |_| vec![Change::SetSettings(settings)])
            .unwrap();
        let wobbled = revisions(&history);
        assert!(wobbled.0 > framed.0 && wobbled.1 > framed.1);

        // Another document's layer 1 never shares a revision.
        let other = History::new(Document::new([64, 64]), false);
        assert_ne!(other.layer_revisions().of(first), wobbled.0);
    }

    fn square(at: f64) -> Option<Selection> {
        let shape = crate::selection::Shape::Rectangle([at, at], [at + 4.0, at + 4.0]);
        Selection::of_shape(&shape, [64, 64])
    }

    #[test]
    fn each_selection_change_is_an_undo_step_that_leaves_the_document_clean() {
        let mut history = History::new(Document::new([64, 64]), true);
        assert!(history.select("Select area", square(2.0)));
        assert!(history.select("Add to selection", square(20.0)));
        // The same selection again records nothing.
        assert!(!history.select("Select area", square(20.0)));
        assert!(!history.is_dirty());
        assert_eq!(history.undo_label(), Some("Add to selection"));
        let revision = history.revision();
        history.undo().unwrap();
        assert_eq!(history.selection().map(|s| (**s).clone()), square(2.0));
        history.undo().unwrap();
        assert_eq!(history.selection(), None);
        assert!(!history.undo().unwrap());
        history.redo().unwrap();
        history.redo().unwrap();
        assert_eq!(history.selection().map(|s| (**s).clone()), square(20.0));
        // Nothing to render changed.
        assert_eq!(history.revision(), revision);
        assert!(!history.is_dirty());
    }

    #[test]
    fn a_stroke_step_leaves_the_selection_alone_and_a_new_selection_drops_redo() {
        let mut history = History::new(Document::new([64, 64]), true);
        history.select("Select area", square(2.0));
        draw_at(&mut history, 1.0);
        history.undo().unwrap();
        assert_eq!(history.selection().map(|s| (**s).clone()), square(2.0));
        assert!(!history.is_dirty());
        history.select("Deselect", None);
        assert_eq!(history.redo_label(), None);
    }

    #[test]
    fn a_group_changes_the_selection_with_the_document_in_one_step() {
        let mut history = History::new(Document::new([64, 64]), true);
        history.select("Select area", square(2.0));
        history
            .group("Crop", |group| {
                group.apply(|document| crate::command::crop_canvas(document, [1, 1], [32, 32]))?;
                let moved = group.selection().and_then(|s| s.cropped([1, 1], [32, 32]));
                group.select(moved);
                Ok(())
            })
            .unwrap();
        let moved = history.selection().unwrap().mask().bounds;
        assert_eq!(moved, [3, 3, 4, 4]);
        history.undo().unwrap();
        assert_eq!(history.document().canvas, [64, 64]);
        assert_eq!(history.selection().map(|s| (**s).clone()), square(2.0));
        // A failing group puts the selection back too.
        let failed = history.group("Fail", |group| {
            group.select(None);
            Err(EditError::NoSuchLayer(LayerId(99)))
        });
        assert!(failed.is_err());
        assert_eq!(history.selection().map(|s| (**s).clone()), square(2.0));
    }
}

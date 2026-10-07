// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! Undo, redo and grouped edits over one document.
//!
//! An undo entry holds the changes that undo it; large data in them is shared,
//! so a long history copies nothing big. Each document state has an id, so
//! returning to the saved state by undo or redo is clean, not dirty.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::document::{Document, Layer, LayerId, LayerKind};
use crate::edit::{Change, EditError, Outcome, commit};

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
                    if before.wobble != after.wobble || before.canvas != after.canvas {
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
    label: String,
    /// Applied to go back to `before` (undo) or forward to `after` (redo).
    changes: Vec<Change>,
    before: StateId,
    after: StateId,
}

pub struct History {
    document: Document,
    undo: Vec<Entry>,
    redo: Vec<Entry>,
    state: StateId,
    next_state: u64,
    saved: Option<StateId>,
    revision: u64,
    layers: LayerRevisions,
}

impl History {
    /// Starts from `document` as saved; pass `saved: false` for a new,
    /// never-saved document.
    pub fn new(document: Document, saved: bool) -> Self {
        let state = StateId(0);
        Self {
            document,
            undo: Vec::new(),
            redo: Vec::new(),
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
        self.undo.last().map(|entry| entry.label.as_str())
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
            undo: Vec::new(),
        };
        if let Err(error) = steps(&mut group) {
            group.roll_back();
            return Err(error);
        }
        let mut undo = group.undo;
        if undo.is_empty() {
            return Ok(Outcome::NoChange);
        }
        undo.reverse();
        let changes: Vec<Change> = undo.into_iter().flatten().collect();
        self.layers.note(&self.document, &changes);
        let before = self.state;
        self.state = StateId(self.next_state);
        self.next_state += 1;
        self.revision += 1;
        self.redo.clear();
        self.undo.push(Entry {
            label: label.to_owned(),
            changes: changes.clone(),
            before,
            after: self.state,
        });
        Ok(Outcome::Committed(changes))
    }

    /// Returns `false` when there is nothing to undo.
    pub fn undo(&mut self) -> Result<bool, EditError> {
        let Some(entry) = self.undo.pop() else {
            return Ok(false);
        };
        match self.reapply(entry, true) {
            Ok(redo) => {
                self.redo.push(redo);
                Ok(true)
            }
            Err((entry, error)) => {
                self.undo.push(entry);
                Err(error)
            }
        }
    }

    /// Returns `false` when there is nothing to redo.
    pub fn redo(&mut self) -> Result<bool, EditError> {
        let Some(entry) = self.redo.pop() else {
            return Ok(false);
        };
        match self.reapply(entry, false) {
            Ok(undo) => {
                self.undo.push(undo);
                Ok(true)
            }
            Err((entry, error)) => {
                self.redo.push(entry);
                Err(error)
            }
        }
    }

    /// Applies an entry's changes and returns the entry that reverses it.
    fn reapply(&mut self, entry: Entry, backwards: bool) -> Result<Entry, (Entry, EditError)> {
        let Entry {
            label,
            changes,
            before,
            after,
        } = entry;
        match commit(&mut self.document, changes.clone()) {
            Ok(outcome) => {
                let reverse = match outcome {
                    Outcome::Committed(reverse) => reverse,
                    Outcome::NoChange => Vec::new(),
                };
                self.layers.note(&self.document, &reverse);
                self.state = if backwards { before } else { after };
                self.revision += 1;
                Ok(Entry {
                    label,
                    changes: reverse,
                    before,
                    after,
                })
            }
            Err(error) => Err((
                Entry {
                    label,
                    changes,
                    before,
                    after,
                },
                error,
            )),
        }
    }
}

/// Edits being grouped into one undo step.
pub struct Group<'a> {
    document: &'a mut Document,
    /// Undo changes of each applied step, in order applied.
    undo: Vec<Vec<Change>>,
}

impl Group<'_> {
    pub fn document(&self) -> &Document {
        self.document
    }

    pub fn apply(&mut self, build: impl FnOnce(&Document) -> Vec<Change>) -> Result<(), EditError> {
        let changes = build(self.document);
        if let Outcome::Committed(undo) = commit(self.document, changes)? {
            self.undo.push(undo);
        }
        Ok(())
    }

    fn roll_back(self) {
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
        settings.frames = 12;
        history
            .edit("Frames", |_| vec![Change::SetSettings(settings.clone())])
            .unwrap();
        assert_eq!(revisions(&history), undone);
        settings.wobble = Wobble::classic(3.0);
        history
            .edit("Wobble", |_| vec![Change::SetSettings(settings)])
            .unwrap();
        let wobbled = revisions(&history);
        assert!(wobbled.0 > undone.0 && wobbled.1 > undone.1);

        // Another document's layer 1 never shares a revision.
        let other = History::new(Document::new([64, 64]), false);
        assert_ne!(other.layer_revisions().of(first), wobbled.0);
    }
}

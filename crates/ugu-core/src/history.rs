// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! Undo, redo and grouped edits over one document.
//!
//! An undo entry holds the changes that undo it; large data in them is shared,
//! so a long history copies nothing big. Each document state has an id, so
//! returning to the saved state by undo or redo is clean, not dirty.

use crate::document::Document;
use crate::edit::{Change, EditError, Outcome, commit};

/// Identifies a document state; equal ids mean equal content.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct StateId(u64);

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
        }
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
    use crate::document::LayerId;
    use crate::ops::Rgba8;
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
}

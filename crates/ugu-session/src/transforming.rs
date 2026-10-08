// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! The pending edit: moving, scaling, turning and flipping the selected part
//! of the current layer as one transform that is shown before it is applied,
//! so a run of changes resamples once. Applying is one undo step that moves
//! the selection along; undo, redo or Esc cancel it, and any other edit
//! applies it first, so no transform is dropped unseen.

use std::sync::Arc;

use ugu_core::command;
use ugu_core::document::LayerId;
use ugu_core::edit::{EditError, Outcome};
use ugu_core::ops::Affine;
use ugu_core::selection::Selection;
use ugu_core::store::limits;

use crate::{FillError, Session, selecting};

/// A transform of the selected part of a layer, not yet in the document.
#[derive(Clone, Debug, PartialEq)]
pub struct Pending {
    pub layer: LayerId,
    /// The selection when the edit began; the transform moves it.
    pub selection: Arc<Selection>,
    pub transform: Affine,
    /// Duplicates: the selected part stays where it was too.
    pub keep_source: bool,
}

impl Session {
    pub fn pending(&self) -> Option<&Pending> {
        self.pending.as_ref()
    }

    /// Starts transforming the selected part of the current layer; `false`
    /// when a transform is already pending.
    pub fn begin_transform(&mut self) -> Result<bool, FillError> {
        if self.pending.is_some() {
            return Ok(false);
        }
        let selection = self.selection().cloned().ok_or(FillError::NoSelection)?;
        self.paintable()?;
        self.live = None;
        self.lasso = None;
        self.pending = Some(Pending {
            layer: self.layer,
            selection,
            transform: Affine::IDENTITY,
            keep_source: false,
        });
        Ok(true)
    }

    /// Replaces the pending transform; `false` when there is none, or the
    /// transform flattens the selection or takes it beyond the coordinates a
    /// document holds.
    pub fn set_transform(&mut self, transform: Affine) -> bool {
        let Some(pending) = self.pending.as_mut() else {
            return false;
        };
        let [left, top, width, height] = pending.selection.mask().bounds.map(f64::from);
        let within = [
            [left, top],
            [left + width, top],
            [left + width, top + height],
            [left, top + height],
        ]
        .iter()
        .flat_map(|&corner| transform.apply(corner))
        .all(|value| value.abs() <= f64::from(limits::COORDINATE));
        if transform.inverse().is_none() || !within {
            return false;
        }
        pending.transform = transform;
        true
    }

    /// Whether the selected part also stays where it was.
    pub fn set_keep_source(&mut self, keep: bool) {
        if let Some(pending) = self.pending.as_mut() {
            pending.keep_source = keep;
        }
    }

    /// Puts the pending transform in the document; the selection follows
    /// it. An unmoved selection changes nothing.
    pub fn apply_transform(&mut self) -> Result<Outcome, EditError> {
        let Some(pending) = self.pending.take() else {
            return Ok(Outcome::NoChange);
        };
        if pending.transform == Affine::IDENTITY {
            return Ok(Outcome::NoChange);
        }
        let sampling = self.transform_sampling;
        let moved = pending.selection.transformed(pending.transform);
        self.history.group("Transform selection", |group| {
            let mask = selecting::stored_mask(group, &pending.selection)?;
            group.apply(|document| {
                command::transform_selection(
                    document,
                    pending.layer,
                    mask,
                    pending.transform,
                    sampling,
                    pending.keep_source,
                )
            })?;
            group.select(moved);
            Ok(())
        })
    }

    /// Drops the pending transform; returns whether there was one.
    pub fn cancel_transform(&mut self) -> bool {
        self.pending.take().is_some()
    }

    /// Clears the selected part of the current layer, after the pending
    /// transform if there is one.
    pub fn delete_selected(&mut self) -> Result<Outcome, FillError> {
        self.settle();
        let selection = self.selection().cloned().ok_or(FillError::NoSelection)?;
        self.paintable()?;
        let layer = self.layer;
        Ok(self.history.group("Delete", |group| {
            let mask = selecting::stored_mask(group, &selection)?;
            group.apply(|document| command::clear_selection(document, layer, mask))
        })?)
    }

    /// Applies the pending transform before another edit. One that cannot
    /// be applied is dropped; it was checked as it was set.
    pub(crate) fn settle(&mut self) {
        let _ = self.apply_transform();
    }
}

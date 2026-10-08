// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! Copy and paste within the app, and placing an image from a file or
//! another app. Copying only reads the document; pasting and placing add a
//! layer above the current one in one undo step and leave what was added
//! selected in a pending transform, so it can be moved at once.

use std::sync::Arc;

use ugu_core::clip::{self, Clip};
use ugu_core::command;
use ugu_core::document::{Document, LayerId};
use ugu_core::edit::{Change, EditError, Outcome};
use ugu_core::ops::AssetId;
use ugu_core::selection::Selection;
use ugu_core::store::Asset;

use crate::{FillError, Session};

impl Session {
    /// Copies the selected part of the current layer, as shown: a pending
    /// transform is applied first.
    pub fn copy(&mut self) -> Result<Arc<Clip>, FillError> {
        self.settle();
        let selection = self.selection().cloned().ok_or(FillError::NoSelection)?;
        clip::copy(self.document(), self.layer, selection)
            .map(Arc::new)
            .ok_or(FillError::NoLayer)
    }

    /// Adds `clip` as a new layer above the current one where it was copied
    /// from, selected and ready to move.
    pub fn paste(&mut self, clip: &Clip) -> Result<Outcome, EditError> {
        self.add_selected("Paste", |document, parent, index, name| {
            clip::paste(document, clip, parent, index, name)
        })
    }

    /// Adds `asset` as a new layer above the current one, in the middle of
    /// the canvas, selected and ready to move.
    pub fn place_image(&mut self, id: AssetId, asset: Asset) -> Result<Outcome, EditError> {
        self.add_selected("Place image", |document, parent, index, name| {
            clip::place_image(document, id, asset, parent, index, name)
        })
    }

    fn add_selected(
        &mut self,
        label: &str,
        make: impl FnOnce(
            &Document,
            Option<LayerId>,
            usize,
            String,
        ) -> (LayerId, Vec<Change>, Option<Selection>),
    ) -> Result<Outcome, EditError> {
        self.settle();
        self.live = None;
        self.lasso = None;
        let (parent, index) = self
            .document()
            .position(self.layer)
            .map_or((None, self.document().layers.len()), |(parent, index)| {
                (parent, index + 1)
            });
        let name = format!("Layer {}", command::next_layer_id(self.document()).0);
        let mut added = None;
        let outcome = self.history.group(label, |group| {
            let mut covered = None;
            group.apply(|document| {
                let (id, changes, selection) = make(document, parent, index, name);
                added = Some(id);
                covered = selection;
                changes
            })?;
            group.select(covered);
            Ok(())
        })?;
        if let (Outcome::Committed(_), Some(id)) = (&outcome, added) {
            self.layer = id;
            // Nothing to move when it lands outside the canvas.
            let _ = self.begin_transform();
        }
        Ok(outcome)
    }
}

// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! Changing the colour and width of what the selection touches on the
//! current layer.

use ugu_core::edit::Outcome;
use ugu_core::restyle::{self, Style, Touched};

use crate::{FillError, Session};

impl Session {
    /// What the selection touches on the current layer, for
    /// `restyle_selected`.
    pub fn touched(&self) -> Result<Touched, FillError> {
        self.restylable()?;
        let selection = self.selection().ok_or(FillError::NoSelection)?;
        let selection = self
            .in_layer(selection)
            .ok_or(FillError::OutsideSelection)?;
        Ok(restyle::touched(self.document(), self.layer, &selection))
    }

    /// Gives what the selection touches on the current layer `style`, as
    /// one undo step.
    pub fn restyle_selected(&mut self, style: Style) -> Result<Outcome, FillError> {
        self.restylable()?;
        let selection = self.selection().cloned().ok_or(FillError::NoSelection)?;
        let selection = self
            .in_layer(&selection)
            .ok_or(FillError::OutsideSelection)?;
        self.live = None;
        self.lasso = None;
        let layer = self.layer;
        Ok(self.history.group("Edit stroke properties", |group| {
            let changes = restyle::restyle(group.document(), layer, &selection, style)?;
            group.apply(|_| changes)
        })?)
    }

    /// A pending transform or placed text keeps the strokes where the
    /// selection does not show them, so they cannot be told apart until it
    /// ends.
    fn restylable(&self) -> Result<(), FillError> {
        if self.pending.is_some() || self.placed.is_some() {
            return Err(FillError::Pending);
        }
        self.paintable()
    }
}

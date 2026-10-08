// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! Selecting and filling by what is drawn, as in 2.2.13: the wand selects
//! the area around a click, the bucket fills it, and the selection or a
//! dragged shape can be filled. The area is found in a reference image of
//! the frame shown, which the caller draws as `Session::reads` says: the
//! session holds no renderer. A fill keeps the pixels it covered when it
//! was made, so it does not move with the strokes around it.

use std::sync::Arc;

use ugu_core::command;
use ugu_core::edit::{EditError, Outcome};
use ugu_core::selection::{Combine, Compare, Selection};

use crate::{Session, selecting};

/// Which layers the wand and the bucket read.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Reads {
    /// The current layer alone.
    #[default]
    Current,
    /// The layers marked as references.
    Marked,
    /// Every shown layer.
    Visible,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FillSettings {
    pub reads: Reads,
    /// Compare colours with `tolerance` instead of stopping at lines.
    pub by_colour: bool,
    pub tolerance: u8,
    /// Fills soften their edges.
    pub antialias: bool,
}

impl FillSettings {
    /// 2.2.13's: the current layer, stopping at lines, tolerance 32 kept for
    /// colour, antialiased.
    pub const DEFAULT: Self = Self {
        reads: Reads::Current,
        by_colour: false,
        tolerance: 32,
        antialias: true,
    };

    pub fn compare(&self) -> Compare {
        if self.by_colour {
            Compare::Color(self.tolerance)
        } else {
            Compare::AlphaBoundary
        }
    }
}

/// Why selecting or filling did nothing.
#[derive(Clone, Debug, PartialEq)]
pub enum FillError {
    /// The current layer is not a paint layer.
    NoLayer,
    HiddenLayer,
    /// Nothing to read: no layer marked as a reference is shown.
    NoReference,
    /// The click is on what stops the area, such as a line.
    NothingThere,
    /// The bucket was clicked outside the selection.
    OutsideSelection,
    NoSelection,
    Edit(EditError),
}

impl From<EditError> for FillError {
    fn from(error: EditError) -> Self {
        Self::Edit(error)
    }
}

impl Session {
    /// The pixel `position` is in, if it is on the canvas.
    fn pixel(&self, position: [f64; 2]) -> Option<[u32; 2]> {
        let canvas = self.document().canvas;
        let [x, y] = position.map(f64::floor);
        (x >= 0.0 && y >= 0.0 && x < f64::from(canvas[0]) && y < f64::from(canvas[1]))
            .then_some([x as u32, y as u32])
    }

    /// The area around `seed` in `reference`, the frame shown as
    /// `self.fill.reads` says (premultiplied rows of the canvas); `None`
    /// when there was nothing to read.
    fn area(&self, seed: [u32; 2], reference: Option<&[[u8; 4]]>) -> Result<Selection, FillError> {
        let reference = reference.ok_or(FillError::NoReference)?;
        Selection::flood(reference, self.document().canvas, seed, self.fill.compare())
            .ok_or(FillError::NothingThere)
    }

    /// Selects the area clicked at `position`, combined with the selection
    /// as `combine` says; returns whether the selection changed. When the
    /// area cannot be found, replacing clears the selection as a click with
    /// the selection tool does, and the reason is returned.
    pub fn wand(
        &mut self,
        position: [f64; 2],
        combine: Combine,
        reference: Option<&[[u8; 4]]>,
    ) -> Result<bool, FillError> {
        self.live = None;
        self.lasso = None;
        let Some(seed) = self.pixel(position) else {
            // Off the canvas, as a click with the selection tool.
            return Ok(combine == Combine::Replace && self.history_mut().select("Deselect", None));
        };
        let area = match self.area(seed, reference) {
            Ok(area) => area,
            Err(error) => {
                if combine == Combine::Replace {
                    self.history_mut().select("Deselect", None);
                }
                return Err(error);
            }
        };
        let label = match combine {
            Combine::Replace => "Select area",
            Combine::Add => "Add to selection",
            Combine::Subtract => "Subtract from selection",
        };
        let next = Selection::combine(self.selection().map(Arc::as_ref), Some(area), combine);
        Ok(self.history_mut().select(label, next))
    }

    /// Fills the area clicked at `position` on the current layer with the
    /// brush colour, cut to the selection; with a selection, only a click
    /// inside it fills.
    pub fn bucket(
        &mut self,
        position: [f64; 2],
        reference: Option<&[[u8; 4]]>,
    ) -> Result<Outcome, FillError> {
        self.live = None;
        let Some(seed) = self.pixel(position) else {
            return Ok(Outcome::NoChange);
        };
        self.paintable()?;
        if self
            .selection()
            .is_some_and(|selection| !selection.mask().contains(seed[0] as i32, seed[1] as i32))
        {
            return Err(FillError::OutsideSelection);
        }
        let area = self.area(seed, reference)?;
        self.commit_fill(&area, self.clip())
    }

    /// Fills the selection on the current layer with the brush colour.
    pub fn fill_selection(&mut self) -> Result<Outcome, FillError> {
        let selection = self.selection().cloned().ok_or(FillError::NoSelection)?;
        self.paintable()?;
        // Cut to the selection too, so the softened edge stays inside the
        // marching ants.
        let clip = Some(selection.clone()).filter(|selection| !selection.covers_canvas());
        self.commit_fill(&selection, clip)
    }

    /// Fills `shape`'s pixels cut to the selection: the selection tool in
    /// paint mode. A shape too small to cover a pixel does nothing.
    pub(crate) fn fill_shape(&mut self, shape: Option<Selection>) -> Result<Outcome, FillError> {
        self.paintable()?;
        let Some(shape) = shape else {
            return Ok(Outcome::NoChange);
        };
        self.commit_fill(&shape, self.clip())
    }

    pub(crate) fn paintable(&self) -> Result<(), FillError> {
        self.can_paint().map_err(|refused| match refused {
            crate::StrokeRefused::NoLayer => FillError::NoLayer,
            crate::StrokeRefused::HiddenLayer => FillError::HiddenLayer,
        })
    }

    fn commit_fill(
        &mut self,
        coverage: &Selection,
        clip: Option<Arc<Selection>>,
    ) -> Result<Outcome, FillError> {
        let (layer, color, antialias) = (self.layer, self.pen.color, self.fill.antialias);
        Ok(self.history_mut().group("Fill", |group| {
            let coverage = selecting::stored_mask(group, coverage)?;
            let clip = match &clip {
                Some(selection) => Some(selecting::stored_mask(group, selection)?),
                None => None,
            };
            group.apply(|document| command::fill(document, layer, coverage, color, antialias, clip))
        })?)
    }
}

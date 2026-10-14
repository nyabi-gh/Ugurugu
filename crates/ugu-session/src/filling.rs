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

use crate::{Session, Tools, selecting};

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
    /// A transform or placed text has to be applied or cancelled first.
    Pending,
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
    /// `fill.reads` says (premultiplied rows of the canvas); `None` when
    /// there was nothing to read.
    fn area(
        &self,
        seed: [u32; 2],
        reference: Option<&[[u8; 4]]>,
        fill: &FillSettings,
    ) -> Result<Selection, FillError> {
        let reference = reference.ok_or(FillError::NoReference)?;
        Selection::flood(reference, self.document().canvas, seed, fill.compare())
            .ok_or(FillError::NothingThere)
    }

    /// Selects the area clicked at `position`, combined with the selection
    /// as `combine` says; returns whether the selection changed. When the
    /// area cannot be found, replacing clears the selection as a click with
    /// the selection tool does, and the reason is returned.
    pub fn wand(
        &mut self,
        tools: &Tools,
        position: [f64; 2],
        combine: Combine,
        reference: Option<&[[u8; 4]]>,
    ) -> Result<bool, FillError> {
        self.live = None;
        self.lasso = None;
        // The current layer alone is read in its own pixels.
        let own = tools.fill.reads == Reads::Current;
        let Some(seed) = self.pixel(if own {
            self.to_layer(position)
        } else {
            position
        }) else {
            // Off the canvas, as a click with the selection tool.
            return Ok(combine == Combine::Replace && self.history_mut()?.select("Deselect", None));
        };
        let area = self
            .area(seed, reference, &tools.fill)
            .and_then(|area| match own {
                true => self.shown_from_layer(area).ok_or(FillError::NothingThere),
                false => Ok(area),
            });
        let area = match area {
            Ok(area) => area,
            Err(error) => {
                if combine == Combine::Replace {
                    self.history_mut()?.select("Deselect", None);
                }
                return Err(error);
            }
        };
        let label = match combine {
            Combine::Replace => "Select area",
            Combine::Add => "Add to selection",
            Combine::Subtract => "Subtract from selection",
        };
        self.settle()?;
        let next = Selection::combine(self.selection().map(Arc::as_ref), Some(area), combine);
        Ok(self.history_mut()?.select(label, next))
    }

    /// Fills the area clicked at `position` on the current layer with the
    /// brush colour, cut to the selection; with a selection, only a click
    /// inside it fills.
    pub fn bucket(
        &mut self,
        tools: &Tools,
        position: [f64; 2],
        reference: Option<&[[u8; 4]]>,
    ) -> Result<Outcome, FillError> {
        self.live = None;
        let Some(clicked) = self.pixel(position) else {
            return Ok(Outcome::NoChange);
        };
        self.settle()?;
        self.draw_on_cel()?;
        self.paintable()?;
        if self.selection().is_some_and(|selection| {
            !selection
                .mask()
                .contains(clicked[0] as i32, clicked[1] as i32)
        }) {
            return Err(FillError::OutsideSelection);
        }
        // The current layer alone is read in its own pixels, anything else
        // as shown.
        let area = if tools.fill.reads == Reads::Current {
            let seed = self
                .pixel(self.to_layer(position))
                .ok_or(FillError::NothingThere)?;
            Arc::new(self.area(seed, reference, &tools.fill)?)
        } else {
            let area = Arc::new(self.area(clicked, reference, &tools.fill)?);
            self.in_layer(&area).ok_or(FillError::NothingThere)?
        };
        let clip = self.layer_clip()?;
        self.commit_fill(tools, &area, clip)
    }

    /// The selection, if it cuts anything away, in the current layer's own
    /// pixels on this frame.
    pub(crate) fn layer_clip(&self) -> Result<Option<Arc<Selection>>, FillError> {
        match self.clip() {
            Some(selection) => self
                .in_layer(&selection)
                .map(Some)
                .ok_or(FillError::OutsideSelection),
            None => Ok(None),
        }
    }

    /// Fills the selection on the current layer with the brush colour.
    pub fn fill_selection(&mut self, tools: &Tools) -> Result<Outcome, FillError> {
        self.settle()?;
        let selection = self.selection().cloned().ok_or(FillError::NoSelection)?;
        self.draw_on_cel()?;
        self.paintable()?;
        // Cut to the selection too, so the softened edge stays inside the
        // marching ants.
        let clip = self.layer_clip()?;
        let coverage = self
            .in_layer(&selection)
            .ok_or(FillError::OutsideSelection)?;
        self.commit_fill(tools, &coverage, clip)
    }

    /// Fills `shape`'s pixels cut to the selection: the selection tool in
    /// paint mode. A shape too small to cover a pixel does nothing.
    pub(crate) fn fill_shape(
        &mut self,
        tools: &Tools,
        shape: Option<Selection>,
    ) -> Result<Outcome, FillError> {
        let Some(shape) = shape else {
            self.paintable()?;
            return Ok(Outcome::NoChange);
        };
        self.settle()?;
        self.draw_on_cel()?;
        self.paintable()?;
        let Some(shape) = self.in_layer(&Arc::new(shape)) else {
            return Ok(Outcome::NoChange);
        };
        let clip = self.layer_clip()?;
        self.commit_fill(tools, &shape, clip)
    }

    pub(crate) fn paintable(&self) -> Result<(), FillError> {
        self.can_paint().map_err(|refused| match refused {
            crate::StrokeRefused::NoLayer => FillError::NoLayer,
            crate::StrokeRefused::HiddenLayer => FillError::HiddenLayer,
            crate::StrokeRefused::Pending => FillError::Pending,
            crate::StrokeRefused::OutsideSelection => FillError::OutsideSelection,
        })
    }

    fn commit_fill(
        &mut self,
        tools: &Tools,
        coverage: &Selection,
        clip: Option<Arc<Selection>>,
    ) -> Result<Outcome, FillError> {
        let antialias = tools.fill.antialias && !self.pixel_mode();
        let (layer, color) = (self.layer, tools.color);
        let outcome = self.history_mut()?.group("Fill", |group| {
            let coverage = selecting::stored_mask(group, coverage)?;
            let clip = match &clip {
                Some(selection) => Some(selecting::stored_mask(group, selection)?),
                None => None,
            };
            group.apply(|document| command::fill(document, layer, coverage, color, antialias, clip))
        })?;
        if let Outcome::Committed(_) = outcome {
            self.used.push(color);
        }
        Ok(outcome)
    }
}

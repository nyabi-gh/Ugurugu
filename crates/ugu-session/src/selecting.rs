// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! Selecting, as in 2.2.13: a shape dragged with the selection tool
//! replaces the selection, adds to it with Shift or takes from it with Alt;
//! select all, invert and deselect. Each change is one undo step. The
//! selection stays when the current layer or frame changes; strokes drawn
//! while it is there are cut to it.

use std::sync::Arc;

use ugu_core::edit::{Change, EditError, Outcome};
use ugu_core::history::Group;
use ugu_core::ops::MaskId;
use ugu_core::selection::{Combine, Selection, Shape};

use crate::{FillError, Session};

/// A freehand point is added once the pointer is this far from the last.
const FREEHAND_STEP: f64 = 1.0;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ShapeKind {
    #[default]
    Freehand,
    Rectangle,
    Ellipse,
}

/// A shape being dragged, in document pixels on the canvas.
#[derive(Clone, Debug, PartialEq)]
pub struct Lasso {
    pub kind: ShapeKind,
    pub combine: Combine,
    /// Every freehand point; for a rectangle or ellipse, where the drag
    /// started and where it is.
    pub points: Vec<[f64; 2]>,
}

impl Lasso {
    pub fn shape(&self) -> Shape {
        let ends = || {
            let first = self.points[0];
            (first, *self.points.last().unwrap_or(&first))
        };
        match self.kind {
            ShapeKind::Freehand => Shape::Freehand(self.points.clone()),
            ShapeKind::Rectangle => {
                let (a, b) = ends();
                Shape::Rectangle(a, b)
            }
            ShapeKind::Ellipse => {
                let (a, b) = ends();
                Shape::Ellipse(a, b)
            }
        }
    }
}

impl Session {
    pub fn selection(&self) -> Option<&Arc<Selection>> {
        self.history.selection()
    }

    pub fn lasso(&self) -> Option<&Lasso> {
        self.lasso.as_ref()
    }

    fn on_canvas(&self, position: [f64; 2]) -> [f64; 2] {
        let canvas = self.document().canvas;
        [0, 1].map(|axis| position[axis].clamp(0.0, f64::from(canvas[axis])))
    }

    pub fn begin_selection(&mut self, position: [f64; 2], combine: Combine) {
        self.live = None;
        self.lasso = Some(Lasso {
            kind: self.selection_shape,
            combine,
            points: vec![self.on_canvas(position)],
        });
    }

    /// Returns whether the shape changed.
    pub fn extend_selection(&mut self, position: [f64; 2]) -> bool {
        let position = self.on_canvas(position);
        let Some(lasso) = self.lasso.as_mut() else {
            return false;
        };
        let last = *lasso.points.last().expect("a lasso starts with a point");
        if lasso.kind == ShapeKind::Freehand {
            if (position[0] - last[0]).hypot(position[1] - last[1]) < FREEHAND_STEP {
                return false;
            }
        } else {
            if position == last {
                return false;
            }
            lasso.points.truncate(1);
        }
        lasso.points.push(position);
        true
    }

    /// Makes the dragged shape part of the selection, or in paint mode
    /// fills it; returns whether anything changed. A shape too small to
    /// select, as a click, clears the selection when replacing and does
    /// nothing otherwise.
    pub fn end_selection(&mut self, position: [f64; 2]) -> Result<bool, FillError> {
        self.extend_selection(position);
        let Some(lasso) = self.lasso.take() else {
            return Ok(false);
        };
        let shape = Selection::of_shape(&lasso.shape(), self.document().canvas);
        if self.lasso_paints {
            return Ok(matches!(self.fill_shape(shape)?, Outcome::Committed(_)));
        }
        let label = match (lasso.combine, &shape) {
            (Combine::Replace, None) => "Deselect",
            (_, None) => return Ok(false),
            (Combine::Replace, Some(_)) => "Select area",
            (Combine::Add, Some(_)) => "Add to selection",
            (Combine::Subtract, Some(_)) => "Subtract from selection",
        };
        let next = Selection::combine(self.selection().map(Arc::as_ref), shape, lasso.combine);
        Ok(self.history_mut().select(label, next))
    }

    /// Drops the shape being dragged, leaving the selection as it was.
    pub fn cancel_selection(&mut self) -> bool {
        self.lasso.take().is_some()
    }

    pub fn select_all(&mut self) -> bool {
        self.lasso = None;
        let all = Selection::all(self.document().canvas);
        self.history_mut().select("Select all", all)
    }

    /// Selects what is not selected; nothing without a selection.
    pub fn invert_selection(&mut self) -> bool {
        self.lasso = None;
        let Some(inverted) = self.selection().map(|selection| selection.invert()) else {
            return false;
        };
        self.history_mut().select("Invert selection", inverted)
    }

    pub fn deselect(&mut self) -> bool {
        self.lasso = None;
        self.history_mut().select("Deselect", None)
    }

    /// Esc: drops a shape being dragged, else a pending transform or placed
    /// text, else the selection.
    pub fn escape(&mut self) -> bool {
        self.cancel_selection() || self.cancel_transform() || self.cancel_text() || self.deselect()
    }

    /// The selection a stroke begun now is cut to; `None` when it cuts
    /// nothing away.
    pub(crate) fn clip(&self) -> Option<Arc<Selection>> {
        self.selection()
            .filter(|selection| !selection.covers_canvas())
            .cloned()
    }
}

/// The stored mask holding `selection`'s pixels, added when none does.
pub(crate) fn stored_mask(
    group: &mut Group<'_>,
    selection: &Selection,
) -> Result<MaskId, EditError> {
    let mask = selection.mask();
    let masks = &group.document().store.masks;
    if let Some((&id, _)) = masks
        .iter()
        .find(|(_, stored)| Arc::ptr_eq(&stored.bits, &mask.bits) && stored.bounds == mask.bounds)
    {
        return Ok(id);
    }
    let id = MaskId(masks.keys().map(|id| id.0 + 1).max().unwrap_or(0));
    group.apply(|_| vec![Change::InsertMask(id, mask.clone())])?;
    Ok(id)
}

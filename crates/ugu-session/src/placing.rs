// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! Text placed on the canvas and not yet in the document. The caller lays
//! the text out with its fonts and hands the outline over; the session
//! keeps where it goes and turns it into strokes and a fill when applied,
//! in the pen's colour, width and brush at that moment. Like a pending
//! transform, undo, redo or Esc drop it and any other edit applies it
//! first.

use std::hash::BuildHasher;
use std::sync::Arc;

use ugu_core::command;
use ugu_core::document::LayerId;
use ugu_core::edit::{EditError, Outcome};
use ugu_core::selection::Selection;
use ugu_core::store::{Brush, Stroke, limits};
use ugu_core::text::{self, Outline};

use crate::transforming::Ended;
use crate::{FillError, Session, selecting};

/// The text tool's settings.
#[derive(Clone, Debug, PartialEq)]
pub struct TextSettings {
    /// `None` takes the default family.
    pub family: Option<String>,
    /// Pixels per em.
    pub size: f32,
    /// Fill the letters under their outlines.
    pub filled: bool,
}

impl TextSettings {
    /// 2.2.13's.
    pub const SIZE: std::ops::RangeInclusive<f32> = 8.0..=512.0;

    pub const DEFAULT: Self = Self {
        family: None,
        size: 48.0,
        filled: false,
    };
}

/// Text placed but not applied.
#[derive(Clone, Debug, PartialEq)]
pub struct Placed {
    pub layer: LayerId,
    /// Where the text's top left goes.
    pub at: [f64; 2],
    pub outline: Arc<Outline>,
    seed: u64,
}

/// What placed text draws, in order: the fill, then the strokes, all cut to
/// `clip`.
#[derive(Clone, Debug, PartialEq)]
pub struct TextDrawing {
    pub layer: LayerId,
    pub fill: Option<Selection>,
    pub strokes: Vec<Stroke>,
    pub clip: Option<Arc<Selection>>,
    /// Whether the fill softens its edge.
    pub antialias: bool,
}

impl Session {
    pub fn placed_text(&self) -> Option<&Placed> {
        self.placed.as_ref()
    }

    /// Puts the text's top left at `at`, placing it on the current layer if
    /// it is not placed yet.
    pub fn place_text(&mut self, at: [f64; 2], outline: Arc<Outline>) -> Result<(), FillError> {
        if let Some(placed) = self.placed.as_mut() {
            placed.at = at;
            placed.outline = outline;
            return Ok(());
        }
        self.paintable()?;
        self.settle();
        self.live = None;
        self.lasso = None;
        self.strokes_started += 1;
        self.placed = Some(Placed {
            layer: self.layer,
            at,
            outline,
            seed: self.seeds.hash_one(self.strokes_started),
        });
        Ok(())
    }

    /// Lays the placed text out anew, after its text, font or size changed.
    pub fn set_text_outline(&mut self, outline: Arc<Outline>) {
        if let Some(placed) = self.placed.as_mut() {
            placed.outline = outline;
        }
    }

    /// What the placed text draws with the pen as it is now; `None` when
    /// none is placed.
    pub fn text_drawing(&self) -> Option<TextDrawing> {
        let placed = self.placed.as_ref()?;
        let template = Stroke {
            points: Arc::from([]),
            color: self.pen.color,
            width: self
                .pen
                .width
                .clamp(*limits::STROKE_WIDTH.start(), *limits::STROKE_WIDTH.end()),
            brush: Brush {
                antialias: self.pen.antialias,
                ..self.pen.preset.brush
            },
            seed: 0,
        };
        let contours = &placed.outline.contours;
        Some(TextDrawing {
            layer: placed.layer,
            fill: self
                .text
                .filled
                .then(|| text::coverage(contours, placed.at, self.document().canvas))
                .flatten(),
            strokes: text::strokes(contours, placed.at, &template, placed.seed),
            clip: self.clip(),
            antialias: self.pen.antialias,
        })
    }

    /// Puts the placed text in the document as one step. Text that draws
    /// nothing changes nothing.
    pub fn apply_text(&mut self) -> Result<Outcome, EditError> {
        let Some(drawing) = self.text_drawing() else {
            return Ok(Outcome::NoChange);
        };
        self.placed = None;
        self.ended = Some(Ended::Dropped);
        if drawing.strokes.is_empty() && drawing.fill.is_none() {
            return Ok(Outcome::NoChange);
        }
        let color = self.pen.color;
        let outcome = self.history.group("Add text", |group| {
            let clip = match &drawing.clip {
                Some(selection) => Some(selecting::stored_mask(group, selection)?),
                None => None,
            };
            if let Some(fill) = &drawing.fill {
                let coverage = selecting::stored_mask(group, fill)?;
                group.apply(|document| {
                    command::fill(
                        document,
                        drawing.layer,
                        coverage,
                        color,
                        drawing.antialias,
                        clip,
                    )
                })?;
            }
            for stroke in drawing.strokes {
                group.apply(|document| {
                    command::draw(document, drawing.layer, stroke, false, clip)
                })?;
            }
            Ok(())
        })?;
        if let Outcome::Committed(_) = outcome {
            self.ended = Some(Ended::Applied {
                revision: self.history.revision(),
            });
            self.colors.record(color);
        }
        Ok(outcome)
    }

    /// Drops the placed text; returns whether there was some.
    pub fn cancel_text(&mut self) -> bool {
        let cancelled = self.placed.take().is_some();
        if cancelled {
            self.ended = Some(Ended::Dropped);
        }
        cancelled
    }
}

// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! Text placed on the canvas and not yet in the document. The caller lays
//! the text out with its fonts and hands the outline over; the session
//! keeps where it goes and turns it into strokes and a fill when applied,
//! in the brush's colour, width and kind the window last handed in. Like a pending
//! transform, undo, redo or Esc drop it and any other edit applies it
//! first.

use std::hash::BuildHasher;
use std::sync::Arc;

use ugu_core::command;
use ugu_core::document::LayerId;
use ugu_core::edit::{EditError, Outcome};
use ugu_core::history::Group;
use ugu_core::ops::Rgba8;
use ugu_core::selection::Selection;
use ugu_core::store::{Brush, BrushEngine, Stroke, limits};
use ugu_core::text::{self, Outline};

use crate::transforming::Ended;
use crate::{FillError, Session, ToolSettings, Tools, selecting};

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

/// What placed text is drawn with.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextStyle {
    /// The brush of the brushes for the document's mode.
    pub pen: ToolSettings,
    pub color: Rgba8,
    pub filled: bool,
}

impl TextStyle {
    pub fn of(tools: &Tools, pixel_mode: bool) -> Self {
        Self {
            pen: tools.brushes(pixel_mode).pen,
            color: tools.color,
            filled: tools.text.filled,
        }
    }
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

/// What letters are outlined with: an even line whatever brush is chosen,
/// so an airbrush or a spray still draws letters (2.2.13 drew them with the
/// brush itself); pixel art's pencil in pixel mode.
fn letter_brush(brush: &Brush) -> Brush {
    if brush.engine == BrushEngine::Pixel {
        *brush
    } else {
        ugu_core::brush::find("monoline")
            .expect("the monoline preset")
            .brush
    }
}

impl Session {
    pub fn placed_text(&self) -> Option<&Placed> {
        self.placed.as_ref()
    }

    /// What placed text is drawn with: the tools as last handed in.
    pub fn text_style(&self) -> &TextStyle {
        &self.style.text
    }

    /// Puts the text's top left at `at`, placing it on the current layer if
    /// it is not placed yet.
    pub fn place_text(
        &mut self,
        tools: &Tools,
        at: [f64; 2],
        outline: Arc<Outline>,
    ) -> Result<(), FillError> {
        self.use_tools(tools);
        if let Some(placed) = self.placed.as_mut() {
            placed.at = at;
            placed.outline = outline;
            return Ok(());
        }
        self.paintable()?;
        self.settle()?;
        self.drop_live();
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

    /// What the placed text draws with the brush as last handed in; `None`
    /// when none is placed.
    pub fn text_drawing(&self) -> Option<TextDrawing> {
        let placed = self.placed.as_ref()?;
        let style = &self.style.text;
        let template = Stroke {
            points: Arc::from([]),
            color: style.color,
            width: style
                .pen
                .width
                .clamp(*limits::STROKE_WIDTH.start(), *limits::STROKE_WIDTH.end()),
            brush: Brush {
                antialias: style.pen.antialias,
                ..letter_brush(&style.pen.preset.brush)
            },
            seed: 0,
        };
        let contours = &placed.outline.contours;
        // Laid out on screen, then taken into the layer's own pixels.
        let back = self
            .layer_transform()
            .and_then(|transform| transform.inverse());
        let fill = style
            .filled
            .then(|| text::coverage(contours, placed.at, self.document().canvas))
            .flatten();
        let mut strokes = text::strokes(contours, placed.at, &template, placed.seed);
        if let Some(back) = back {
            for stroke in &mut strokes {
                let points: Vec<_> = stroke
                    .points
                    .iter()
                    .map(|point| {
                        let [x, y] = back.apply([f64::from(point.x), f64::from(point.y)]);
                        ugu_core::store::Point {
                            x: x as f32,
                            y: y as f32,
                            ..*point
                        }
                    })
                    .collect();
                stroke.points = Arc::from(points);
            }
        }
        Some(TextDrawing {
            layer: placed.layer,
            fill: fill.and_then(|fill| match back {
                Some(back) => fill.transformed(back),
                None => Some(fill),
            }),
            strokes,
            clip: self.clip().and_then(|clip| self.in_layer(&clip)),
            antialias: style.pen.antialias && !self.pixel_mode(),
        })
    }

    /// Puts the placed text in the document as one step. Text that draws
    /// nothing changes nothing; text the document cannot take stays placed.
    pub fn apply_text(&mut self) -> Result<Outcome, EditError> {
        let Some(drawing) = self.text_drawing() else {
            return Ok(Outcome::NoChange);
        };
        if drawing.strokes.is_empty() && drawing.fill.is_none() {
            self.placed = None;
            self.ended = Some(Ended::Dropped);
            return Ok(Outcome::NoChange);
        }
        let color = self.style.text.color;
        let outcome = self
            .history
            .group("Add text", |group| text_into(group, drawing, color))?;
        self.placed = None;
        self.ended = Some(Ended::Dropped);
        if let Outcome::Committed(_) = outcome {
            self.ended = Some(Ended::Applied {
                revision: self.history.revision(),
            });
            self.used.push(color);
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

/// The steps that put `drawing` in the document.
pub(crate) fn text_into(
    group: &mut Group<'_>,
    drawing: TextDrawing,
    color: Rgba8,
) -> Result<(), EditError> {
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
        group.apply(|document| command::draw(document, drawing.layer, stroke, false, clip))?;
    }
    Ok(())
}

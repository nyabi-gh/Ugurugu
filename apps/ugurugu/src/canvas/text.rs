// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! The text tool on the canvas, as 2.2.13's: a click places the text's top
//! left, a drag on placed text moves it, and Enter or the panel applies it.
//! The text is laid out here with the installed fonts and shown on the
//! edited layer as applying it would draw it.

use std::sync::Arc;
use std::time::Instant;

use ugu_core::edit::Outcome;
use ugu_core::text::Outline;
use ugu_render::placing::Placing;
use ugu_session::{Session, TextDrawing, TextSettings};
use ugu_text::Fonts;

use super::{Canvas, Interaction, wobble_of};
use crate::i18n::tr;

/// Logical pixels around placed text that still grab it, as in 2.2.13.
const GRAB_MARGIN: f64 = 8.0;

/// The preview of placed text and what it shows.
pub(super) struct TextPreview {
    pub(super) placing: Placing,
    shown: Option<Shown>,
}

/// What the shown text was drawn from. Holding the outline and selection
/// keeps them from being dropped and another taking their place.
struct Shown {
    at: [f64; 2],
    outline: Arc<Outline>,
    pen: ugu_session::ToolSettings,
    filled: bool,
    selection: Option<Arc<ugu_core::selection::Selection>>,
}

impl PartialEq for Shown {
    fn eq(&self, other: &Self) -> bool {
        self.at == other.at
            && Arc::ptr_eq(&self.outline, &other.outline)
            && self.pen == other.pen
            && self.filled == other.filled
            && match (&self.selection, &other.selection) {
                (Some(a), Some(b)) => Arc::ptr_eq(a, b),
                (a, b) => a.is_none() && b.is_none(),
            }
    }
}

/// The fonts, opened when the text tool is first used, and the last text
/// laid out with them.
#[derive(Default)]
pub(super) struct Typesetter {
    fonts: Option<Fonts>,
    families: Vec<String>,
    laid: Option<(String, TextSettings, Arc<Outline>)>,
}

impl Typesetter {
    fn fonts(&mut self) -> &mut Fonts {
        self.fonts
            .get_or_insert_with(|| Fonts::new(crate::i18n::language()))
    }

    fn outline(&mut self, text: &str, settings: &TextSettings) -> Arc<Outline> {
        if let Some((laid_text, laid_settings, outline)) = &self.laid
            && laid_text == text
            && laid_settings == settings
        {
            return outline.clone();
        }
        let started = Instant::now();
        let outline = Arc::new(self.fonts().outline(
            text,
            settings.family.as_deref(),
            settings.size,
        ));
        tracing::debug!(
            ms = started.elapsed().as_secs_f64() * 1000.0,
            contours = outline.contours.len(),
            "text laid out"
        );
        self.laid = Some((text.to_owned(), settings.clone(), outline.clone()));
        outline
    }
}

impl Canvas {
    /// The installed font families, sorted.
    pub fn font_families(&mut self) -> &[String] {
        if self.typesetter.families.is_empty() {
            self.typesetter.families = self.typesetter.fonts().families();
        }
        &self.typesetter.families
    }

    fn text_outline(&mut self) -> Arc<Outline> {
        let session = &self.session;
        self.typesetter
            .outline(&session.text_content, &session.text)
    }

    /// Changes the text or how it is set, laying placed text out again.
    pub fn set_text(&mut self, change: impl FnOnce(&mut String, &mut TextSettings)) {
        let session = &mut self.session;
        change(&mut session.text_content, &mut session.text);
        session.text.size = session
            .text
            .size
            .clamp(*TextSettings::SIZE.start(), *TextSettings::SIZE.end());
        if self.session.placed_text().is_some() {
            let outline = self.text_outline();
            self.session.set_text_outline(outline);
        }
    }

    /// Left, top, right and bottom of placed text in document pixels; text
    /// with nothing typed still has a box to grab.
    pub fn text_box(&self) -> Option<[f64; 4]> {
        let placed = self.session.placed_text()?;
        let size = f64::from(self.session.text.size);
        let [width, height] = placed.outline.size;
        let [x, y] = placed.at;
        Some([
            x,
            y,
            x + width.max((size * 2.0).max(24.0)),
            y + height.max(size),
        ])
    }

    /// A press with the text tool: on placed text it starts moving it,
    /// elsewhere it places the text there.
    pub(super) fn press_text(&mut self, position: [f64; 2]) {
        let point = self.to_document(position);
        if let (Some([left, top, right, bottom]), Some(placed)) =
            (self.text_box(), self.session.placed_text())
        {
            let margin = GRAB_MARGIN * f64::from(self.pixels_per_point) / self.scale;
            if (left - margin..=right + margin).contains(&point[0])
                && (top - margin..=bottom + margin).contains(&point[1])
            {
                self.interaction = Interaction::MovingText {
                    start: point,
                    base: placed.at,
                };
                return;
            }
        }
        self.notice = None;
        let outline = self.text_outline();
        match self.edit(|session| session.place_text(point, outline)) {
            Ok(()) => {
                self.interaction = Interaction::MovingText {
                    start: point,
                    base: point,
                };
            }
            Err(error) => self.fill_notice(&error),
        }
    }

    /// Shown once per frame by `sync`.
    pub(super) fn drag_text(&mut self, position: [f64; 2]) {
        let Interaction::MovingText { start, base } = self.interaction else {
            return;
        };
        let Some(outline) = self
            .session
            .placed_text()
            .map(|placed| placed.outline.clone())
        else {
            return;
        };
        let point = self.to_document(position);
        let at = [base[0] + point[0] - start[0], base[1] + point[1] - start[1]];
        let _ = self.session.place_text(at, outline);
    }

    pub fn apply_text(&mut self) {
        if self.session.placed_text().is_none() {
            return;
        }
        let started = Instant::now();
        self.notice = match self.edit(Session::apply_text) {
            Ok(Outcome::Committed(_)) => {
                tracing::info!(
                    ms = started.elapsed().as_secs_f64() * 1000.0,
                    text = ?self.session.text_content,
                    "text applied"
                );
                None
            }
            Ok(Outcome::NoChange) => Some(tr("text-empty").to_owned()),
            Err(error) => Some(format!("The text was not added: {error}")),
        };
    }

    pub fn cancel_text(&mut self) {
        self.edit(Session::cancel_text);
    }

    /// Shows placed text on the split's layer, once the split of the state
    /// it goes on is here.
    pub(super) fn refresh_text_preview(&mut self) {
        let key = self.key();
        let Some(placed) = self.session.placed_text() else {
            self.text_preview = None;
            return;
        };
        let Some((shown, split)) = self.split.as_mut() else {
            self.text_preview = None;
            return;
        };
        if *shown != key || split.layer != placed.layer {
            return;
        }
        let wanted = Shown {
            at: placed.at,
            outline: placed.outline.clone(),
            pen: self.session.pen,
            filled: self.session.text.filled,
            selection: self.session.selection().cloned(),
        };
        if self.text_preview.is_none() {
            let Some(placing) = split.begin_place() else {
                return;
            };
            self.text_preview = Some(TextPreview {
                placing,
                shown: None,
            });
        }
        let preview = self.text_preview.as_mut().expect("made above");
        if preview.shown.as_ref() == Some(&wanted) {
            return;
        }
        let started = Instant::now();
        let Some(TextDrawing {
            fill,
            strokes,
            clip,
            antialias,
            ..
        }) = self.session.text_drawing()
        else {
            return;
        };
        let document = self.session.document();
        let color = self.session.pen.color.0;
        let rect = split.show_place(
            &mut preview.placing,
            &mut self.stamp,
            fill.as_ref().map(|fill| (fill.mask(), antialias, color)),
            &strokes,
            wobble_of(document, placed.layer),
            document.frames,
            clip.as_ref().map(|clip| clip.mask()),
        );
        preview.shown = Some(wanted);
        let layer = started.elapsed();
        self.recomposite(rect);
        tracing::debug!(
            ms = started.elapsed().as_secs_f64() * 1000.0,
            layer_ms = layer.as_secs_f64() * 1000.0,
            strokes = strokes.len(),
            "text shown"
        );
    }
}

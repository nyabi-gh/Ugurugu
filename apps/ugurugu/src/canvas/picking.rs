// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! The canvas's side of the eyedropper and of editing stroke properties.

use ugu_core::restyle::{Style, Touched};
use ugu_session::Tool;

use super::{Canvas, Interaction};
use crate::i18n::tr;

impl Canvas {
    /// Whether a press picks a colour, with Alt held or not: the eyedropper,
    /// or Alt with a tool that paints, as in 2.2.13. The selection tools take
    /// Alt to subtract.
    pub fn picks_with(&self, alt: bool) -> bool {
        match self.session.tool() {
            Tool::Eyedropper => true,
            Tool::Pen | Tool::Eraser | Tool::Fill => alt,
            Tool::Select | Tool::Wand | Tool::Text => false,
        }
    }

    pub(super) fn picks(&self) -> bool {
        self.picks_with(self.modifiers.1)
    }

    pub(super) fn begin_pick(&mut self, position: [f64; 2]) {
        self.interaction = Interaction::Picking;
        self.pick(position);
    }

    /// Makes the pen colour that of the pixel shown at `position` (client
    /// physical pixels): the frame being edited, with any pending transform
    /// or placed text, or the playback frame on screen.
    pub(super) fn pick(&mut self, position: [f64; 2]) {
        let [x, y] = self.to_document(position);
        let [width, height] = self.session.document().canvas.map(f64::from);
        if !(0.0..width).contains(&x) || !(0.0..height).contains(&y) {
            return;
        }
        let (pixels, shrink) = self.shown();
        let shrink = f64::from(shrink);
        let column = ((x / shrink) as usize).min(usize::from(pixels.width()) - 1);
        let row = ((y / shrink) as usize).min(usize::from(pixels.height()) - 1);
        let pixel = pixels.data_as_u8_slice().as_chunks::<4>().0
            [row * usize::from(pixels.width()) + column];
        self.session.pick_color(pixel);
    }

    /// What the selection touches on the current layer, for the stroke
    /// properties dialog; `None`, with the reason shown, when there is
    /// nothing to edit.
    pub fn touched(&mut self) -> Option<Touched> {
        self.notice = None;
        match self.session.touched() {
            Ok(touched) if touched.colored + touched.sized > 0 => Some(touched),
            Ok(_) => {
                self.notice = Some(tr("restyle-nothing").to_owned());
                None
            }
            Err(error) => {
                self.fill_notice(&error);
                None
            }
        }
    }

    /// Gives what the selection touches `style`.
    pub fn restyle(&mut self, style: Style) {
        let started = std::time::Instant::now();
        self.notice = None;
        if let Err(error) = self.edit(|session| session.restyle_selected(style)) {
            self.fill_notice(&error);
        }
        tracing::debug!(
            ms = started.elapsed().as_secs_f64() * 1000.0,
            "stroke properties applied"
        );
    }
}

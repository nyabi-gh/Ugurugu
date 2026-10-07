// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! M0 IME probe: text fields for Korean and Japanese composition, and canvas
//! shortcuts that must not fire while text is being edited.

#[derive(Default)]
pub struct ImeProbe {
    line: String,
    text: String,
    /// Field rectangles last logged, so that tests can click them.
    logged_rects: Option<[egui::Rect; 2]>,
}

impl ImeProbe {
    pub fn show(&mut self, ui: &mut egui::Ui) {
        ui.heading("IME");
        // Labelled so that screen readers name the fields.
        let line_label = ui.label("Line");
        let line = ui
            .add(egui::TextEdit::singleline(&mut self.line).id_salt("ime-line"))
            .labelled_by(line_label.id);
        let text_label = ui.label("Text");
        let text = ui
            .add(
                egui::TextEdit::multiline(&mut self.text)
                    .id_salt("ime-text")
                    .desired_rows(4),
            )
            .labelled_by(text_label.id);
        let rects = [line.rect, text.rect];
        if self.logged_rects != Some(rects) {
            let ppp = ui.ctx().pixels_per_point();
            let physical = |rect: egui::Rect| {
                [
                    rect.min.x * ppp,
                    rect.min.y * ppp,
                    rect.max.x * ppp,
                    rect.max.y * ppp,
                ]
            };
            tracing::debug!(line = ?physical(line.rect), text = ?physical(text.rect), "ime probe fields");
            self.logged_rects = Some(rects);
        }
        if line.changed() || text.changed() {
            tracing::debug!(line = ?self.line, text = ?self.text, "ime probe text");
        }
    }
}

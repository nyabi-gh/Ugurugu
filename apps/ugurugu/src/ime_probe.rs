// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! M0 IME probe: text fields for Korean and Japanese composition, and canvas
//! shortcuts that must not fire while text is being edited.

use std::sync::Arc;

/// System fonts covering Korean and Japanese, tried in order. M0 only: the
/// product bundles its fonts.
const CJK_FONTS: [(&str, &str); 2] = [
    ("malgun.ttf", "Malgun Gothic"),
    ("YuGothR.ttc", "Yu Gothic"),
];

/// Adds the CJK system fonts as fallbacks after egui's own fonts.
pub fn install_cjk_fonts(ctx: &egui::Context) {
    let fonts_dir = std::env::var_os("WINDIR")
        .map(|windows| std::path::PathBuf::from(windows).join("Fonts"))
        .unwrap_or_default();
    let mut definitions = egui::FontDefinitions::default();
    for (file, name) in CJK_FONTS {
        match std::fs::read(fonts_dir.join(file)) {
            Ok(bytes) => {
                definitions
                    .font_data
                    .insert(name.to_owned(), Arc::new(egui::FontData::from_owned(bytes)));
                for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
                    definitions
                        .families
                        .entry(family)
                        .or_default()
                        .push(name.to_owned());
                }
            }
            Err(error) => tracing::warn!(file, %error, "CJK font not loaded"),
        }
    }
    ctx.set_fonts(definitions);
}

#[derive(Default)]
pub struct ImeProbe {
    line: String,
    text: String,
    /// Enter and Ctrl+Z that reached the canvas.
    canvas_enter: u32,
    canvas_undo: u32,
    /// Field rectangles last logged, so that tests can click them.
    logged_rects: Option<[egui::Rect; 2]>,
}

impl ImeProbe {
    pub fn show(&mut self, ui: &mut egui::Ui) {
        ui.heading("IME");
        let line = ui.add(egui::TextEdit::singleline(&mut self.line).id_salt("ime-line"));
        let text = ui.add(
            egui::TextEdit::multiline(&mut self.text)
                .id_salt("ime-text")
                .desired_rows(4),
        );
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
        ui.label(format!(
            "canvas Enter {}  Ctrl+Z {}",
            self.canvas_enter, self.canvas_undo
        ));
    }

    /// Counts canvas shortcuts in this frame's input. Text editing keeps
    /// them, including Enter that commits a composition.
    pub fn count_canvas_shortcuts(&mut self, ctx: &egui::Context) {
        if ctx.egui_wants_keyboard_input() {
            return;
        }
        // A whole chord can arrive within one frame, so each key is matched
        // with the modifiers it was pressed with, not the frame's last state.
        let (enter, undo) = ctx.input_mut(|input| {
            (
                input.consume_key(egui::Modifiers::NONE, egui::Key::Enter),
                input.consume_key(egui::Modifiers::COMMAND, egui::Key::Z),
            )
        });
        if enter {
            self.canvas_enter += 1;
            tracing::debug!(count = self.canvas_enter, "canvas Enter");
        }
        if undo {
            self.canvas_undo += 1;
            tracing::debug!(count = self.canvas_undo, "canvas Ctrl+Z");
        }
    }
}

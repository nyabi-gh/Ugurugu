// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! M0 IME probe: text fields for Korean and Japanese composition, and canvas
//! shortcuts that must not fire while text is being edited.

use std::sync::Arc;

/// System fonts covering Korean and Japanese, tried in order, each with a
/// character that only it covers. M0 only: the product bundles its fonts.
const CJK_FONTS: [(&str, &str, char); 2] = [
    ("malgun.ttf", "Malgun Gothic", '한'),
    ("YuGothR.ttc", "Yu Gothic", '学'),
];

/// Adds the CJK system fonts as fallbacks after egui's own fonts.
pub fn install_cjk_fonts(ctx: &egui::Context) {
    let fonts_dir = std::env::var_os("WINDIR")
        .map(|windows| std::path::PathBuf::from(windows).join("Fonts"))
        .unwrap_or_default();
    let mut definitions = egui::FontDefinitions::default();
    let mut loaded = Vec::new();
    for (file, name, sample) in CJK_FONTS {
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
                loaded.push((name, sample));
            }
            Err(error) => tracing::warn!(file, %error, "CJK font not loaded"),
        }
    }
    align_baselines(&mut definitions, &loaded);
    ctx.set_fonts(definitions);
}

/// Puts each fallback font on the primary font's baseline. epaint centres a
/// fallback glyph by the difference in row height, which includes the line
/// gap, so a font with a large gap (Yu Gothic) draws its glyphs well above
/// the line. The offset is measured with epaint's own layout, against the
/// proportional family that text fields use.
fn align_baselines(definitions: &mut egui::FontDefinitions, fonts: &[(&str, char)]) {
    use egui::epaint::text::{Fonts, LayoutJob, TextOptions};
    const SIZE: f32 = 1000.0;
    let mut measure = Fonts::new(TextOptions::default(), definitions.clone());
    let mut view = measure.with_pixels_per_point(1.0);
    for &(name, sample) in fonts {
        let galley = view.layout_job(LayoutJob::simple_singleline(
            format!("x{sample}"),
            egui::FontId::proportional(SIZE),
            egui::Color32::BLACK,
        ));
        let glyphs = &galley.rows[0].row.glyphs;
        let [primary, fallback] = [&glyphs[0], &glyphs[1]];
        if let Some(data) = definitions.font_data.get_mut(name) {
            let factor = (primary.pos.y - fallback.pos.y) / SIZE;
            tracing::debug!(name, factor, "fallback baseline offset");
            Arc::make_mut(data).tweak.y_offset_factor = factor;
        }
    }
}

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

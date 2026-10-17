// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! The text tool's settings, as 2.2.13's text panel: the text, its font and
//! size, whether the letters are filled, and placing or cancelling it. The
//! letters take the brush's colour and width, outlined with an even line.

use ugu_session::TextSettings;

use crate::canvas::Canvas;
use crate::i18n::tr;
use crate::{theme, widgets};

pub fn settings(ui: &mut egui::Ui, canvas: &mut Canvas) {
    let label = widgets::field_label(ui, tr("text-content-label"));
    let mut content = canvas.session().text_content.clone();
    let edit = ui
        .add(
            egui::TextEdit::multiline(&mut content)
                .id_salt("text-content")
                .hint_text(tr("text-content-hint"))
                .desired_rows(3)
                .desired_width(f32::INFINITY),
        )
        .labelled_by(label.id);
    // The IME test clicks the field and reads what it holds.
    let field = edit.rect * ui.ctx().pixels_per_point();
    tracing::trace!(rect = ?[field.min.x, field.min.y, field.max.x, field.max.y], "text tool field");
    if edit.changed() {
        log_content(&content, crate::render::diagnostics());
        canvas.set_text(|text, _| *text = content);
    }
    ui.add_space(theme::space::S);

    widgets::field_label(ui, tr("text-font"));
    let mut family = canvas.tools().text.family.clone();
    let shown = family
        .clone()
        .unwrap_or_else(|| tr("text-font-default").to_owned());
    egui::ComboBox::from_id_salt("text-font")
        .selected_text(shown)
        .width(ui.available_width())
        .height(320.0)
        .show_ui(ui, |ui| {
            widgets::choice(ui, &mut family, None, tr("text-font-default"));
            for name in canvas.font_families().to_vec() {
                let chosen = Some(name.clone());
                widgets::choice(ui, &mut family, chosen, &name);
            }
        });
    if family != canvas.tools().text.family {
        canvas.set_text(|_, settings| settings.family = family);
    }

    let mut size = canvas.tools().text.size;
    widgets::form_row(ui, tr("text-size"), |ui| {
        widgets::number(
            ui,
            &mut size,
            TextSettings::SIZE,
            " px",
            0,
            72.0,
            tr("text-size"),
        )
    });
    let size = size.round();
    if size != canvas.tools().text.size {
        canvas.set_text(|_, settings| settings.size = size);
    }
    let mut filled = canvas.tools().text.filled;
    widgets::check_row(ui, tr("text-filled"), tr("text-filled"), &mut filled);
    if filled != canvas.tools().text.filled {
        canvas.set_text(|_, settings| settings.filled = filled);
    }
    ui.add_space(theme::space::S);

    let placed = canvas.session().placed_text().is_some();
    ui.horizontal(|ui| {
        let secondary = widgets::Emphasis::Secondary;
        if widgets::button(ui, tr("text-place"), secondary, placed).clicked() {
            canvas.apply_text();
        }
        if widgets::button(ui, tr("text-cancel"), secondary, placed).clicked() {
            canvas.cancel_text();
        }
    });
    ui.add_space(theme::space::S);
    ui.label(
        egui::RichText::new(tr("text-hint"))
            .size(theme::SMALL)
            .color(theme::muted()),
    );
}

/// The log keeps none of the user's writing, only its length; with the
/// test keys on, the IME test reads the text itself.
fn log_content(content: &str, diagnostics: bool) {
    if diagnostics {
        tracing::debug!(content = ?content, "text tool content");
    } else {
        tracing::debug!(chars = content.chars().count(), "text tool content");
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    /// What `log` writes to the log at debug.
    fn logged(log: impl FnOnce()) -> String {
        let written = Arc::new(Mutex::new(Vec::new()));
        let sink = written.clone();
        let subscriber = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::DEBUG)
            .with_ansi(false)
            .with_writer(move || Writer(sink.clone()))
            .finish();
        tracing::subscriber::with_default(subscriber, log);
        String::from_utf8(written.lock().unwrap().clone()).unwrap()
    }

    struct Writer(Arc<Mutex<Vec<u8>>>);

    impl std::io::Write for Writer {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn the_log_keeps_the_texts_length_and_not_the_text() {
        let line = logged(|| super::log_content("비밀 메모", false));
        assert!(line.contains("text tool content"), "{line}");
        assert!(line.contains("chars=5"), "{line}");
        assert!(!line.contains("비밀"), "{line}");
        // The IME test reads it, with test keys on.
        let line = logged(|| super::log_content("비밀 메모", true));
        assert!(line.contains("비밀 메모"), "{line}");
    }
}

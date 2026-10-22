// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! The canvas background dialog, after 2.2.13's `chooseBackgroundColor`: a
//! colour with alpha, where alpha 0 is a transparent background.

use ugu_core::ops::Rgba8;

use super::Panels;
use crate::canvas::Canvas;
use crate::i18n::tr;
use crate::{theme, widgets};

pub struct Dialog {
    /// Straight RGBA.
    pub(super) color: [u8; 4],
}

pub fn open(canvas: &Canvas, panels: &mut Panels) {
    panels.background = Some(Dialog {
        color: canvas.session().document().background.0,
    });
}

/// Shows the open dialog and sets the background when confirmed, as one
/// step.
pub fn show(ctx: &egui::Context, canvas: &mut Canvas, panels: &mut Panels) {
    let Some(dialog) = panels.background.as_mut() else {
        return;
    };
    let mut close = false;
    let mut apply = false;
    let modal = widgets::dialog(
        ctx,
        "background dialog",
        400.0,
        tr("background-title"),
        Some(tr("background-description")),
        |ui| {
            widgets::card(ui, |ui| {
                let [r, g, b, a] = dialog.color;
                let mut color = egui::Color32::from_rgba_unmultiplied(r, g, b, a);
                if egui::color_picker::color_picker_color32(
                    ui,
                    &mut color,
                    egui::color_picker::Alpha::OnlyBlend,
                ) {
                    dialog.color = color.to_srgba_unmultiplied();
                }
                ui.add_space(theme::space::XS);
            });
            widgets::dialog_buttons(ui, |ui| {
                let ok = widgets::button(ui, tr("dialog-ok"), widgets::Emphasis::Primary, true);
                if widgets::button(ui, tr("dialog-cancel"), widgets::Emphasis::Secondary, true)
                    .clicked()
                {
                    close = true;
                }
                let enter = ui.input(|input| input.key_pressed(egui::Key::Enter));
                if ok.clicked() || enter {
                    apply = true;
                }
            });
        },
    );
    if apply {
        let result = canvas.edit(|session| session.set_background(Rgba8(dialog.color)));
        panels.report(result);
        close = true;
    }
    if close || modal.should_close() {
        panels.background = None;
    }
}

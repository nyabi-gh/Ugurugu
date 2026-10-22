// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! The canvas background dialog, after 2.2.13's `chooseBackgroundColor`: a
//! colour with alpha, where alpha 0 is a transparent background.

use egui::ecolor::Hsva;
use ugu_core::ops::Rgba8;

use super::Panels;
use crate::canvas::Canvas;
use crate::i18n::tr;
use crate::{theme, widgets};

pub struct Dialog {
    /// Unmultiplied, so that the colour stays while its alpha is 0: egui's
    /// `Color32` is premultiplied and would make it black.
    color: Hsva,
}

impl Dialog {
    fn new(background: Rgba8) -> Self {
        // Not `Hsva::from_srgba_unmultiplied`, which goes through `Color32`.
        let [r, g, b, a] = background.0;
        let linear = egui::ecolor::linear_f32_from_gamma_u8;
        Self {
            color: Hsva::from_rgba_unmultiplied(
                linear(r),
                linear(g),
                linear(b),
                f32::from(a) / 255.0,
            ),
        }
    }

    /// Straight RGBA.
    pub(super) fn color(&self) -> [u8; 4] {
        self.color.to_srgba_unmultiplied()
    }

    #[cfg(test)]
    pub(super) fn set_alpha(&mut self, alpha: f32) {
        self.color.a = alpha;
    }
}

pub fn open(canvas: &Canvas, panels: &mut Panels) {
    panels.background = Some(Dialog::new(canvas.session().document().background));
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
                egui::color_picker::color_picker_hsva_2d(
                    ui,
                    &mut dialog.color,
                    egui::color_picker::Alpha::OnlyBlend,
                );
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
        let color = Rgba8(dialog.color());
        let result = canvas.edit(|session| session.set_background(color));
        panels.report(result);
        close = true;
    }
    if close || modal.should_close() {
        panels.background = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_colour_keeps_itself_through_alpha_0_and_back() {
        let mut dialog = Dialog::new(Rgba8([200, 100, 50, 255]));
        dialog.set_alpha(0.0);
        assert_eq!(dialog.color(), [200, 100, 50, 0]);
        dialog.set_alpha(1.0);
        assert_eq!(dialog.color(), [200, 100, 50, 255]);
        // A transparent background opens with its own colour, not black.
        let low = Dialog::new(Rgba8([200, 100, 50, 10]));
        assert_eq!(low.color(), [200, 100, 50, 10]);
    }
}

// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! The stroke properties dialog, after 2.2.13's `StrokePropertiesDialog`: a
//! colour and a width, each changed only when ticked.

use ugu_core::ops::Rgba8;
use ugu_core::restyle::{Style, Touched};
use ugu_core::store::limits::STROKE_WIDTH;

use super::{Panels, resize};
use crate::canvas::Canvas;
use crate::i18n::tr;

pub struct Dialog {
    colorable: bool,
    sizable: bool,
    color_on: bool,
    /// Straight RGBA.
    color: [u8; 4],
    width_on: bool,
    width: f32,
}

impl Dialog {
    /// Starts from the colour and width all of `touched` share, ticked; a
    /// mix starts unticked, from black and 6 px, as in 2.2.13.
    pub fn new(touched: Touched) -> Self {
        let (colorable, sizable) = (touched.colored > 0, touched.sized > 0);
        Self {
            colorable,
            sizable,
            color_on: colorable && touched.color.is_some(),
            color: touched.color.map_or([0, 0, 0, 255], |color| color.0),
            width_on: sizable && touched.width.is_some(),
            width: touched.width.unwrap_or(6.0),
        }
    }

    pub fn style(&self) -> Style {
        Style {
            color: (self.colorable && self.color_on).then_some(Rgba8(self.color)),
            width: (self.sizable && self.width_on).then_some(self.width),
        }
    }
}

/// Opens the dialog for what the selection touches, or says why not.
pub fn open(canvas: &mut Canvas, panels: &mut Panels) {
    panels.restyle = canvas.touched().map(Dialog::new);
}

/// Whether the dialog can open now: with a selection and nothing pending.
pub fn available(canvas: &Canvas) -> bool {
    let session = canvas.session();
    session.selection().is_some() && session.pending().is_none() && session.placed_text().is_none()
}

/// Shows the open dialog and applies it when confirmed.
pub fn show(ctx: &egui::Context, canvas: &mut Canvas, panels: &mut Panels) {
    let Some(dialog) = panels.restyle.as_mut() else {
        return;
    };
    let mut close = false;
    let mut apply = false;
    let modal = egui::Modal::new(egui::Id::new("restyle dialog")).show(ctx, |ui| {
        ui.set_width(340.0);
        resize::heading(ui, tr("restyle-title"), tr("restyle-description"));
        egui::Grid::new("restyle fields")
            .num_columns(2)
            .spacing([12.0, 8.0])
            .show(ui, |ui| {
                ui.add_enabled(
                    dialog.colorable,
                    egui::Checkbox::new(&mut dialog.color_on, tr("restyle-color")),
                );
                let picked = ui
                    .add_enabled_ui(dialog.colorable, |ui| {
                        ui.color_edit_button_srgba_unmultiplied(&mut dialog.color)
                    })
                    .inner
                    .on_hover_text(tr("restyle-choose-color"));
                if picked.changed() {
                    dialog.color_on = true;
                }
                ui.end_row();
                ui.add_enabled(
                    dialog.sizable,
                    egui::Checkbox::new(&mut dialog.width_on, tr("restyle-width")),
                );
                let width = ui.add_enabled(
                    dialog.sizable,
                    egui::DragValue::new(&mut dialog.width)
                        .range(STROKE_WIDTH)
                        .speed(0.1)
                        .fixed_decimals(2)
                        .suffix(" px"),
                );
                let shown = f64::from(dialog.width);
                width.widget_info(|| {
                    let mut info = egui::WidgetInfo::drag_value(dialog.sizable, shown);
                    info.label = Some(tr("restyle-width").to_owned());
                    info
                });
                if width.changed() {
                    dialog.width_on = true;
                }
                ui.end_row();
            });
        let style = dialog.style();
        let can_apply = style.color.is_some() || style.width.is_some();
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button(tr("dialog-cancel")).clicked() {
                    close = true;
                }
                let ok = ui.add_enabled(can_apply, egui::Button::new(tr("dialog-ok")));
                let enter = ui.input(|input| input.key_pressed(egui::Key::Enter));
                if ok.clicked() || (enter && can_apply) {
                    apply = true;
                }
            });
        });
    });
    if apply {
        canvas.restyle(dialog.style());
        close = true;
    }
    if close || modal.should_close() {
        panels.restyle = None;
    }
}

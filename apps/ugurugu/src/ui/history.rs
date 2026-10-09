// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! The colour history dock, as 2.2.13's: the colours strokes, fills and text
//! were made with, newest first; a click makes one the pen colour. Only used
//! colours are shown, not 2.2.13's 256 empty slots.

use egui::{Color32, CornerRadius, Sense, Stroke, Ui};
use ugu_core::ops::Rgba8;

use crate::canvas::Canvas;
use crate::i18n::{tr, tr_with};
use crate::theme;

use super::args;

/// 2.2.13's swatch size and gap.
const SWATCH: f32 = 22.0;
const GAP: f32 = 2.0;

/// `#AARRGGBB`, as 2.2.13 names colours.
pub fn hex(Rgba8([r, g, b, a]): Rgba8) -> String {
    format!("#{a:02X}{r:02X}{g:02X}{b:02X}")
}

/// A colour over a two-tone check, so that see-through colours show as such.
pub fn paint_swatch(ui: &Ui, rect: egui::Rect, color: Rgba8) {
    let painter = ui.painter();
    let radius = CornerRadius::same(3);
    let Rgba8([r, g, b, a]) = color;
    if a < 255 {
        painter.rect_filled(rect, radius, Color32::from_gray(200));
        let half = rect.size() / 2.0;
        for corner in [rect.min, rect.min + half] {
            painter.rect_filled(
                egui::Rect::from_min_size(corner, half),
                CornerRadius::ZERO,
                Color32::from_gray(150),
            );
        }
    }
    painter.rect_filled(rect, radius, Color32::from_rgba_unmultiplied(r, g, b, a));
}

pub fn show(ui: &mut Ui, canvas: &mut Canvas) {
    let colors = canvas.session().colors.colors().to_vec();
    let current = canvas.session().pen.color;
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let clear = ui.add_enabled(
                !colors.is_empty(),
                egui::Button::new(
                    egui::RichText::new(tr("color-history-clear")).size(theme::SMALL),
                ),
            );
            if clear.clicked() {
                canvas.edit(|session| session.colors.clear());
            }
        });
    });
    let mut chosen = None;
    egui::ScrollArea::vertical()
        .id_salt("colour history")
        .auto_shrink([false, true])
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing = egui::vec2(GAP, GAP);
            ui.horizontal_wrapped(|ui| {
                for color in colors {
                    let (rect, response) =
                        ui.allocate_exact_size(egui::Vec2::splat(SWATCH), Sense::click());
                    paint_swatch(ui, rect, color);
                    let (width, stroke) = if color == current {
                        (2.0, theme::accent())
                    } else if response.hovered() {
                        (1.0, theme::TEXT)
                    } else {
                        (1.0, theme::BORDER)
                    };
                    ui.painter().rect_stroke(
                        rect,
                        CornerRadius::same(3),
                        Stroke::new(width, stroke),
                        egui::StrokeKind::Inside,
                    );
                    let name = tr_with("color-history-swatch", &args([("color", hex(color))]));
                    response.widget_info(|| {
                        egui::WidgetInfo::selected(
                            egui::WidgetType::Button,
                            true,
                            color == current,
                            &name,
                        )
                    });
                    if response.on_hover_text(hex(color)).clicked() {
                        chosen = Some(color);
                    }
                }
            });
        });
    if let Some(color) = chosen {
        canvas.edit(|session| session.pen.color = color);
    }
}

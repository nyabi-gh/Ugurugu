// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! Controls drawn the way 2.2.13's style sheet draws them: tool buttons with
//! an amber checked state, thin sliders with a round handle, and number
//! fields without arrows.

use egui::{Color32, CornerRadius, FontId, Rect, Response, Sense, Stroke, StrokeKind, Ui, Vec2};

use crate::icons::{self, Glyph};
use crate::theme;

/// A tool rail button: the glyph alone, filled with the accent while it is
/// the current tool; `name` and `shortcut` show on hover.
pub fn tool_button(
    ui: &mut Ui,
    glyph: Glyph,
    name: &str,
    shortcut: &str,
    checked: bool,
) -> Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(40.0), Sense::click());
    response
        .widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, checked, name));
    let (fill, ink) = if checked {
        let fill = if response.hovered() {
            theme::accent_pressed()
        } else {
            theme::accent()
        };
        (fill, theme::accent_text())
    } else if response.hovered() {
        (theme::hover(), theme::text())
    } else {
        (Color32::TRANSPARENT, theme::text())
    };
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(7), fill);
    if response.has_focus() {
        painter.rect_stroke(
            rect,
            CornerRadius::same(7),
            Stroke::new(1.0, theme::accent_ink()),
            StrokeKind::Inside,
        );
    }
    icons::paint(
        painter,
        Rect::from_center_size(rect.center(), Vec2::splat(22.0)),
        glyph,
        ink,
        0.0,
    );
    response.on_hover_text(with_key(name, shortcut))
}

/// A tooltip naming the key that does the same, if there is one.
pub fn with_key(tip: &str, key: &str) -> String {
    if key.is_empty() {
        tip.to_owned()
    } else {
        format!("{tip} ({key})")
    }
}

/// A flat button showing only `glyph`, named `name` for the tooltip and
/// screen readers.
pub fn icon_button(
    ui: &mut Ui,
    glyph: Glyph,
    glyph_size: f32,
    name: &str,
    enabled: bool,
) -> Response {
    icon_button_tip(ui, glyph, glyph_size, name, name, enabled)
}

/// An icon button whose tooltip says more than its name, such as why it is
/// disabled.
pub fn icon_button_tip(
    ui: &mut Ui,
    glyph: Glyph,
    glyph_size: f32,
    name: &str,
    tip: &str,
    enabled: bool,
) -> Response {
    let side = glyph_size + 8.0;
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(side), sense);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, name));
    if enabled && response.hovered() {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(7), theme::hover());
    }
    let ink = if enabled {
        theme::text()
    } else {
        theme::disabled()
    };
    icons::paint(
        ui.painter(),
        Rect::from_center_size(rect.center(), Vec2::splat(glyph_size)),
        glyph,
        ink,
        0.0,
    );
    response.on_hover_text(tip)
}

/// An icon button that stays filled with the accent while `checked`.
pub fn icon_toggle(
    ui: &mut Ui,
    glyph: Glyph,
    glyph_size: f32,
    name: &str,
    tip: &str,
    checked: bool,
) -> Response {
    let side = glyph_size + 8.0;
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(side), Sense::click());
    response
        .widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, checked, name));
    let (fill, ink) = match (checked, response.hovered()) {
        (true, true) => (theme::accent_pressed(), theme::accent_text()),
        (true, false) => (theme::accent(), theme::accent_text()),
        (false, true) => (theme::hover(), theme::text()),
        (false, false) => (Color32::TRANSPARENT, theme::text()),
    };
    ui.painter().rect_filled(rect, CornerRadius::same(7), fill);
    icons::paint(
        ui.painter(),
        Rect::from_center_size(rect.center(), Vec2::splat(glyph_size)),
        glyph,
        ink,
        0.0,
    );
    response.on_hover_text(tip)
}

/// A flat button with a small glyph and its text beside it, as 2.2.13's
/// status bar and animation bar buttons.
pub fn text_icon_button(ui: &mut Ui, glyph: Glyph, text: &str, tip: &str) -> Response {
    let galley = ui.painter().layout_no_wrap(
        text.to_owned(),
        FontId::proportional(theme::BODY),
        theme::text(),
    );
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(6.0 + 16.0 + 4.0 + galley.size().x + 6.0, 24.0),
        Sense::click(),
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, text));
    if response.hovered() {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(5), theme::hover());
    }
    let icon = Rect::from_center_size(
        egui::pos2(rect.left() + 6.0 + 8.0, rect.center().y),
        Vec2::splat(16.0),
    );
    icons::paint(ui.painter(), icon, glyph, theme::text(), 0.0);
    ui.painter().galley(
        egui::pos2(icon.right() + 4.0, rect.center().y - galley.size().y / 2.0),
        galley,
        theme::text(),
    );
    response.on_hover_text(tip)
}

/// A muted caption naming the control under or beside it.
pub fn field_label(ui: &mut Ui, text: &str) -> Response {
    ui.label(
        egui::RichText::new(text)
            .size(theme::SMALL)
            .color(theme::muted()),
    )
}

/// A slider: a 5-point groove filled with the accent up to the value and a
/// round handle that turns amber when the pointer is on it.
pub fn slider(
    ui: &mut Ui,
    value: &mut f32,
    range: std::ops::RangeInclusive<f32>,
    name: &str,
    width: f32,
) -> Response {
    let height = 18.0;
    let (rect, mut response) =
        ui.allocate_exact_size(egui::vec2(width, height), Sense::click_and_drag());
    let (low, high) = (*range.start(), *range.end());
    let handle = 8.0;
    let left = rect.left() + handle;
    let span = (rect.width() - 2.0 * handle).max(1.0);
    if let Some(pointer) = response.interact_pointer_pos() {
        let t = ((pointer.x - left) / span).clamp(0.0, 1.0);
        let new = low + t * (high - low);
        if new != *value {
            *value = new;
            response.mark_changed();
        }
    }
    if response.has_focus() {
        let step = (high - low) / 100.0;
        let delta = ui.input(|input| {
            f32::from(
                i8::from(input.key_pressed(egui::Key::ArrowRight))
                    - i8::from(input.key_pressed(egui::Key::ArrowLeft)),
            )
        });
        if delta != 0.0 {
            *value = (*value + delta * step).clamp(low, high);
            response.mark_changed();
        }
    }
    let t = if high > low {
        ((*value - low) / (high - low)).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let x = left + t * span;
    let painter = ui.painter();
    let groove = Rect::from_min_max(
        egui::pos2(left, rect.center().y - 2.5),
        egui::pos2(left + span, rect.center().y + 2.5),
    );
    painter.rect_filled(groove, CornerRadius::same(2), theme::border());
    let filled = Rect::from_min_max(groove.min, egui::pos2(x, groove.max.y));
    painter.rect_filled(filled, CornerRadius::same(2), theme::accent());
    let hot = response.hovered() || response.dragged();
    painter.circle(
        egui::pos2(x, rect.center().y),
        handle,
        if hot {
            theme::accent_ink()
        } else {
            theme::text()
        },
        Stroke::new(
            2.0,
            if response.has_focus() {
                theme::accent_ink()
            } else {
                theme::chrome()
            },
        ),
    );
    response.widget_info(|| egui::WidgetInfo::slider(true, f64::from(*value), name));
    response
}

/// A number field without arrows: type a value, or drag sideways. `name`
/// is what screen readers call it.
pub fn number(
    ui: &mut Ui,
    value: &mut f32,
    range: std::ops::RangeInclusive<f32>,
    suffix: &str,
    decimals: usize,
    width: f32,
    name: &str,
) -> Response {
    let response = ui
        .scope(|ui| {
            let widgets = &mut ui.visuals_mut().widgets;
            widgets.inactive.weak_bg_fill = theme::base();
            widgets.inactive.bg_fill = theme::base();
            widgets.hovered.weak_bg_fill = theme::control();
            widgets.hovered.bg_fill = theme::control();
            widgets.active.weak_bg_fill = theme::base();
            widgets.active.bg_fill = theme::base();
            ui.add_sized(
                [width, 26.0],
                egui::DragValue::new(value)
                    .range(range)
                    .suffix(suffix)
                    .max_decimals(decimals)
                    .min_decimals(decimals)
                    .speed(0.2),
            )
        })
        .inner;
    let shown = f64::from(*value);
    response.widget_info(|| {
        let mut info = egui::WidgetInfo::drag_value(true, shown);
        info.label = Some(name.to_owned());
        info
    });
    response
}

/// A square check box, amber with a dark tick when checked, followed by
/// `text` if there is any.
pub fn checkbox(ui: &mut Ui, checked: &mut bool, text: &str) -> Response {
    let galley = (!text.is_empty()).then(|| {
        ui.painter().layout_no_wrap(
            text.to_owned(),
            FontId::proportional(theme::BODY),
            theme::text(),
        )
    });
    let width = 16.0 + galley.as_ref().map_or(0.0, |galley| 6.0 + galley.size().x);
    let (rect, mut response) = ui.allocate_exact_size(egui::vec2(width, 20.0), Sense::click());
    if response.clicked() {
        *checked = !*checked;
        response.mark_changed();
    }
    let name = if text.is_empty() { None } else { Some(text) };
    response.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::WidgetType::Checkbox,
            true,
            *checked,
            name.unwrap_or_default(),
        )
    });
    let square = Rect::from_min_size(
        egui::pos2(rect.left(), rect.center().y - 8.0),
        Vec2::splat(16.0),
    );
    let painter = ui.painter();
    let (fill, edge) = match (*checked, response.hovered()) {
        (true, _) => (theme::accent(), theme::accent_ink()),
        (false, true) => (theme::control(), theme::disabled()),
        (false, false) => (theme::base(), theme::border()),
    };
    painter.rect(
        square,
        CornerRadius::same(4),
        fill,
        Stroke::new(1.0, edge),
        StrokeKind::Inside,
    );
    if *checked {
        let at = |x: f32, y: f32| square.min + egui::vec2(x, y);
        painter.add(egui::Shape::line(
            vec![at(4.0, 8.4), at(7.0, 11.4), at(12.2, 5.2)],
            Stroke::new(2.0, theme::accent_text()),
        ));
    }
    if response.has_focus() {
        painter.rect_stroke(
            square.expand(2.0),
            CornerRadius::same(5),
            Stroke::new(1.0, theme::accent_ink()),
            StrokeKind::Outside,
        );
    }
    if let Some(galley) = galley {
        painter.galley(
            egui::pos2(
                square.right() + 6.0,
                rect.center().y - galley.size().y / 2.0,
            ),
            galley,
            theme::text(),
        );
    }
    response
}

/// A caption on the left and a check box on the right, as 2.2.13's option
/// rows.
pub fn check_row(ui: &mut Ui, label: &str, name: &str, checked: &mut bool) -> Response {
    ui.horizontal(|ui| {
        field_label(ui, label);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let response = checkbox(ui, checked, "");
            response.widget_info(|| {
                egui::WidgetInfo::selected(egui::WidgetType::Checkbox, true, *checked, name)
            });
            response
        })
        .inner
    })
    .inner
}

/// A whole number field; `signed` shows a plus on positive values.
pub fn whole(
    ui: &mut egui::Ui,
    value: &mut i64,
    range: std::ops::RangeInclusive<i64>,
    signed: bool,
    name: &str,
) -> egui::Response {
    let response = ui.add_sized(
        [96.0, 26.0],
        egui::DragValue::new(value)
            .range(range)
            .speed(1.0)
            .suffix(" px")
            .custom_formatter(move |value, _| {
                if signed && value > 0.0 {
                    format!("+{value}")
                } else {
                    format!("{value}")
                }
            }),
    );
    let shown = *value as f64;
    response.widget_info(|| {
        let mut info = egui::WidgetInfo::drag_value(true, shown);
        info.label = Some(name.to_owned());
        info
    });
    response
}

/// An item of a drop-down list that closes the list when chosen. egui closes
/// a popup only on a pointer click, and left it open after Space or Enter.
pub fn choice<T: PartialEq>(
    ui: &mut Ui,
    current: &mut T,
    value: T,
    text: impl Into<egui::WidgetText>,
) -> Response {
    let response = ui.selectable_value(current, value, text);
    if response.clicked() {
        ui.close();
    }
    response
}

/// How much a button stands out: the one that ends a dialog is filled with
/// the accent, the others sit on the panel.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Emphasis {
    Primary,
    Secondary,
}

/// A text button, 28 points tall, in the dialogs' style.
pub fn button(ui: &mut Ui, text: &str, emphasis: Emphasis, enabled: bool) -> Response {
    let galley = ui.painter().layout_no_wrap(
        text.to_owned(),
        FontId::proportional(theme::BODY),
        theme::text(),
    );
    let size = egui::vec2((galley.size().x + 28.0).max(72.0), 28.0);
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(size, sense);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, text));
    let hovered = enabled && response.hovered();
    let (fill, edge, ink) = match emphasis {
        _ if !enabled => (theme::control(), theme::border(), theme::disabled()),
        Emphasis::Primary if hovered => (
            theme::accent_pressed(),
            theme::accent_pressed(),
            theme::accent_text(),
        ),
        Emphasis::Primary => (theme::accent(), theme::accent(), theme::accent_text()),
        Emphasis::Secondary if hovered => (theme::hover(), theme::disabled(), theme::text()),
        Emphasis::Secondary => (theme::control(), theme::border(), theme::text()),
    };
    let painter = ui.painter();
    painter.rect(
        rect,
        CornerRadius::same(6),
        fill,
        Stroke::new(1.0, edge),
        StrokeKind::Inside,
    );
    if response.has_focus() {
        painter.rect_stroke(
            rect.expand(2.0),
            CornerRadius::same(8),
            Stroke::new(1.0, theme::accent_ink()),
            StrokeKind::Outside,
        );
    }
    let text_at = rect.center() - galley.size() / 2.0;
    painter.galley(text_at, galley, ink);
    response
}

/// An on-off switch named `name` for screen readers; the row beside it
/// says what it turns on.
pub fn switch(ui: &mut Ui, on: &mut bool, name: &str) -> Response {
    let (rect, mut response) = ui.allocate_exact_size(egui::vec2(36.0, 20.0), Sense::click());
    if response.clicked() {
        *on = !*on;
        response.mark_changed();
    }
    response
        .widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Checkbox, true, *on, name));
    let shift = ui.ctx().animate_bool(response.id, *on);
    let (track, edge, knob) = if *on {
        (theme::accent(), theme::accent_ink(), theme::accent_text())
    } else if response.hovered() {
        (theme::control(), theme::disabled(), theme::text())
    } else {
        (theme::base(), theme::border(), theme::muted())
    };
    let painter = ui.painter();
    painter.rect(
        rect,
        CornerRadius::same(10),
        track,
        Stroke::new(1.0, edge),
        StrokeKind::Inside,
    );
    let x = egui::lerp((rect.left() + 10.0)..=(rect.right() - 10.0), shift);
    painter.circle_filled(egui::pos2(x, rect.center().y), 6.0, knob);
    if response.has_focus() {
        painter.rect_stroke(
            rect.expand(2.0),
            CornerRadius::same(12),
            Stroke::new(1.0, theme::accent_ink()),
            StrokeKind::Outside,
        );
    }
    response
}

/// A group of settings on a raised surface.
pub fn card<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    egui::Frame::new()
        .fill(theme::base())
        .stroke(Stroke::new(1.0, theme::border()))
        .corner_radius(CornerRadius::same(8))
        .inner_margin(egui::Margin::symmetric(14, 6))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        })
        .inner
}

/// The width a settings row keeps for its control.
pub const ROW_CONTROL: f32 = 210.0;

/// One setting: its name and, quieter under it, what it does, with
/// `control` at the right. A thin line parts it from the row before unless
/// it is the first.
pub fn setting_row<R>(
    ui: &mut Ui,
    first: bool,
    title: &str,
    description: Option<&str>,
    control: impl FnOnce(&mut Ui) -> R,
) -> R {
    if !first {
        let y = ui.cursor().top();
        let x = ui.max_rect().x_range();
        ui.painter()
            .hline(x, y, Stroke::new(1.0, theme::border().gamma_multiply(0.6)));
    }
    let width = ui.available_width();
    ui.horizontal(|ui| {
        ui.set_min_height(44.0);
        ui.allocate_ui_with_layout(
            egui::vec2(width - ROW_CONTROL - 12.0, 0.0),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                ui.add_space(8.0);
                ui.label(egui::RichText::new(title).color(theme::text()));
                if let Some(description) = description {
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(description)
                                .size(theme::SMALL)
                                .color(theme::muted()),
                        )
                        .wrap(),
                    );
                }
                ui.add_space(8.0);
            },
        );
        ui.allocate_ui_with_layout(
            egui::vec2(ROW_CONTROL, 44.0),
            egui::Layout::right_to_left(egui::Align::Center),
            control,
        )
        .inner
    })
    .inner
}

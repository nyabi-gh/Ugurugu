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
    painter.rect_filled(rect, CornerRadius::same(theme::radius::CONTROL), fill);
    // Outside, as an amber fill would hide a ring in the accent inside it.
    if response.has_focus() {
        focus_ring(painter, rect, theme::radius::CONTROL);
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

/// The ring keyboard focus draws 2 points outside a control whose corners
/// are `radius`.
pub fn focus_ring(painter: &egui::Painter, rect: Rect, radius: u8) {
    painter.rect_stroke(
        rect.expand(2.0),
        CornerRadius::same(radius + 2),
        Stroke::new(1.0, theme::accent_ink()),
        StrokeKind::Outside,
    );
}

/// A colour chip and its hex code in a control-sized box, which opens a
/// picker shown by the caller while `open`; the alpha is written only when
/// not opaque.
pub fn color_swatch(ui: &mut Ui, color: Color32, label: &str, open: bool) -> Response {
    let hex = hex_code(color);
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(120.0, theme::CONTROL_HEIGHT), Sense::click());
    let enabled = ui.is_enabled();
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, format!("{label} {hex}"))
    });
    let painter = ui.painter();
    let edge = if enabled && (response.hovered() || open) {
        theme::disabled()
    } else {
        theme::border()
    };
    painter.rect(
        rect,
        CornerRadius::same(theme::radius::CONTROL),
        theme::control(),
        Stroke::new(1.0, edge),
        StrokeKind::Inside,
    );
    let chip = Rect::from_center_size(
        egui::pos2(rect.left() + 14.0, rect.center().y),
        egui::vec2(18.0, 18.0),
    );
    let chip_color = if enabled {
        color
    } else {
        color.gamma_multiply(0.4)
    };
    painter.rect_filled(chip, CornerRadius::same(theme::radius::SMALL), chip_color);
    let ink = if enabled {
        theme::text()
    } else {
        theme::disabled()
    };
    let galley = painter.layout_no_wrap(hex, egui::FontId::monospace(theme::BODY), ink);
    painter.galley(
        egui::pos2(chip.right() + 8.0, rect.center().y - galley.size().y / 2.0),
        galley,
        ink,
    );
    if response.has_focus() {
        focus_ring(painter, rect, theme::radius::CONTROL);
    }
    response
}

/// `#RRGGBB`, or `#RRGGBBAA` when not opaque.
fn hex_code(color: Color32) -> String {
    let [r, g, b, a] = color.to_srgba_unmultiplied();
    if a == u8::MAX {
        format!("#{r:02X}{g:02X}{b:02X}")
    } else {
        format!("#{r:02X}{g:02X}{b:02X}{a:02X}")
    }
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
        ui.painter().rect_filled(
            rect,
            CornerRadius::same(theme::radius::CONTROL),
            theme::hover(),
        );
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
    if response.has_focus() {
        focus_ring(ui.painter(), rect, theme::radius::CONTROL);
    }
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
    ui.painter()
        .rect_filled(rect, CornerRadius::same(theme::radius::CONTROL), fill);
    icons::paint(
        ui.painter(),
        Rect::from_center_size(rect.center(), Vec2::splat(glyph_size)),
        glyph,
        ink,
        0.0,
    );
    if response.has_focus() {
        focus_ring(ui.painter(), rect, theme::radius::CONTROL);
    }
    response.on_hover_text(tip)
}

/// A flat button with a small glyph and its text beside it, as 2.2.13's
/// status bar and animation bar buttons.
pub fn text_icon_button(ui: &mut Ui, glyph: Glyph, text: &str, tip: &str) -> Response {
    text_icon_button_as_wide(ui, glyph, text, tip, &[])
}

/// `text_icon_button` as wide as with the widest of `others` too, for a
/// button whose text changes in place.
pub fn text_icon_button_as_wide(
    ui: &mut Ui,
    glyph: Glyph,
    text: &str,
    tip: &str,
    others: &[&str],
) -> Response {
    let measure = |text: &str| {
        ui.painter()
            .layout_no_wrap(
                text.to_owned(),
                FontId::proportional(theme::BODY),
                theme::text(),
            )
            .size()
            .x
    };
    let widest = others
        .iter()
        .map(|other| measure(other))
        .fold(0.0, f32::max);
    let galley = ui.painter().layout_no_wrap(
        text.to_owned(),
        FontId::proportional(theme::BODY),
        theme::text(),
    );
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(6.0 + 16.0 + 4.0 + galley.size().x.max(widest) + 6.0, 24.0),
        Sense::click(),
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, text));
    if response.hovered() {
        ui.painter().rect_filled(
            rect,
            CornerRadius::same(theme::radius::CONTROL),
            theme::hover(),
        );
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
    if response.has_focus() {
        focus_ring(ui.painter(), rect, theme::radius::CONTROL);
    }
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

/// A heading over a group of panel rows, set apart from their names.
pub fn panel_heading(ui: &mut Ui, text: &str) -> Response {
    ui.label(
        egui::RichText::new(text)
            .size(theme::SMALL)
            .strong()
            .color(theme::text()),
    )
}

/// A panel row: `label` in a column of the same width on every row, so the
/// controls of rows one under another start in line, cut short with the
/// whole name on hover if it does not fit; `control` fills the rest from
/// the right.
pub fn form_row<R>(ui: &mut Ui, label: &str, control: impl FnOnce(&mut Ui) -> R) -> R {
    let width = (ui.available_width() * 0.36).clamp(56.0, 112.0);
    ui.horizontal(|ui| {
        ui.allocate_ui_with_layout(
            egui::vec2(width, theme::CONTROL_HEIGHT),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                ui.set_min_width(width);
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(label)
                            .size(theme::SMALL)
                            .color(theme::muted()),
                    )
                    .truncate(),
                );
            },
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), control)
            .inner
    })
    .inner
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
                [width, theme::CONTROL_HEIGHT],
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

/// A square check box with a tick and an edge in the accent when checked,
/// followed by `text` if there is any.
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
        (true, _) => (theme::control(), theme::accent_ink()),
        (false, true) => (theme::control(), theme::disabled()),
        (false, false) => (theme::base(), theme::border()),
    };
    painter.rect(
        square,
        CornerRadius::same(theme::radius::SMALL),
        fill,
        Stroke::new(1.0, edge),
        StrokeKind::Inside,
    );
    if *checked {
        let at = |x: f32, y: f32| square.min + egui::vec2(x, y);
        painter.add(egui::Shape::line(
            vec![at(4.0, 8.4), at(7.0, 11.4), at(12.2, 5.2)],
            Stroke::new(2.0, theme::accent_ink()),
        ));
    }
    if response.has_focus() {
        focus_ring(painter, square, theme::radius::SMALL);
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

/// A caption on the left and a switch on the right, as 2.2.13's option rows
/// with the settings' switch.
pub fn check_row(ui: &mut Ui, label: &str, name: &str, checked: &mut bool) -> Response {
    ui.horizontal(|ui| {
        field_label(ui, label);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            switch(ui, checked, name)
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
        [96.0, theme::CONTROL_HEIGHT],
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

/// An item of a drop-down list, drawn as a menu item checked while it is
/// the current one, that closes the list when chosen. egui closes a popup
/// only on a pointer click, and left it open after Space or Enter.
pub fn choice<T: PartialEq>(ui: &mut Ui, current: &mut T, value: T, text: &str) -> Response {
    let width = menu_item_width(ui, text, "").max(ui.available_width());
    let mut response = menu_item(ui, width, text, "", Some(*current == value), true);
    if response.clicked() {
        if *current != value {
            *current = value;
            response.mark_changed();
        }
        ui.close();
    }
    response
}

/// One of a few choices side by side in a well, `columns` to a row: the
/// chosen one raised with an edge in the accent, as the brush tiles and
/// shape options are. Changed when another is chosen.
pub fn segmented<T: PartialEq + Copy>(
    ui: &mut Ui,
    current: &mut T,
    options: &[(T, &str)],
    columns: usize,
) -> Response {
    const INSET: f32 = 2.0;
    const SEGMENT: f32 = theme::CONTROL_HEIGHT - 2.0 * INSET;
    let columns = columns.clamp(1, options.len().max(1));
    let rows = options.len().div_ceil(columns);
    let width = ui.available_width();
    let height = rows as f32 * (SEGMENT + INSET) + INSET;
    let (rect, mut response) = ui.allocate_exact_size(egui::vec2(width, height), Sense::hover());
    let painter = ui.painter().clone();
    painter.rect(
        rect,
        CornerRadius::same(theme::radius::CONTROL),
        theme::base(),
        Stroke::new(1.0, theme::border()),
        StrokeKind::Inside,
    );
    let cell = (width - INSET - columns as f32 * INSET) / columns as f32;
    let enabled = ui.is_enabled();
    for (index, &(value, text)) in options.iter().enumerate() {
        let (row, column) = (index / columns, index % columns);
        let segment = Rect::from_min_size(
            rect.min
                + egui::vec2(
                    INSET + column as f32 * (cell + INSET),
                    INSET + row as f32 * (SEGMENT + INSET),
                ),
            egui::vec2(cell, SEGMENT),
        );
        let each = ui.interact(segment, response.id.with(index), Sense::click());
        let selected = *current == value;
        each.widget_info(|| {
            egui::WidgetInfo::selected(egui::WidgetType::RadioButton, enabled, selected, text)
        });
        if each.clicked() && !selected {
            *current = value;
            response.mark_changed();
        }
        let ink = match (enabled, selected) {
            (false, _) => theme::disabled(),
            (true, true) => theme::text(),
            (true, false) => theme::muted(),
        };
        let radius = CornerRadius::same(theme::radius::CONTROL - INSET as u8);
        if selected {
            let edge = if enabled {
                theme::accent_ink()
            } else {
                theme::border()
            };
            painter.rect(
                segment,
                radius,
                theme::control(),
                Stroke::new(1.0, edge),
                StrokeKind::Inside,
            );
        } else if each.hovered() {
            painter.rect_filled(segment, radius, theme::hover());
        }
        if each.has_focus() {
            focus_ring(&painter, segment, theme::radius::SMALL);
        }
        let galley =
            painter.layout_no_wrap(text.to_owned(), FontId::proportional(theme::BODY), ink);
        painter.galley(segment.center() - galley.size() / 2.0, galley, ink);
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
    // Drawn in whatever colour the state below gives it.
    let galley = ui.painter().layout_no_wrap(
        text.to_owned(),
        FontId::proportional(theme::BODY),
        Color32::PLACEHOLDER,
    );
    let size = egui::vec2((galley.size().x + 28.0).max(72.0), theme::CONTROL_HEIGHT);
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
        CornerRadius::same(theme::radius::CONTROL),
        fill,
        Stroke::new(1.0, edge),
        StrokeKind::Inside,
    );
    if response.has_focus() {
        focus_ring(painter, rect, theme::radius::CONTROL);
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
        CornerRadius::same(rect.height() as u8 / 2),
        track,
        Stroke::new(1.0, edge),
        StrokeKind::Inside,
    );
    let x = egui::lerp((rect.left() + 10.0)..=(rect.right() - 10.0), shift);
    painter.circle_filled(egui::pos2(x, rect.center().y), 6.0, knob);
    if response.has_focus() {
        focus_ring(painter, rect, rect.height() as u8 / 2);
    }
    response
}

/// A group of settings on a raised surface.
pub fn card<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    egui::Frame::new()
        .fill(theme::base())
        .stroke(Stroke::new(1.0, theme::border()))
        .corner_radius(CornerRadius::same(theme::radius::SURFACE))
        .inner_margin(egui::Margin {
            left: 14,
            right: 20,
            top: 0,
            bottom: 0,
        })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        })
        .inner
}

/// The width a settings row keeps for its control.
pub const ROW_CONTROL: f32 = 210.0;

/// One setting: its name and, quieter under it, what it does, in a column
/// on the left, and `control` at the right in a column of its own, both
/// centred on the row. A thin line parts it from the row before unless it
/// is the first.
pub fn setting_row<R>(
    ui: &mut Ui,
    first: bool,
    title: &str,
    description: Option<&str>,
    control: impl FnOnce(&mut Ui) -> R,
) -> R {
    row_with(
        ui,
        first,
        title,
        description,
        [ROW_CONTROL, theme::CONTROL_HEIGHT],
        control,
    )
}

/// A setting whose control is `size` and taller than the others, which the
/// row grows to hold.
pub fn tall_setting_row<R>(
    ui: &mut Ui,
    first: bool,
    title: &str,
    size: egui::Vec2,
    control: impl FnOnce(&mut Ui) -> R,
) -> R {
    row_with(ui, first, title, None, size.into(), control)
}

/// A setting turned on or off by a switch alone, which leaves the name and
/// what it does the rest of the row.
pub fn switch_row(
    ui: &mut Ui,
    first: bool,
    title: &str,
    description: Option<&str>,
    on: &mut bool,
    enabled: bool,
) -> Response {
    row_with(
        ui,
        first,
        title,
        description,
        [40.0, theme::CONTROL_HEIGHT],
        |ui| ui.add_enabled_ui(enabled, |ui| switch(ui, on, title)).inner,
    )
}

/// A settings row whose control takes `control_width` at the right.
fn row_with<R>(
    ui: &mut Ui,
    first: bool,
    title: &str,
    description: Option<&str>,
    [control_width, control_height]: [f32; 2],
    control: impl FnOnce(&mut Ui) -> R,
) -> R {
    const PADDING: f32 = theme::space::L;
    let width = ui.available_width();
    let text_width = width - control_width - 16.0;
    // Measured as the labels below lay them out, to centre them.
    let measure = |text: &str, size: f32| {
        ui.painter()
            .layout(
                text.to_owned(),
                FontId::proportional(size),
                Color32::PLACEHOLDER,
                text_width,
            )
            .size()
            .y
    };
    let text_height = measure(title, theme::BODY)
        + description.map_or(0.0, |description| 2.0 + measure(description, theme::SMALL));
    let height = text_height.max(control_height) + 2.0 * PADDING;
    if !first {
        // Up against the row before, or egui's room after it would widen
        // the room under that row's text and not over it.
        ui.add_space(-ui.spacing().item_spacing.y);
    }
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), Sense::hover());
    if !first {
        ui.painter().hline(rect.x_range(), rect.top(), rule());
    }
    let text_rect = Rect::from_min_size(
        egui::pos2(rect.left(), rect.center().y - text_height / 2.0),
        egui::vec2(text_width, text_height),
    );
    ui.scope_builder(
        egui::UiBuilder::new()
            .max_rect(text_rect)
            .layout(egui::Layout::top_down(egui::Align::Min)),
        |ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            ui.add(egui::Label::new(egui::RichText::new(title).color(theme::text())).wrap());
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
        },
    );
    let control_rect = Rect::from_min_max(
        egui::pos2(rect.right() - control_width, rect.top()),
        rect.right_bottom(),
    );
    ui.scope_builder(
        egui::UiBuilder::new()
            .max_rect(control_rect)
            .layout(egui::Layout::right_to_left(egui::Align::Center)),
        control,
    )
    .inner
}

/// Between a dialog's edge and what is in it.
pub const DIALOG_MARGIN: i8 = theme::space::XXL as i8;

/// A faint line between parts of a dialog or rows of a card.
pub fn rule() -> Stroke {
    Stroke::new(1.0, theme::border().gamma_multiply(0.6))
}

/// A dialog `width` points wide inside its margin: the title, a quieter line
/// on what it does, then what `add` puts in, which ends with
/// `dialog_buttons`.
pub fn dialog<R>(
    ctx: &egui::Context,
    id: &str,
    width: f32,
    title: &str,
    description: Option<&str>,
    add: impl FnOnce(&mut Ui) -> R,
) -> egui::ModalResponse<R> {
    let frame = egui::Frame::popup(&ctx.global_style()).inner_margin(DIALOG_MARGIN);
    egui::Modal::new(egui::Id::new(id))
        .frame(frame)
        .show(ctx, |ui| {
            ui.set_width(width);
            ui.label(
                egui::RichText::new(title)
                    .size(theme::TITLE)
                    .strong()
                    .color(theme::text()),
            );
            if let Some(description) = description {
                ui.add_space(theme::space::XXS);
                ui.add(
                    egui::Label::new(egui::RichText::new(description).color(theme::muted())).wrap(),
                );
            }
            space_after(ui, theme::space::XL);
            add(ui)
        })
}

/// The buttons at the foot of a dialog, after a faint line, at the right:
/// `add` adds them from the right, the one that confirms first.
pub fn dialog_buttons<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    divider(ui, theme::space::XL);
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), add)
        .inner
}

/// `amount` points of room under the widget just added: egui leaves some
/// after every widget, and an end of a card or a line under a widget would
/// otherwise have that much more room than the start over it.
pub fn space_after(ui: &mut Ui, amount: f32) {
    ui.add_space(amount - ui.spacing().item_spacing.y);
}

/// A faint line across `ui` with `gap` points on either side of it, after
/// a widget: egui's room after that widget counts toward the gap above.
pub fn divider(ui: &mut Ui, gap: f32) {
    space_after(ui, gap);
    let y = ui.cursor().top();
    ui.painter().hline(ui.max_rect().x_range(), y, rule());
    ui.add_space(gap);
}

/// A small heading over a card, with room above it unless it is the first.
pub fn section(ui: &mut Ui, text: &str, first: bool) {
    if !first {
        space_after(ui, theme::space::XL);
    }
    ui.label(
        egui::RichText::new(text)
            .size(theme::SMALL)
            .strong()
            .color(theme::muted()),
    );
    ui.add_space(theme::space::XXS);
}

const MENU_CHECK: f32 = 22.0;
const MENU_PADDING: f32 = theme::space::M;
/// Between an item's text and its shortcut.
const MENU_GAP: f32 = 28.0;

/// The narrowest a menu is, so short ones do not look cramped.
pub const MENU_WIDTH: f32 = 200.0;

/// How wide `menu_item` needs to be for `text` and `shortcut`.
pub fn menu_item_width(ui: &Ui, text: &str, shortcut: &str) -> f32 {
    let measure = |text: &str| {
        ui.painter()
            .layout_no_wrap(
                text.to_owned(),
                FontId::proportional(theme::BODY),
                Color32::PLACEHOLDER,
            )
            .size()
            .x
    };
    let key = if shortcut.is_empty() {
        0.0
    } else {
        MENU_GAP + measure(shortcut)
    };
    MENU_PADDING + MENU_CHECK + measure(text) + key + MENU_PADDING * 1.5
}

/// A menu item `width` wide, the widest of its menu's: a check mark in the
/// accent when `checked` is on, the text, and the shortcut quieter at the
/// right. The pointer or the focus on it raises it.
pub fn menu_item(
    ui: &mut Ui,
    width: f32,
    text: &str,
    shortcut: &str,
    checked: Option<bool>,
    enabled: bool,
) -> Response {
    const CHECK: f32 = MENU_CHECK;
    const PADDING: f32 = MENU_PADDING;
    let ink = if enabled {
        theme::text()
    } else {
        theme::disabled()
    };
    let font = FontId::proportional(theme::BODY);
    let label = ui
        .painter()
        .layout_no_wrap(text.to_owned(), font.clone(), ink);
    let key = (!shortcut.is_empty()).then(|| {
        ui.painter()
            .layout_no_wrap(shortcut.to_owned(), font, theme::muted())
    });
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, theme::CONTROL_HEIGHT), sense);
    response.widget_info(|| match checked {
        Some(on) => {
            egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, enabled, on, text)
        }
        None => egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, text),
    });
    let painter = ui.painter();
    if enabled && (response.hovered() || response.has_focus()) {
        painter.rect_filled(
            rect,
            CornerRadius::same(theme::radius::CONTROL),
            theme::hover(),
        );
    }
    if checked == Some(true) {
        let at = |x: f32, y: f32| egui::pos2(rect.left() + PADDING + x, rect.center().y + y);
        painter.add(egui::Shape::line(
            vec![at(2.0, 0.0), at(6.0, 4.0), at(13.0, -4.5)],
            Stroke::new(1.8, if enabled { theme::accent_ink() } else { ink }),
        ));
    }
    let y = rect.center().y - label.size().y / 2.0;
    painter.galley(egui::pos2(rect.left() + PADDING + CHECK, y), label, ink);
    if let Some(key) = key {
        let at = egui::pos2(rect.right() - PADDING * 1.5 - key.size().x, y);
        painter.galley(at, key, theme::muted());
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_colour_code_names_the_alpha_only_when_translucent() {
        assert_eq!(hex_code(Color32::from_rgb(255, 201, 74)), "#FFC94A");
        assert_eq!(
            hex_code(Color32::from_rgba_unmultiplied(16, 32, 48, 128)),
            "#10203080"
        );
    }

    /// Runs one frame of `add` with `events`, returning what it returned.
    fn frame<R>(
        ctx: &egui::Context,
        events: Vec<egui::Event>,
        mut add: impl FnMut(&mut Ui) -> R,
    ) -> R {
        let input = egui::RawInput {
            events,
            ..Default::default()
        };
        let mut result = None;
        let mut output = ctx.run_ui(input, |ui| result = Some(add(ui)));
        // No renderer takes the font atlas.
        output.textures_delta.clear();
        result.unwrap()
    }

    fn press(at: egui::Pos2, pressed: bool) -> egui::Event {
        egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        }
    }

    #[test]
    fn settings_rows_meet_so_each_is_centred_between_its_lines() {
        let ctx = egui::Context::default();
        let show = |ui: &mut Ui| {
            ui.set_width(400.0);
            card(ui, |ui| {
                [true, false, false].map(|first| {
                    setting_row(ui, first, "name", Some("what it does"), |ui| {
                        ui.label("control");
                        ui.max_rect()
                    })
                })
            })
        };
        frame(&ctx, Vec::new(), show);
        let rows = frame(&ctx, Vec::new(), show);
        assert_eq!(rows[1].top(), rows[0].bottom());
        assert_eq!(rows[2].top(), rows[1].bottom());
    }

    #[test]
    fn a_divider_has_as_much_room_over_it_as_under_it() {
        let ctx = egui::Context::default();
        let (over, under) = frame(&ctx, Vec::new(), |ui| {
            let over = ui.label("over").rect;
            divider(ui, theme::space::XL);
            (over, ui.label("under").rect)
        });
        assert_eq!(under.top() - over.bottom(), 2.0 * theme::space::XL);
    }

    #[test]
    fn a_segment_on_a_later_row_is_chosen_where_it_is_drawn() {
        let ctx = egui::Context::default();
        let options = [(1, "one"), (2, "two"), (3, "three")];
        let mut current = 1;
        let show = |ui: &mut Ui, current: &mut i32| {
            ui.set_width(200.0);
            segmented(ui, current, &options, 2)
        };
        let well = frame(&ctx, Vec::new(), |ui| show(ui, &mut current)).rect;
        assert_eq!(well.height(), 2.0 * theme::CONTROL_HEIGHT - 2.0);
        // The third choice is first on the second row.
        let at = egui::pos2(well.left() + well.width() / 4.0, well.bottom() - 6.0);
        frame(
            &ctx,
            vec![egui::Event::PointerMoved(at), press(at, true)],
            |ui| show(ui, &mut current),
        );
        let changed = frame(&ctx, vec![press(at, false)], |ui| show(ui, &mut current)).changed();
        assert!(changed);
        assert_eq!(current, 3);
    }
}

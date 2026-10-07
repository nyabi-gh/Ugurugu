// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! Colours, type and widget style, after 2.2.13's `Theme.cpp`: a dark,
//! neutral interface with one amber accent, set in Pretendard JP.

use std::sync::Arc;

use egui::{Color32, CornerRadius, FontFamily, FontId, Stroke, TextStyle};

const fn rgb(value: u32) -> Color32 {
    Color32::from_rgb((value >> 16) as u8, (value >> 8) as u8, value as u8)
}

/// Menu, tool bars and the animation bar.
pub const CHROME: Color32 = rgb(0x20_22_26);
pub const STATUS: Color32 = rgb(0x1B_1D_21);
/// Around the paper, and inside input fields.
pub const BASE: Color32 = rgb(0x2A_2C_30);
pub const PANEL: Color32 = rgb(0x24_26_2B);
pub const CONTROL: Color32 = rgb(0x34_37_3D);
pub const HOVER: Color32 = rgb(0x2E_31_38);
pub const BORDER: Color32 = rgb(0x3F_43_4B);
pub const TEXT: Color32 = rgb(0xE8_E8_EA);
pub const MUTED: Color32 = rgb(0x9A_A0_A8);
pub const DISABLED: Color32 = rgb(0x6A_6F_78);
pub const ACCENT: Color32 = rgb(0xFF_C9_4A);
/// `ACCENT` 12% darker, as Qt's `darker(112)`.
pub const ACCENT_PRESSED: Color32 = rgb(0xE4_B3_42);
/// Text on the accent, dark because the amber is light.
pub const ACCENT_TEXT: Color32 = rgb(0x18_18_1A);

/// Body text, the size of the Windows interface font Qt uses (9 pt).
pub const BODY: f32 = 12.0;
/// Field labels and captions; 2.2.13 keeps them at 11 px or more so Hangul
/// and kana stay legible.
pub const SMALL: f32 = 11.0;

/// Sets the font, text sizes and widget style on `ctx`.
pub fn apply(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "Pretendard JP".to_owned(),
        Arc::new(egui::FontData::from_static(include_bytes!(
            "../../../resources/fonts/PretendardJP-Medium.otf"
        ))),
    );
    // In front of egui's own fonts, which stay as fallbacks for symbols.
    for family in [FontFamily::Proportional, FontFamily::Monospace] {
        fonts
            .families
            .entry(family)
            .or_default()
            .insert(0, "Pretendard JP".to_owned());
    }
    ctx.set_fonts(fonts);

    ctx.all_styles_mut(|style| {
        style.text_styles = [
            (TextStyle::Heading, FontId::proportional(BODY)),
            (TextStyle::Body, FontId::proportional(BODY)),
            (TextStyle::Button, FontId::proportional(BODY)),
            (TextStyle::Small, FontId::proportional(SMALL)),
            (TextStyle::Monospace, FontId::monospace(BODY)),
        ]
        .into();

        let spacing = &mut style.spacing;
        spacing.item_spacing = egui::vec2(6.0, 6.0);
        spacing.button_padding = egui::vec2(8.0, 3.0);
        spacing.interact_size = egui::vec2(40.0, 26.0);
        spacing.indent = 14.0;
        spacing.menu_margin = egui::Margin::same(4);
        spacing.window_margin = egui::Margin::same(8);
        spacing.slider_rail_height = 5.0;
        spacing.scroll = egui::style::ScrollStyle {
            floating: false,
            bar_width: 6.0,
            handle_min_length: 24.0,
            bar_inner_margin: 2.0,
            bar_outer_margin: 2.0,
            ..egui::style::ScrollStyle::solid()
        };

        let visuals = &mut style.visuals;
        *visuals = egui::Visuals::dark();
        visuals.panel_fill = PANEL;
        visuals.window_fill = PANEL;
        visuals.window_stroke = Stroke::new(1.0, BORDER);
        visuals.window_corner_radius = CornerRadius::same(8);
        visuals.menu_corner_radius = CornerRadius::same(6);
        visuals.extreme_bg_color = BASE;
        visuals.faint_bg_color = HOVER;
        visuals.code_bg_color = BASE;
        visuals.hyperlink_color = ACCENT;
        visuals.selection.bg_fill = ACCENT;
        visuals.selection.stroke = Stroke::new(1.0, ACCENT_TEXT);
        visuals.text_cursor.stroke = Stroke::new(2.0, ACCENT);
        visuals.slider_trailing_fill = true;
        visuals.handle_shape = egui::style::HandleShape::Circle;
        visuals.popup_shadow = egui::Shadow {
            offset: [0, 4],
            blur: 12,
            spread: 0,
            color: Color32::from_black_alpha(90),
        };
        visuals.window_shadow = visuals.popup_shadow;

        let widgets = &mut visuals.widgets;
        let radius = CornerRadius::same(6);
        widgets.noninteractive.bg_fill = PANEL;
        widgets.noninteractive.weak_bg_fill = PANEL;
        widgets.noninteractive.bg_stroke = Stroke::new(1.0, BORDER);
        widgets.noninteractive.fg_stroke = Stroke::new(1.0, TEXT);
        widgets.noninteractive.corner_radius = radius;
        widgets.inactive.bg_fill = CONTROL;
        widgets.inactive.weak_bg_fill = CONTROL;
        widgets.inactive.bg_stroke = Stroke::new(1.0, BORDER);
        widgets.inactive.fg_stroke = Stroke::new(1.0, TEXT);
        widgets.inactive.corner_radius = radius;
        widgets.hovered.bg_fill = rgb(0x3A_3E_45);
        widgets.hovered.weak_bg_fill = rgb(0x3A_3E_45);
        widgets.hovered.bg_stroke = Stroke::new(1.0, DISABLED);
        widgets.hovered.fg_stroke = Stroke::new(1.0, TEXT);
        widgets.hovered.corner_radius = radius;
        widgets.hovered.expansion = 0.0;
        widgets.active.bg_fill = CONTROL;
        widgets.active.weak_bg_fill = CONTROL;
        widgets.active.bg_stroke = Stroke::new(1.0, ACCENT);
        widgets.active.fg_stroke = Stroke::new(1.0, TEXT);
        widgets.active.corner_radius = radius;
        widgets.active.expansion = 0.0;
        widgets.open = widgets.active;
    });
}

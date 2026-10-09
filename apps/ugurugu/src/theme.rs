// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! Colours, type and widget style, after 2.2.13's `Theme.cpp`: a dark,
//! neutral interface with one amber accent, set in Pretendard JP.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

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
/// The accent unless the user chose another.
pub const DEFAULT_ACCENT: [u8; 3] = [0xFF, 0xC9, 0x4A];

/// The accent as 0xRRGGBB; read on every frame, set from the settings.
static ACCENT: AtomicU32 = AtomicU32::new(0xFF_C9_4A);

pub fn accent() -> Color32 {
    rgb(ACCENT.load(Ordering::Relaxed))
}

/// `accent()` 12% darker, as Qt's `darker(112)`.
pub fn accent_pressed() -> Color32 {
    let [r, g, b, _] = accent().to_array();
    let darker = |channel: u8| (f32::from(channel) * 100.0 / 112.0).round() as u8;
    Color32::from_rgb(darker(r), darker(g), darker(b))
}

/// Text on the accent: dark on a light accent, light on a dark one, by
/// 2.2.13's luminance threshold.
pub fn accent_text() -> Color32 {
    let [r, g, b, _] = accent().to_array();
    let luminance = (0.2126 * f32::from(r) + 0.7152 * f32::from(g) + 0.0722 * f32::from(b)) / 255.0;
    if luminance >= 0.52 {
        rgb(0x18_18_1A)
    } else {
        rgb(0xFA_FA_FB)
    }
}

/// Uses `color` as the accent, or the default for `None`, and restyles `ctx`.
pub fn set_accent(ctx: &egui::Context, color: Option<[u8; 3]>) {
    let [r, g, b] = color.unwrap_or(DEFAULT_ACCENT);
    ACCENT.store(u32::from_be_bytes([0, r, g, b]), Ordering::Relaxed);
    ctx.all_styles_mut(style_accent);
}

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
        widgets.active.fg_stroke = Stroke::new(1.0, TEXT);
        widgets.active.corner_radius = radius;
        widgets.active.expansion = 0.0;
        style_accent(style);
    });
}

/// The parts of the style drawn in the accent.
fn style_accent(style: &mut egui::Style) {
    let visuals = &mut style.visuals;
    visuals.hyperlink_color = accent();
    visuals.selection.bg_fill = accent();
    visuals.selection.stroke = Stroke::new(1.0, accent_text());
    visuals.text_cursor.stroke = Stroke::new(2.0, accent());
    visuals.widgets.active.bg_stroke = Stroke::new(1.0, accent());
    visuals.widgets.open = visuals.widgets.active;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_accent_gives_2_2_13s_colours() {
        let ctx = egui::Context::default();
        set_accent(&ctx, None);
        assert_eq!(accent(), rgb(0xFF_C9_4A));
        assert_eq!(accent_pressed(), rgb(0xE4_B3_42));
        assert_eq!(accent_text(), rgb(0x18_18_1A));
        set_accent(&ctx, Some([0x20, 0x40, 0xA0]));
        assert_eq!(accent_text(), rgb(0xFA_FA_FB));
        assert_eq!(
            ctx.global_style().visuals.selection.bg_fill,
            rgb(0x20_40_A0)
        );
        set_accent(&ctx, None);
    }
}

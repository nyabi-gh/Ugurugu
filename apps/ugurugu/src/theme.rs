// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! Colours, type and widget style, after 2.2.13's `Theme.cpp`: a neutral
//! interface, dark or light, with one amber accent, set in Pretendard JP.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use egui::{Color32, CornerRadius, FontFamily, FontId, Stroke, TextStyle, Theme, ThemePreference};

const fn rgb(value: u32) -> Color32 {
    Color32::from_rgb((value >> 16) as u8, (value >> 8) as u8, value as u8)
}

/// Which palette the interface is drawn in, as the settings keep it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    /// 2.2.13's.
    #[default]
    Dark,
    Light,
    /// Light or dark as Windows's app mode is.
    System,
}

impl Mode {
    pub const ALL: [Self; 3] = [Self::Dark, Self::Light, Self::System];

    /// Its name in the settings file.
    pub fn code(self) -> &'static str {
        match self {
            Self::Dark => "dark",
            Self::Light => "light",
            Self::System => "system",
        }
    }

    pub fn from_code(code: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|each| each.code() == code)
    }

    fn preference(self) -> ThemePreference {
        match self {
            Self::Dark => ThemePreference::Dark,
            Self::Light => ThemePreference::Light,
            Self::System => ThemePreference::System,
        }
    }
}

struct Palette {
    /// Menu, tool bars and the animation bar.
    chrome: Color32,
    status: Color32,
    /// Inside input fields and other wells.
    base: Color32,
    panel: Color32,
    control: Color32,
    hover: Color32,
    /// A control under the pointer.
    control_hover: Color32,
    border: Color32,
    text: Color32,
    muted: Color32,
    disabled: Color32,
    /// Around the document, opaque straight RGBA.
    workspace: [u8; 4],
    shadow: Color32,
}

const DARK: Palette = Palette {
    chrome: rgb(0x20_22_26),
    status: rgb(0x1B_1D_21),
    base: rgb(0x2A_2C_30),
    panel: rgb(0x24_26_2B),
    control: rgb(0x34_37_3D),
    hover: rgb(0x2E_31_38),
    control_hover: rgb(0x3A_3E_45),
    border: rgb(0x3F_43_4B),
    text: rgb(0xE8_E8_EA),
    muted: rgb(0x9A_A0_A8),
    disabled: rgb(0x6A_6F_78),
    workspace: [0x2A, 0x2C, 0x30, 255],
    shadow: Color32::from_black_alpha(90),
};

/// The dark palette's steps turned over: the panels are near white, wells
/// white, and text and muted text keep their contrast with the panels.
const LIGHT: Palette = Palette {
    chrome: rgb(0xE9_EB_EE),
    status: rgb(0xE2_E4_E8),
    base: rgb(0xFF_FF_FF),
    panel: rgb(0xF4_F5_F7),
    control: rgb(0xFB_FB_FC),
    hover: rgb(0xE8_EA_EE),
    control_hover: rgb(0xEE_F0_F3),
    border: rgb(0xC8_CC_D2),
    text: rgb(0x1F_21_25),
    muted: rgb(0x5E_65_70),
    disabled: rgb(0xA4_AA_B2),
    workspace: [0xD4, 0xD7, 0xDC, 255],
    shadow: Color32::from_black_alpha(40),
};

/// Whether egui last drew with its light style, for what is painted by hand.
static LIGHT_ON: AtomicBool = AtomicBool::new(false);

fn palette() -> &'static Palette {
    if LIGHT_ON.load(Ordering::Relaxed) {
        &LIGHT
    } else {
        &DARK
    }
}

pub fn chrome() -> Color32 {
    palette().chrome
}

pub fn status() -> Color32 {
    palette().status
}

pub fn base() -> Color32 {
    palette().base
}

pub fn panel() -> Color32 {
    palette().panel
}

pub fn control() -> Color32 {
    palette().control
}

pub fn hover() -> Color32 {
    palette().hover
}

pub fn border() -> Color32 {
    palette().border
}

pub fn text() -> Color32 {
    palette().text
}

pub fn muted() -> Color32 {
    palette().muted
}

pub fn disabled() -> Color32 {
    palette().disabled
}

pub fn workspace() -> [u8; 4] {
    palette().workspace
}

/// The accent unless the user chose another.
pub const DEFAULT_ACCENT: [u8; 3] = [0xFF, 0xC9, 0x4A];

/// The accent as 0xRRGGBB; read on every frame, set from the settings.
static ACCENT: AtomicU32 = AtomicU32::new(0xFF_C9_4A);

/// What the accent fills.
pub fn accent() -> Color32 {
    rgb(ACCENT.load(Ordering::Relaxed))
}

/// `accent()` 12% darker, as Qt's `darker(112)`.
pub fn accent_pressed() -> Color32 {
    let [r, g, b, _] = accent().to_array();
    let darker = |channel: u8| (f32::from(channel) * 100.0 / 112.0).round() as u8;
    Color32::from_rgb(darker(r), darker(g), darker(b))
}

/// WCAG relative luminance.
fn luminance(color: Color32) -> f32 {
    let linear = |channel: u8| {
        let value = f32::from(channel) / 255.0;
        if value <= 0.040_45 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    };
    let [r, g, b, _] = color.to_array();
    0.2126 * linear(r) + 0.7152 * linear(g) + 0.0722 * linear(b)
}

fn contrast(a: Color32, b: Color32) -> f32 {
    let (a, b) = (luminance(a), luminance(b));
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

/// The accent for lines and text on the panels: darkened, if it must be,
/// until it stands out from them as much as WCAG asks of lines and large
/// text (3:1).
pub fn accent_ink() -> Color32 {
    ink_on(palette())
}

fn ink_on(colors: &Palette) -> Color32 {
    let mut color = accent();
    for _ in 0..64 {
        if contrast(color, colors.panel) >= 3.0 || luminance(colors.panel) < 0.2 {
            break;
        }
        let [r, g, b, _] = color.to_array();
        let darker = |channel: u8| (f32::from(channel) * 0.94) as u8;
        color = Color32::from_rgb(darker(r), darker(g), darker(b));
    }
    color
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
    for (theme, colors) in [(Theme::Dark, &DARK), (Theme::Light, &LIGHT)] {
        ctx.style_mut_of(theme, |style| style_accent(style, colors));
    }
}

/// Draws the interface dark, light, or as Windows's apps are; egui follows
/// Windows when it changes.
pub fn set_mode(ctx: &egui::Context, mode: Mode) {
    ctx.set_theme(mode.preference());
}

/// Takes the palette egui draws with for what is painted by hand, once a
/// frame; returns whether it changed.
pub fn follow(ctx: &egui::Context) -> bool {
    let light = ctx.theme() == Theme::Light;
    LIGHT_ON.swap(light, Ordering::Relaxed) != light
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

    for (theme, colors) in [(Theme::Dark, &DARK), (Theme::Light, &LIGHT)] {
        ctx.style_mut_of(theme, |style| {
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
            style_colors(style, theme, colors);
        });
    }
    ctx.set_theme(Mode::default().preference());
}

/// The colours of `colors`, the accent's included.
fn style_colors(style: &mut egui::Style, theme: Theme, colors: &Palette) {
    let visuals = &mut style.visuals;
    *visuals = match theme {
        Theme::Dark => egui::Visuals::dark(),
        Theme::Light => egui::Visuals::light(),
    };
    visuals.panel_fill = colors.panel;
    visuals.window_fill = colors.panel;
    visuals.window_stroke = Stroke::new(1.0, colors.border);
    visuals.window_corner_radius = CornerRadius::same(8);
    visuals.menu_corner_radius = CornerRadius::same(6);
    visuals.extreme_bg_color = colors.base;
    visuals.faint_bg_color = colors.hover;
    visuals.code_bg_color = colors.base;
    visuals.slider_trailing_fill = true;
    visuals.handle_shape = egui::style::HandleShape::Circle;
    visuals.popup_shadow = egui::Shadow {
        offset: [0, 4],
        blur: 12,
        spread: 0,
        color: colors.shadow,
    };
    visuals.window_shadow = visuals.popup_shadow;

    let widgets = &mut visuals.widgets;
    let radius = CornerRadius::same(6);
    widgets.noninteractive.bg_fill = colors.panel;
    widgets.noninteractive.weak_bg_fill = colors.panel;
    widgets.noninteractive.bg_stroke = Stroke::new(1.0, colors.border);
    widgets.noninteractive.fg_stroke = Stroke::new(1.0, colors.text);
    widgets.noninteractive.corner_radius = radius;
    widgets.inactive.bg_fill = colors.control;
    widgets.inactive.weak_bg_fill = colors.control;
    widgets.inactive.bg_stroke = Stroke::new(1.0, colors.border);
    widgets.inactive.fg_stroke = Stroke::new(1.0, colors.text);
    widgets.inactive.corner_radius = radius;
    widgets.hovered.bg_fill = colors.control_hover;
    widgets.hovered.weak_bg_fill = colors.control_hover;
    widgets.hovered.bg_stroke = Stroke::new(1.0, colors.disabled);
    widgets.hovered.fg_stroke = Stroke::new(1.0, colors.text);
    widgets.hovered.corner_radius = radius;
    widgets.hovered.expansion = 0.0;
    widgets.active.bg_fill = colors.control;
    widgets.active.weak_bg_fill = colors.control;
    widgets.active.fg_stroke = Stroke::new(1.0, colors.text);
    widgets.active.corner_radius = radius;
    widgets.active.expansion = 0.0;
    style_accent(style, colors);
}

/// The parts of the style drawn in the accent.
fn style_accent(style: &mut egui::Style, colors: &Palette) {
    let ink = ink_on(colors);
    let visuals = &mut style.visuals;
    visuals.hyperlink_color = ink;
    visuals.selection.bg_fill = accent();
    visuals.selection.stroke = Stroke::new(1.0, accent_text());
    visuals.text_cursor.stroke = Stroke::new(2.0, ink);
    visuals.widgets.active.bg_stroke = Stroke::new(1.0, ink);
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

    #[test]
    fn each_palette_keeps_text_readable_and_lines_in_the_accent_visible() {
        for palette in [&DARK, &LIGHT] {
            assert!(contrast(palette.text, palette.panel) >= 7.0);
            assert!(contrast(palette.text, palette.base) >= 7.0);
            assert!(contrast(palette.muted, palette.panel) >= 4.5);
        }
        assert!(contrast(ink_on(&LIGHT), LIGHT.panel) >= 3.0);
        assert_eq!(
            ink_on(&DARK),
            accent(),
            "the dark palette takes it as it is"
        );
        let ctx = egui::Context::default();
        apply(&ctx);
        set_mode(&ctx, Mode::Light);
        assert_eq!(ctx.theme(), Theme::Light);
        assert_eq!(ctx.global_style().visuals.panel_fill, LIGHT.panel);
        set_mode(&ctx, Mode::Dark);
        assert_eq!(ctx.global_style().visuals.panel_fill, DARK.panel);
    }
}

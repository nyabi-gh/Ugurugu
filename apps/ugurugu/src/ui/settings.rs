// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! The settings dialog, as 2.2.13's: tabs for general, drawing, files and
//! about, each change applied and kept at once, and a button that restores
//! the defaults. 2.2.13's shortcut tab comes with custom shortcuts (M5-5),
//! and its choice of wobbling while a stroke is drawn is left out because 3.0
//! stops playback for a stroke.

use crate::canvas::Canvas;
use crate::files::Files;
use crate::i18n::{tr, tr_with};
use crate::settings::{Language, Settings, Store};
use crate::theme;
use crate::widgets;

use super::{Panels, args};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tab {
    #[default]
    General,
    Drawing,
    Files,
    About,
}

impl Tab {
    const ALL: [Self; 4] = [Self::General, Self::Drawing, Self::Files, Self::About];

    fn title(self) -> &'static str {
        match self {
            Self::General => tr("settings-general"),
            Self::Drawing => tr("settings-drawing"),
            Self::Files => tr("settings-files"),
            Self::About => tr("settings-about"),
        }
    }
}

/// The dialog while open.
#[derive(Default)]
pub struct Dialog {
    tab: Tab,
    /// Whether the accent picker is open.
    picking_accent: bool,
}

fn language_name(language: Language) -> &'static str {
    match language {
        Language::System => tr("settings-language-system"),
        Language::English => "English",
        Language::Korean => "한국어",
        Language::Japanese => "日本語",
    }
}

/// Puts the tools and colour history kept in `settings` back, at start.
pub fn restore_tools(settings: &Settings, canvas: &mut Canvas) {
    let tools = settings.tools.clone();
    canvas.edit(|session| session.set_tools(tools));
}

/// Puts `settings` into effect everywhere but the interface language, which
/// waits for the next start.
pub fn apply(ctx: &egui::Context, settings: &Settings, canvas: &mut Canvas, files: &mut Files) {
    theme::set_accent(ctx, settings.accent);
    canvas.allow_animation(settings.wobble_animation);
    files.set_default_save_folder(settings.default_save_folder.clone());
}

/// Shows the dialog if open, and takes a default save folder chosen in it.
pub fn show(
    ctx: &egui::Context,
    canvas: &mut Canvas,
    files: &mut Files,
    store: &mut Store,
    panels: &mut Panels,
) {
    if let Some(folder) = files.take_chosen_save_folder() {
        store.change(|settings| settings.default_save_folder = Some(folder));
        apply(ctx, store.get(), canvas, files);
    }
    let Some(dialog) = panels.settings.as_mut() else {
        return;
    };
    let before = store.get().clone();
    let mut settings = before.clone();
    let mut close = false;
    let modal = egui::Modal::new(egui::Id::new("settings dialog")).show(ctx, |ui| {
        ui.set_width(460.0);
        ui.heading(tr("settings-title"));
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            for tab in Tab::ALL {
                if ui
                    .selectable_label(dialog.tab == tab, tab.title())
                    .clicked()
                {
                    dialog.tab = tab;
                    dialog.picking_accent = false;
                }
            }
        });
        ui.separator();
        ui.allocate_ui(egui::vec2(ui.available_width(), 220.0), |ui| {
            ui.set_min_height(220.0);
            match dialog.tab {
                Tab::General => general(ui, dialog, &mut settings),
                Tab::Drawing => {
                    widgets::checkbox(
                        ui,
                        &mut settings.wobble_animation,
                        tr("settings-wobble-animation"),
                    );
                }
                Tab::Files => save_folder(ui, files, &mut settings),
                Tab::About => about(ui),
            }
        });
        ui.separator();
        ui.horizontal(|ui| {
            if ui.button(tr("settings-restore-defaults")).clicked() {
                settings = settings.restored();
                dialog.picking_accent = false;
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button(tr("settings-close")).clicked() {
                    close = true;
                }
            });
        });
    });
    if settings != before {
        store.change(|kept| *kept = settings);
        apply(ctx, store.get(), canvas, files);
    }
    if close || modal.should_close() {
        panels.settings = None;
    }
}

fn general(ui: &mut egui::Ui, dialog: &mut Dialog, settings: &mut Settings) {
    egui::Grid::new("settings general")
        .num_columns(2)
        .spacing([18.0, 10.0])
        .show(ui, |ui| {
            widgets::field_label(ui, tr("settings-language"));
            egui::ComboBox::from_id_salt("settings language")
                .selected_text(language_name(settings.language))
                .width(180.0)
                .show_ui(ui, |ui| {
                    for language in Language::ALL {
                        ui.selectable_value(
                            &mut settings.language,
                            language,
                            language_name(language),
                        );
                    }
                })
                .response
                .widget_info(|| {
                    egui::WidgetInfo::labeled(
                        egui::WidgetType::ComboBox,
                        true,
                        tr("settings-language"),
                    )
                });
            ui.end_row();

            widgets::field_label(ui, tr("settings-theme-color"));
            let [r, g, b] = settings.accent.unwrap_or(theme::DEFAULT_ACCENT);
            let color = egui::Color32::from_rgb(r, g, b);
            let hex = format!("#{r:02x}{g:02x}{b:02x}");
            let button = ui.add(
                egui::Button::new(egui::RichText::new(&hex).color(theme::accent_text()))
                    .fill(color)
                    .min_size(egui::vec2(120.0, 26.0)),
            );
            button.widget_info(|| {
                egui::WidgetInfo::labeled(
                    egui::WidgetType::Button,
                    true,
                    format!("{} {hex}", tr("settings-theme-color")),
                )
            });
            if button.clicked() {
                dialog.picking_accent = !dialog.picking_accent;
            }
            ui.end_row();
        });
    if dialog.picking_accent {
        let [r, g, b] = settings.accent.unwrap_or(theme::DEFAULT_ACCENT);
        let mut color = egui::Color32::from_rgb(r, g, b);
        if egui::color_picker::color_picker_color32(
            ui,
            &mut color,
            egui::color_picker::Alpha::Opaque,
        ) {
            let [r, g, b, _] = color.to_array();
            settings.accent = (([r, g, b]) != theme::DEFAULT_ACCENT).then_some([r, g, b]);
        }
    }
    ui.add_space(6.0);
    ui.label(egui::RichText::new(tr("settings-restart")).color(theme::MUTED));
}

fn save_folder(ui: &mut egui::Ui, files: &mut Files, settings: &mut Settings) {
    let label = widgets::field_label(ui, tr("settings-default-folder"));
    ui.label(egui::RichText::new(tr("settings-default-folder-hint")).color(theme::MUTED));
    ui.add_space(4.0);
    // The configured folder, or what "system default" means; checking that
    // the folder still exists is left to the dialogs.
    let shown = settings
        .default_save_folder
        .clone()
        .or_else(ugu_win::folder::documents)
        .map(|folder| folder.display().to_string())
        .unwrap_or_default();
    ui.horizontal(|ui| {
        let mut text = shown.as_str();
        ui.add(
            egui::TextEdit::singleline(&mut text)
                .desired_width(ui.available_width() - 90.0)
                .interactive(true),
        )
        .labelled_by(label.id);
        if ui.button(tr("settings-choose")).clicked() {
            files.choose_save_folder();
        }
    });
    if ui
        .add_enabled(
            settings.default_save_folder.is_some(),
            egui::Button::new(tr("settings-system-folder")),
        )
        .clicked()
    {
        settings.default_save_folder = None;
    }
}

fn about(ui: &mut egui::Ui) {
    ui.label(
        egui::RichText::new("Ugurugu")
            .size(theme::BODY + 4.0)
            .strong(),
    );
    ui.label(tr_with(
        "settings-version",
        &args([("version", env!("CARGO_PKG_VERSION").to_owned())]),
    ));
    ui.label(tr("settings-credit-development"));
    ui.label(tr("settings-credit-icon"));
}

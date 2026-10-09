// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! The settings dialog, as 2.2.13's: tabs for general, drawing, files,
//! shortcuts and about, each change applied and kept at once, and a button
//! that restores the defaults. 2.2.13's choice of wobbling while a stroke is
//! drawn is left out because 3.0 stops playback for a stroke.

use crate::canvas::Canvas;
use crate::files::Files;
use crate::i18n::{tr, tr_with};
use crate::settings::{Language, Settings, Store};
use crate::shortcuts::{Action, Chord, Keymap};
use crate::theme;
use crate::widgets;

use super::{Panels, actions, args};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tab {
    #[default]
    General,
    Drawing,
    Files,
    Shortcuts,
    About,
}

impl Tab {
    const ALL: [Self; 5] = [
        Self::General,
        Self::Drawing,
        Self::Files,
        Self::Shortcuts,
        Self::About,
    ];

    fn title(self) -> &'static str {
        match self {
            Self::General => tr("settings-general"),
            Self::Drawing => tr("settings-drawing"),
            Self::Files => tr("settings-files"),
            Self::Shortcuts => tr("settings-shortcuts"),
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
    /// The action whose new key is awaited.
    capturing: Option<Action>,
    /// The key button to focus again: Escape, which stops the wait, also
    /// drops egui's focus.
    refocus: Option<Action>,
    /// Why the last key was refused.
    refusal: Option<String>,
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
pub fn apply(
    ctx: &egui::Context,
    settings: &Settings,
    canvas: &mut Canvas,
    files: &mut Files,
    keys: &mut Keymap,
) {
    theme::set_accent(ctx, settings.accent);
    canvas.allow_animation(settings.wobble_animation);
    files.set_default_save_folder(settings.default_save_folder.clone());
    *keys = Keymap::new(&settings.shortcuts);
}

/// Shows the dialog if open, and takes a default save folder chosen in it.
/// `typed`: chords egui turns into clipboard events, for a new shortcut.
pub fn show(
    ctx: &egui::Context,
    canvas: &mut Canvas,
    files: &mut Files,
    store: &mut Store,
    panels: &mut Panels,
    typed: &[Chord],
) {
    if let Some(folder) = files.take_chosen_save_folder() {
        store.change(|settings| settings.default_save_folder = Some(folder));
        apply(ctx, store.get(), canvas, files, &mut panels.keys);
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
                    dialog.capturing = None;
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
                Tab::Shortcuts => shortcuts(ui, dialog, canvas, &mut settings, typed),
                Tab::About => about(ui),
            }
        });
        ui.separator();
        ui.horizontal(|ui| {
            if ui.button(tr("settings-restore-defaults")).clicked() {
                settings = settings.restored();
                dialog.picking_accent = false;
                dialog.capturing = None;
                dialog.refusal = None;
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
        apply(ctx, store.get(), canvas, files, &mut panels.keys);
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

/// What was pressed while a new key is awaited.
#[derive(Debug, PartialEq)]
enum Pressed {
    Key(Chord),
    /// Tab, which goes on to move the focus.
    Cancel,
    Escape,
}

/// Takes the first key pressed in this frame, before the dialog's buttons
/// and its Escape handling see it. Tab still moves the focus and ends the
/// wait; Space, which pans the canvas, is not taken.
fn pressed(ctx: &egui::Context, typed: &[Chord]) -> Option<Pressed> {
    let mut found = typed.first().copied().map(Pressed::Key);
    ctx.input_mut(|input| {
        input.events.retain(|event| {
            let &egui::Event::Key {
                key,
                pressed: true,
                modifiers,
                ..
            } = event
            else {
                return true;
            };
            if found.is_some() {
                return true;
            }
            let chord = Chord::new(modifiers, key);
            match key {
                egui::Key::Tab => {
                    found = Some(Pressed::Cancel);
                    true
                }
                egui::Key::Escape if modifiers.is_none() => {
                    found = Some(Pressed::Escape);
                    false
                }
                _ if !chord.assignable() => false,
                _ => {
                    found = Some(Pressed::Key(chord));
                    false
                }
            }
        });
    });
    found
}

/// Every action with its key, a button that waits for a new key, and one
/// that clears it, as 2.2.13's shortcut tab.
fn shortcuts(
    ui: &mut egui::Ui,
    dialog: &mut Dialog,
    canvas: &Canvas,
    settings: &mut Settings,
    typed: &[Chord],
) {
    if let Some(action) = dialog.capturing
        && let Some(pressed) = pressed(ui.ctx(), typed)
    {
        dialog.capturing = None;
        if pressed == Pressed::Escape {
            dialog.refocus = Some(action);
        }
        if let Pressed::Key(key) = pressed {
            dialog.refusal = settings
                .shortcuts
                .assign(action, Some(key))
                .err()
                .map(|holder| {
                    tr_with(
                        "settings-shortcut-taken",
                        &args([("action", actions::label(holder, canvas).to_owned())]),
                    )
                });
        }
    }
    ui.label(egui::RichText::new(tr("settings-shortcuts-hint")).color(theme::MUTED));
    ui.add_space(4.0);
    let keys = Keymap::new(&settings.shortcuts);
    egui::ScrollArea::vertical()
        .max_height(170.0)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            egui::Grid::new("settings shortcuts")
                .num_columns(3)
                .spacing([12.0, 4.0])
                .show(ui, |ui| {
                    for &action in Action::ALL {
                        shortcut_row(ui, dialog, canvas, settings, &keys, action);
                        ui.end_row();
                    }
                });
        });
    if let Some(refusal) = &dialog.refusal {
        ui.colored_label(theme::accent(), refusal);
    }
}

fn shortcut_row(
    ui: &mut egui::Ui,
    dialog: &mut Dialog,
    canvas: &Canvas,
    settings: &mut Settings,
    keys: &Keymap,
    action: Action,
) {
    let name = actions::label(action, canvas);
    widgets::field_label(ui, name);
    let capturing = dialog.capturing == Some(action);
    let key = keys.key(action);
    let text = match key {
        _ if capturing => tr("settings-shortcut-press").to_owned(),
        Some(key) => key.text(),
        None => tr("settings-shortcut-none").to_owned(),
    };
    let button =
        ui.add(egui::Button::selectable(capturing, &text).min_size(egui::vec2(170.0, 0.0)));
    button.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, format!("{name}: {text}"))
    });
    if dialog.refocus == Some(action) {
        dialog.refocus = None;
        button.request_focus();
    }
    if button.clicked() {
        dialog.capturing = (!capturing).then_some(action);
        dialog.refusal = None;
    }
    let clear = ui
        .add_enabled(key.is_some(), egui::Button::new("×"))
        .on_hover_text(tr("settings-shortcut-clear"));
    clear.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::Button,
            key.is_some(),
            format!("{name}: {}", tr("settings-shortcut-clear")),
        )
    });
    // egui does not scroll to a widget Tab moves the focus to.
    for response in [&button, &clear] {
        if response.gained_focus() {
            response.scroll_to_me(None);
        }
    }
    if clear.clicked() {
        // Nothing conflicts with no key.
        let _ = settings.shortcuts.assign(action, None);
        dialog.capturing = None;
        dialog.refusal = None;
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

#[cfg(test)]
mod tests {
    use super::*;

    /// What a frame with `keys` pressed gives a waiting shortcut, and the
    /// keys left for the rest of the dialog.
    fn waiting(
        keys: &[(egui::Modifiers, egui::Key)],
        typed: &[Chord],
    ) -> (Option<Pressed>, Vec<egui::Key>) {
        let ctx = egui::Context::default();
        let mut input = egui::RawInput::default();
        for &(modifiers, key) in keys {
            input.events.push(egui::Event::Key {
                key,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers,
            });
        }
        let mut result = (None, Vec::new());
        let mut output = ctx.run_ui(input, |ui| {
            let found = pressed(ui.ctx(), typed);
            let left = ui.input(|input| {
                input
                    .events
                    .iter()
                    .filter_map(|event| match event {
                        egui::Event::Key { key, .. } => Some(*key),
                        _ => None,
                    })
                    .collect()
            });
            result = (found, left);
        });
        output.textures_delta.clear();
        result
    }

    #[test]
    fn a_waiting_shortcut_takes_the_first_key() {
        let none = egui::Modifiers::NONE;
        let command = egui::Modifiers::COMMAND;
        let enter = Chord::new(none, egui::Key::Enter);
        assert_eq!(
            waiting(&[(none, egui::Key::Enter), (none, egui::Key::B)], &[]),
            (Some(Pressed::Key(enter)), vec![egui::Key::B])
        );
        // Escape stops waiting, and the dialog does not close on it.
        assert_eq!(
            waiting(&[(none, egui::Key::Escape)], &[]),
            (Some(Pressed::Escape), vec![])
        );
        // Tab stops waiting and still moves the focus.
        assert_eq!(
            waiting(&[(none, egui::Key::Tab)], &[]),
            (Some(Pressed::Cancel), vec![egui::Key::Tab])
        );
        // Space is the canvas's.
        assert_eq!(waiting(&[(none, egui::Key::Space)], &[]), (None, vec![]));
        // Ctrl arrives as a key of its own before the key it is held with.
        let undo = Chord::new(command, egui::Key::Z);
        assert_eq!(
            waiting(
                &[(command, egui::Key::ControlLeft), (command, egui::Key::Z)],
                &[]
            ),
            (Some(Pressed::Key(undo)), vec![])
        );
        let copy = Chord::new(command, egui::Key::C);
        assert_eq!(waiting(&[], &[copy]), (Some(Pressed::Key(copy)), vec![]));
        assert_eq!(waiting(&[], &[]), (None, vec![]));
    }
}

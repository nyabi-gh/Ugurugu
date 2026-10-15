// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! The settings dialog: 2.2.13's tabs for general, drawing, files,
//! shortcuts and about, listed on the left, with each tab's settings in
//! cards on the right; each change is applied and kept at once, and a button
//! restores the defaults. 2.2.13's choice of wobbling while a stroke is
//! drawn is left out because 3.0 stops playback for a stroke.

use crate::canvas::Canvas;
use crate::files::Files;
use crate::i18n::{tr, tr_with};
use crate::icons::Glyph;
use crate::settings::{Language, Settings, Store};
use crate::shortcuts::{Action, Chord, Keymap};
use crate::theme;
use crate::widgets;

use super::{Panels, actions, args};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
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
    /// What the shortcut list is narrowed to.
    search: String,
    /// The app icon shown on the about tab, once it has been.
    icon: Option<egui::TextureHandle>,
    /// When the diagnostic info was last copied.
    copied: Option<std::time::Instant>,
}

fn theme_name(mode: theme::Mode) -> &'static str {
    tr(match mode {
        theme::Mode::Dark => "settings-theme-dark",
        theme::Mode::Light => "settings-theme-light",
        theme::Mode::System => "settings-theme-system",
    })
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
    if canvas.set_tools(settings.tools.clone()).is_err() {
        tracing::warn!("the tools were not restored: an edit is pending");
    }
}

/// Puts `settings` into effect everywhere but the interface language, which
/// waits for the next start.
impl Dialog {
    /// Open on the about tab, for Help > About.
    pub fn about() -> Self {
        Self {
            tab: Tab::About,
            ..Self::default()
        }
    }
}

pub fn apply(
    ctx: &egui::Context,
    settings: &Settings,
    canvas: &mut Canvas,
    files: &mut Files,
    keys: &mut Keymap,
) {
    theme::set_accent(ctx, settings.accent);
    theme::set_mode(ctx, settings.theme);
    canvas.allow_animation(settings.wobble_animation);
    canvas.tools_mut().uses_pressure = settings.tools.uses_pressure;
    files.set_default_save_folder(settings.default_save_folder.clone());
    files.set_export_edge(settings.export_edge);
    files.follow_updates(settings.check_updates, settings.update_checked);
    *keys = Keymap::new(&settings.shortcuts);
}

/// The list of tabs on the left, and the room the tab shown takes.
const SIDEBAR: f32 = 156.0;
const CONTENT: egui::Vec2 = egui::vec2(540.0, 400.0);
/// Between the list and the tab shown, with a line in the middle.
const GAP: f32 = theme::space::XXL;
/// Between the cards and the scroll bar.
const GUTTER: i8 = 14;

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
    if let Some(edge) = files.take_chosen_export_edge() {
        store.change(|settings| settings.export_edge = Some(edge));
        files.set_export_edge(Some(edge));
    }
    let Some(dialog) = panels.settings.as_mut() else {
        return;
    };
    let before = store.get().clone();
    let mut settings = before.clone();
    let mut close = false;
    let frame = egui::Frame::popup(&ctx.global_style()).inner_margin(widgets::DIALOG_MARGIN);
    let modal = egui::Modal::new(egui::Id::new("settings dialog"))
        .frame(frame)
        .show(ctx, |ui| {
            ui.set_width(SIDEBAR + 2.0 * GAP + 1.0 + CONTENT.x);
            ui.label(
                egui::RichText::new(tr("settings-title"))
                    .size(theme::TITLE)
                    .strong()
                    .color(theme::text()),
            );
            widgets::space_after(ui, theme::space::XL);
            let mut line = 0.0;
            let body = ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.set_width(SIDEBAR);
                    ui.spacing_mut().item_spacing.y = 2.0;
                    for tab in Tab::ALL {
                        let item = sidebar_item(ui, tab.title(), dialog.tab == tab);
                        if item.clicked() {
                            item.request_focus();
                            dialog.tab = tab;
                            dialog.picking_accent = false;
                            dialog.capturing = None;
                            dialog.refusal = None;
                        }
                    }
                });
                // egui's room after the list counts toward the gap.
                ui.add_space(GAP - ui.spacing().item_spacing.x);
                line = ui.cursor().left() + 0.5;
                ui.add_space(1.0 + GAP);
                ui.vertical(|ui| {
                    ui.set_width(CONTENT.x);
                    ui.label(
                        egui::RichText::new(dialog.tab.title())
                            .size(theme::HEADING)
                            .strong()
                            .color(theme::text()),
                    );
                    ui.add_space(theme::space::M);
                    // Shorter in a short window, so the buttons stay on screen.
                    let room = ui.ctx().content_rect().height() - 220.0;
                    let mut height = room.clamp(200.0, CONTENT.y);
                    if dialog.tab == Tab::Shortcuts {
                        let top = ui.cursor().top();
                        ui.scope(|ui| {
                            ui.set_max_width(CONTENT.x - f32::from(GUTTER));
                            shortcuts_header(ui, dialog, canvas, &mut settings, typed);
                        });
                        height -= ui.cursor().top() - top;
                    }
                    egui::ScrollArea::vertical()
                        .id_salt(("settings tab", dialog.tab))
                        .max_height(height)
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            ui.set_min_height(height);
                            egui::Frame::new()
                                .inner_margin(egui::Margin {
                                    left: 0,
                                    right: GUTTER,
                                    top: 0,
                                    bottom: 4,
                                })
                                .show(ui, |ui| match dialog.tab {
                                    Tab::General => general(ui, dialog, &mut settings),
                                    Tab::Drawing => drawing(ui, &mut settings),
                                    Tab::Files => save_folder(ui, files, &mut settings),
                                    Tab::Shortcuts => shortcuts(ui, dialog, canvas, &mut settings),
                                    Tab::About => about(ui, dialog),
                                });
                        });
                });
            });
            ui.painter()
                .vline(line, body.response.rect.y_range(), widgets::rule());
            widgets::divider(ui, theme::space::XL);
            ui.horizontal(|ui| {
                let restore = widgets::button(
                    ui,
                    tr("settings-restore-defaults"),
                    widgets::Emphasis::Secondary,
                    true,
                );
                if restore.clicked() {
                    settings = settings.restored();
                    dialog.picking_accent = false;
                    dialog.capturing = None;
                    dialog.refusal = None;
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if widgets::button(ui, tr("settings-close"), widgets::Emphasis::Primary, true)
                        .clicked()
                    {
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

/// A tab in the list on the left: a bar in the accent and a raised fill
/// mark the one shown.
fn sidebar_item(ui: &mut egui::Ui, text: &str, selected: bool) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(SIDEBAR, 32.0), egui::Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, selected, text)
    });
    let painter = ui.painter();
    if selected || response.hovered() || response.has_focus() {
        painter.rect_filled(
            rect,
            egui::CornerRadius::same(theme::radius::CONTROL),
            theme::hover(),
        );
    }
    if selected {
        let bar = egui::Rect::from_min_size(
            egui::pos2(rect.left(), rect.top() + 8.0),
            egui::vec2(3.0, rect.height() - 16.0),
        );
        painter.rect_filled(bar, egui::CornerRadius::same(2), theme::accent_ink());
    }
    let ink = if selected || response.has_focus() {
        theme::text()
    } else {
        theme::muted()
    };
    let galley = painter.layout_no_wrap(
        text.to_owned(),
        egui::FontId::proportional(theme::BODY),
        ink,
    );
    painter.galley(
        egui::pos2(rect.left() + 14.0, rect.center().y - galley.size().y / 2.0),
        galley,
        ink,
    );
    if response.has_focus() {
        widgets::focus_ring(painter, rect, theme::radius::CONTROL);
    }
    response
}

fn general(ui: &mut egui::Ui, dialog: &mut Dialog, settings: &mut Settings) {
    widgets::section(ui, tr("settings-appearance"), true);
    widgets::card(ui, |ui| {
        widgets::setting_row(ui, true, tr("settings-theme"), None, |ui| {
            egui::ComboBox::from_id_salt("settings-theme")
                .selected_text(theme_name(settings.theme))
                .width(190.0)
                .show_ui(ui, |ui| {
                    for mode in theme::Mode::ALL {
                        widgets::choice(ui, &mut settings.theme, mode, theme_name(mode));
                    }
                })
                .response
                .widget_info(|| {
                    egui::WidgetInfo::labeled(
                        egui::WidgetType::ComboBox,
                        true,
                        tr("settings-theme"),
                    )
                });
        });
        widgets::setting_row(ui, false, tr("settings-theme-color"), None, |ui| {
            accent_swatch(ui, dialog, settings);
        });
        if dialog.picking_accent {
            let [r, g, b] = settings.accent.unwrap_or(theme::DEFAULT_ACCENT);
            let mut color = egui::Color32::from_rgb(r, g, b);
            ui.add_space(theme::space::XS);
            if egui::color_picker::color_picker_color32(
                ui,
                &mut color,
                egui::color_picker::Alpha::Opaque,
            ) {
                let [r, g, b, _] = color.to_array();
                settings.accent = (([r, g, b]) != theme::DEFAULT_ACCENT).then_some([r, g, b]);
            }
            // As much room as over it; the next row takes back egui's.
            ui.add_space(ui.spacing().item_spacing.y + theme::space::XS);
        }
        widgets::setting_row(
            ui,
            false,
            tr("settings-language"),
            Some(tr("settings-restart")),
            |ui| {
                egui::ComboBox::from_id_salt("settings language")
                    .selected_text(language_name(settings.language))
                    .width(190.0)
                    .show_ui(ui, |ui| {
                        for language in Language::ALL {
                            widgets::choice(
                                ui,
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
            },
        );
    });

    widgets::section(ui, tr("settings-updates"), false);
    widgets::card(ui, |ui| {
        widgets::switch_row(
            ui,
            true,
            tr("settings-check-updates"),
            None,
            &mut settings.check_updates,
            true,
        );
    });
}

/// The accent as a swatch with its hex code, opening the picker.
fn accent_swatch(ui: &mut egui::Ui, dialog: &mut Dialog, settings: &Settings) {
    let [r, g, b] = settings.accent.unwrap_or(theme::DEFAULT_ACCENT);
    let color = egui::Color32::from_rgb(r, g, b);
    let swatch =
        widgets::color_swatch(ui, color, tr("settings-theme-color"), dialog.picking_accent);
    if swatch.clicked() {
        dialog.picking_accent = !dialog.picking_accent;
    }
}

fn drawing(ui: &mut egui::Ui, settings: &mut Settings) {
    widgets::card(ui, |ui| {
        widgets::switch_row(
            ui,
            true,
            tr("settings-wobble-animation"),
            None,
            &mut settings.wobble_animation,
            true,
        );
        widgets::switch_row(
            ui,
            false,
            tr("settings-pixel-grid"),
            Some(tr("settings-pixel-grid-tip")),
            &mut settings.pixel_grid,
            true,
        );
        // Every brush and eraser at once; no tool keeps its own.
        widgets::switch_row(
            ui,
            false,
            tr("pen-pressure-use"),
            Some(tr("pen-pressure-tip")),
            &mut settings.tools.uses_pressure,
            true,
        );
    });
}

fn save_folder(ui: &mut egui::Ui, files: &mut Files, settings: &mut Settings) {
    // The configured folder, or what "system default" means; checking that
    // the folder still exists is left to the dialogs.
    let shown = settings
        .default_save_folder
        .clone()
        .or_else(ugu_win::folder::documents)
        .map(|folder| folder.display().to_string())
        .unwrap_or_default();
    widgets::card(ui, |ui| {
        widgets::setting_row(
            ui,
            true,
            tr("settings-default-folder"),
            Some(tr("settings-default-folder-hint")),
            |ui| {
                let choose = widgets::button(
                    ui,
                    tr("settings-choose"),
                    widgets::Emphasis::Secondary,
                    true,
                );
                if choose.clicked() {
                    files.choose_save_folder();
                }
            },
        );
        egui::Frame::new()
            .fill(theme::panel())
            .corner_radius(egui::CornerRadius::same(theme::radius::CONTROL))
            .inner_margin(egui::Margin::symmetric(
                theme::space::L as i8,
                theme::space::M as i8,
            ))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.add(
                    egui::Label::new(egui::RichText::new(&shown).monospace().color(theme::text()))
                        .wrap(),
                )
                .widget_info(|| {
                    egui::WidgetInfo::labeled(
                        egui::WidgetType::Label,
                        true,
                        format!("{}: {shown}", tr("settings-default-folder")),
                    )
                });
            });
        ui.add_space(theme::space::M);
        let system = widgets::button(
            ui,
            tr("settings-system-folder"),
            widgets::Emphasis::Secondary,
            settings.default_save_folder.is_some(),
        );
        if system.clicked() {
            settings.default_save_folder = None;
        }
        widgets::space_after(ui, theme::space::L);
    });
}

/// What was pressed while a new key is awaited.
#[derive(Debug, PartialEq)]
enum Pressed {
    Key(Chord),
    /// Tab without Ctrl, which goes on to move the focus.
    Cancel,
    Escape,
}

/// Takes the first key pressed in this frame, before the dialog's buttons
/// and its Escape handling see it. Tab without Ctrl still moves the focus
/// and ends the wait; Space, which pans the canvas, is not taken.
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
                egui::Key::Tab if !chord.assignable() => {
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

/// Above the list: takes the key awaited, and holds the search field, the
/// hint and why the last key was refused.
fn shortcuts_header(
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
    ui.add(
        egui::TextEdit::singleline(&mut dialog.search)
            .hint_text(tr("settings-shortcuts-search"))
            .desired_width(f32::INFINITY)
            .margin(egui::Margin::symmetric(
                theme::space::M as i8,
                theme::space::S as i8,
            )),
    )
    .widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::TextEdit,
            true,
            tr("settings-shortcuts-search"),
        )
    });
    ui.add_space(theme::space::XS);
    ui.label(
        egui::RichText::new(tr("settings-shortcuts-hint"))
            .size(theme::SMALL)
            .color(theme::muted()),
    );
    if let Some(refusal) = &dialog.refusal {
        ui.colored_label(theme::accent_ink(), refusal);
    }
    ui.add_space(theme::space::S);
}

/// Every action matching the search with its key, a field that waits for a
/// new key, and a button that clears it, as 2.2.13's shortcut tab.
fn shortcuts(ui: &mut egui::Ui, dialog: &mut Dialog, canvas: &Canvas, settings: &mut Settings) {
    let keys = Keymap::new(&settings.shortcuts);
    let query = dialog.search.trim().to_lowercase();
    let shown: Vec<Action> = Action::ALL
        .iter()
        .copied()
        .filter(|&action| {
            query.is_empty()
                || actions::label(action, canvas)
                    .to_lowercase()
                    .contains(&query)
                || keys
                    .key(action)
                    .is_some_and(|key| key.text().to_lowercase().contains(&query))
        })
        .collect();
    if shown.is_empty() {
        ui.label(egui::RichText::new(tr("settings-shortcuts-no-match")).color(theme::muted()));
        return;
    }
    widgets::card(ui, |ui| {
        for (index, &action) in shown.iter().enumerate() {
            shortcut_row(ui, index == 0, dialog, canvas, settings, &keys, action);
        }
    });
}

fn shortcut_row(
    ui: &mut egui::Ui,
    first: bool,
    dialog: &mut Dialog,
    canvas: &Canvas,
    settings: &mut Settings,
    keys: &Keymap,
    action: Action,
) {
    let name = actions::label(action, canvas);
    let capturing = dialog.capturing == Some(action);
    let key = keys.key(action);
    let text = match key {
        _ if capturing => tr("settings-shortcut-press").to_owned(),
        Some(key) => key.text(),
        None => tr("settings-shortcut-none").to_owned(),
    };
    let (field, clear) = widgets::setting_row(ui, first, name, None, |ui| {
        let clear = widgets::icon_button_tip(
            ui,
            Glyph::Cancel,
            12.0,
            &format!("{name}: {}", tr("settings-shortcut-clear")),
            tr("settings-shortcut-clear"),
            key.is_some(),
        );
        let field = key_field(ui, &text, capturing, key.is_some());
        (field, clear)
    });
    field.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, format!("{name}: {text}"))
    });
    if dialog.refocus == Some(action) {
        dialog.refocus = None;
        field.request_focus();
    }
    if field.clicked() {
        dialog.capturing = (!capturing).then_some(action);
        dialog.refusal = None;
    }
    // egui does not scroll to a widget Tab moves the focus to.
    for response in [&field, &clear] {
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

/// The key of an action as a field to click: outlined in the accent while
/// it waits for a new key, muted when there is none.
fn key_field(ui: &mut egui::Ui, text: &str, capturing: bool, assigned: bool) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(160.0, 28.0), egui::Sense::click());
    let painter = ui.painter();
    let edge = if capturing {
        egui::Stroke::new(1.5, theme::accent_ink())
    } else if response.hovered() {
        egui::Stroke::new(1.0, theme::disabled())
    } else {
        egui::Stroke::new(1.0, theme::border())
    };
    painter.rect(
        rect,
        egui::CornerRadius::same(theme::radius::CONTROL),
        theme::control(),
        edge,
        egui::StrokeKind::Inside,
    );
    if response.has_focus() && !capturing {
        widgets::focus_ring(painter, rect, theme::radius::CONTROL);
    }
    let ink = if capturing || assigned {
        theme::text()
    } else {
        theme::muted()
    };
    let font = if assigned && !capturing {
        egui::FontId::monospace(theme::BODY)
    } else {
        egui::FontId::proportional(theme::BODY)
    };
    let galley = painter.layout_no_wrap(text.to_owned(), font, ink);
    painter.galley(rect.center() - galley.size() / 2.0, galley, ink);
    response
}

/// The app icon at 128 pixels, decoded once.
fn app_icon() -> &'static egui::ColorImage {
    static ICON: std::sync::OnceLock<egui::ColorImage> = std::sync::OnceLock::new();
    ICON.get_or_init(|| {
        let started = std::time::Instant::now();
        let bytes = include_bytes!("../../../../resources/icons/Ugurugu.png");
        let image = image::load_from_memory_with_format(bytes, image::ImageFormat::Png)
            .map(|image| {
                image
                    .resize_exact(128, 128, image::imageops::FilterType::Lanczos3)
                    .to_rgba8()
            })
            .map(|pixels| egui::ColorImage::from_rgba_unmultiplied([128, 128], &pixels))
            .unwrap_or_else(|error| {
                tracing::warn!(%error, "the app icon could not be read");
                egui::ColorImage::filled([128, 128], egui::Color32::TRANSPARENT)
            });
        tracing::debug!(
            ms = started.elapsed().as_secs_f64() * 1000.0,
            "app icon decoded"
        );
        image
    })
}

/// How long "Copied" stays on the copy button.
const COPIED_FOR: std::time::Duration = std::time::Duration::from_secs(2);

fn about(ui: &mut egui::Ui, dialog: &mut Dialog) {
    let icon = dialog
        .icon
        .get_or_insert_with(|| {
            ui.ctx()
                .load_texture("app icon", app_icon().clone(), egui::TextureOptions::LINEAR)
        })
        .clone();
    widgets::card(ui, |ui| {
        ui.add_space(theme::space::XL);
        ui.horizontal(|ui| {
            ui.add(egui::Image::new(&icon).fit_to_exact_size(egui::vec2(64.0, 64.0)));
            ui.add_space(theme::space::L);
            ui.vertical(|ui| {
                ui.add_space(theme::space::M);
                ui.label(
                    egui::RichText::new("Ugurugu")
                        .size(theme::DISPLAY)
                        .strong()
                        .color(theme::text()),
                );
                ui.label(
                    egui::RichText::new(format!(
                        "{}  \u{b7}  {}",
                        tr_with(
                            "settings-version",
                            &args([("version", env!("CARGO_PKG_VERSION").to_owned())]),
                        ),
                        tr_with(
                            "settings-build",
                            &args([("commit", crate::logs::COMMIT.to_owned())]),
                        ),
                    ))
                    .color(theme::muted()),
                );
            });
        });
        widgets::divider(ui, theme::space::L);
        ui.label(
            egui::RichText::new(tr("settings-credit-icon"))
                .size(theme::SMALL)
                .color(theme::muted()),
        );
        widgets::space_after(ui, theme::space::XL);
    });
    ui.add_space(theme::space::XL);
    widgets::card(ui, |ui| {
        let gpu = tr_with("settings-gpu", &args([("gpu", crate::logs::gpu())]));
        let copied = dialog.copied.is_some_and(|at| at.elapsed() < COPIED_FOR);
        widgets::setting_row(ui, true, tr("settings-diagnostics"), Some(&gpu), |ui| {
            let text = if copied {
                tr("settings-copied")
            } else {
                tr("settings-copy")
            };
            let button = widgets::button(ui, text, widgets::Emphasis::Secondary, true);
            button.widget_info(|| {
                egui::WidgetInfo::labeled(
                    egui::WidgetType::Button,
                    true,
                    tr("settings-copy-diagnostics"),
                )
            });
            if button.clicked() {
                ui.ctx().copy_text(crate::logs::summary());
                dialog.copied = Some(std::time::Instant::now());
            }
        });
        if copied {
            ui.ctx().request_repaint_after(COPIED_FOR);
        }
        widgets::setting_row(
            ui,
            false,
            tr("settings-logs"),
            Some(tr("settings-logs-hint")),
            |ui| {
                let button =
                    widgets::button(ui, tr("settings-open"), widgets::Emphasis::Secondary, true);
                button.widget_info(|| {
                    egui::WidgetInfo::labeled(
                        egui::WidgetType::Button,
                        true,
                        tr("settings-open-logs"),
                    )
                });
                if button.clicked() {
                    crate::logs::show_folder();
                }
            },
        );
    });
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
        // Ctrl+Tab is taken, so the next tab default can be given back.
        let next = Chord::new(command, egui::Key::Tab);
        assert_eq!(
            waiting(&[(command, egui::Key::Tab)], &[]),
            (Some(Pressed::Key(next)), vec![])
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

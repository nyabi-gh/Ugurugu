// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! The window around the canvas, laid out as 2.2.13: menus and quick access
//! on top, the tool rail and tool settings on the left, wobble, colour and
//! layers on the right, the animation bar under the canvas and the status
//! bar at the bottom. Only what 3.0 can already do is shown.

mod color;
mod layers;

use fluent_bundle::FluentArgs;
use ugu_core::document::{LayerKind, limits};
use ugu_core::edit::{EditError, Outcome};
use ugu_core::motion::frame_in_cycle;
use ugu_core::ops::Wobble;
use ugu_session::{Session, Tool};

use crate::canvas::{Canvas, ZOOM_RANGE};
use crate::files::{Action, Files};
use crate::i18n::{tr, tr_with};
use crate::icons::{self, Glyph};
use crate::theme;
use crate::widgets;

/// What the panels keep between frames: which are open, and edits in
/// progress that are committed as one undo step when they end.
#[derive(Default)]
pub struct Panels {
    pub layers: layers::LayerDock,
    color: color::ColorDock,
    pub shown: Shown,
    /// Frames and frames per second being edited.
    animation: Option<(f32, f32)>,
    /// Wobble amount being dragged.
    wobble: Option<f32>,
    /// Whether the wobble panel edits the current layer instead of the
    /// whole drawing.
    wobble_layer: bool,
    /// The document point under the pointer, for the status bar.
    pub pointer: Option<[f64; 2]>,
    /// Why the last edit from the panels was refused.
    refusal: Option<String>,
}

/// Panels the Window menu opens and closes.
#[derive(Clone, Copy)]
pub struct Shown {
    pub tool_settings: bool,
    pub wobble: bool,
    pub color: bool,
    pub layers: bool,
    pub animation_bar: bool,
}

impl Default for Shown {
    fn default() -> Self {
        Self {
            tool_settings: true,
            wobble: true,
            color: true,
            layers: true,
            animation_bar: true,
        }
    }
}

fn args<const N: usize>(pairs: [(&'static str, String); N]) -> FluentArgs<'static> {
    let mut args = FluentArgs::new();
    for (name, value) in pairs {
        args.set(name, value);
    }
    args
}

impl Panels {
    /// Reports a refused edit until the next edit that goes through.
    fn report(&mut self, result: Result<Outcome, EditError>) {
        match result {
            Ok(Outcome::Committed(_)) => self.refusal = None,
            Ok(Outcome::NoChange) => {}
            Err(error) => {
                tracing::warn!(%error, "edit refused");
                self.refusal = Some(error.to_string());
            }
        }
    }
}

/// Handles the canvas's shortcuts in this frame's input. Text fields keep
/// their keys, Enter that commits a composition included.
pub fn shortcuts(ctx: &egui::Context, canvas: &mut Canvas, files: &mut Files, panels: &mut Panels) {
    // Not `egui_wants_keyboard_input`, which also holds for a focused row or
    // button and would leave the shortcuts dead after clicking a layer.
    if ctx.text_edit_focused() {
        return;
    }
    // A whole chord can arrive within one frame, so each key is matched with
    // the modifiers it was pressed with, not the frame's last state.
    let pressed = |modifiers, key| ctx.input_mut(|input| input.consume_key(modifiers, key));
    let none = egui::Modifiers::NONE;
    let command = egui::Modifiers::COMMAND;
    let command_shift = egui::Modifiers::COMMAND | egui::Modifiers::SHIFT;
    if pressed(command, egui::Key::Z) {
        tracing::debug!("canvas Ctrl+Z");
        report_bool(canvas.edit(Session::undo));
    }
    if pressed(command, egui::Key::Y) || pressed(command_shift, egui::Key::Z) {
        report_bool(canvas.edit(Session::redo));
    }
    if pressed(none, egui::Key::Enter) {
        // Nothing on the canvas uses Enter yet; the IME test counts it.
        tracing::debug!("canvas Enter");
    }
    if pressed(none, egui::Key::B) {
        canvas.edit(|session| session.tool = Tool::Pen);
    }
    if pressed(none, egui::Key::E) {
        canvas.edit(|session| session.tool = Tool::Eraser);
    }
    if pressed(command, egui::Key::Plus) || pressed(command, egui::Key::Equals) {
        canvas.zoom_in_place(1.0);
    }
    if pressed(command, egui::Key::Minus) {
        canvas.zoom_in_place(-1.0);
    }
    if pressed(command, egui::Key::Num1) {
        canvas.zoom_to(1.0);
    }
    if pressed(command, egui::Key::Num0) {
        canvas.fit();
    }
    if pressed(none, egui::Key::P) {
        canvas.toggle_playback();
    }
    if pressed(command, egui::Key::T) {
        panels.shown.animation_bar = !panels.shown.animation_bar;
    }
    if pressed(command, egui::Key::N) {
        files.request(Action::New, canvas);
    }
    if pressed(command, egui::Key::O) {
        files.request(Action::Open, canvas);
    }
    if pressed(command_shift, egui::Key::S) {
        files.save_as();
    }
    if pressed(command, egui::Key::S) {
        files.save(canvas);
    }
    if pressed(command_shift, egui::Key::E) {
        files.export_png();
    }
    if pressed(command, egui::Key::Q) {
        files.request(Action::Close, canvas);
    }
}

fn report_bool(result: Result<bool, EditError>) {
    if let Err(error) = result {
        tracing::warn!(%error, "undo or redo failed");
    }
}

fn item(ui: &mut egui::Ui, text: &str, shortcut: &str) -> bool {
    ui.add(egui::Button::new(text).shortcut_text(shortcut))
        .clicked()
}

fn toggle_item(ui: &mut egui::Ui, value: &mut bool, text: &str, shortcut: &str) {
    let mut button = egui::Button::selectable(*value, text);
    if !shortcut.is_empty() {
        button = button.shortcut_text(shortcut);
    }
    if ui.add(button).clicked() {
        *value = !*value;
    }
}

/// The Window menu's items, also behind the quick access Panels button.
fn window_items(ui: &mut egui::Ui, shown: &mut Shown) {
    toggle_item(
        ui,
        &mut shown.animation_bar,
        tr("window-animation-bar"),
        "Ctrl+T",
    );
    ui.separator();
    toggle_item(ui, &mut shown.tool_settings, tr("tool-settings"), "");
    toggle_item(ui, &mut shown.wobble, tr("wobble-dock"), "");
    toggle_item(ui, &mut shown.color, tr("color-dock"), "");
    toggle_item(ui, &mut shown.layers, tr("layers"), "");
    ui.separator();
    if ui.button(tr("window-reset-layout")).clicked() {
        *shown = Shown::default();
    }
}

pub fn menu_bar(ui: &mut egui::Ui, canvas: &mut Canvas, files: &mut Files, panels: &mut Panels) {
    egui::MenuBar::new().ui(ui, |ui| {
        ui.menu_button(tr("menu-file"), |ui| {
            if item(ui, tr("file-new"), "Ctrl+N") {
                files.request(Action::New, canvas);
            }
            if item(ui, tr("file-open"), "Ctrl+O") {
                files.request(Action::Open, canvas);
            }
            ui.separator();
            if item(ui, tr("file-save"), "Ctrl+S") {
                files.save(canvas);
            }
            if item(ui, tr("file-save-as"), "Ctrl+Shift+S") {
                files.save_as();
            }
            ui.separator();
            if item(ui, tr("file-export-frame"), "Ctrl+Shift+E") {
                files.export_png();
            }
            ui.separator();
            if item(ui, tr("file-quit"), "Ctrl+Q") {
                files.request(Action::Close, canvas);
            }
        });
        ui.menu_button(tr("menu-edit"), |ui| {
            let session = canvas.session();
            let (can_undo, can_redo) = (
                session.undo_label().is_some(),
                session.redo_label().is_some(),
            );
            if ui
                .add_enabled(
                    can_undo,
                    egui::Button::new(tr("edit-undo")).shortcut_text("Ctrl+Z"),
                )
                .clicked()
            {
                report_bool(canvas.edit(Session::undo));
            }
            if ui
                .add_enabled(
                    can_redo,
                    egui::Button::new(tr("edit-redo")).shortcut_text("Ctrl+Y"),
                )
                .clicked()
            {
                report_bool(canvas.edit(Session::redo));
            }
        });
        ui.menu_button(tr("menu-view"), |ui| {
            if item(ui, tr("view-zoom-in"), "Ctrl++") {
                canvas.zoom_in_place(1.0);
            }
            if item(ui, tr("view-zoom-out"), "Ctrl+-") {
                canvas.zoom_in_place(-1.0);
            }
            if item(ui, tr("view-actual-pixels"), "Ctrl+1") {
                canvas.zoom_to(1.0);
            }
            if item(ui, tr("view-fit"), "Ctrl+0") {
                canvas.fit();
            }
            ui.separator();
            let mut playing = canvas.is_playing();
            toggle_item(ui, &mut playing, tr("view-animate"), "P");
            if playing != canvas.is_playing() {
                canvas.toggle_playback();
            }
        });
        ui.menu_button(tr("menu-tools"), |ui| {
            let tool = canvas.session().tool;
            for (each, key, shortcut) in [
                (Tool::Pen, "tool-brush", "B"),
                (Tool::Eraser, "tool-eraser", "E"),
            ] {
                if ui
                    .add(egui::Button::selectable(tool == each, tr(key)).shortcut_text(shortcut))
                    .clicked()
                {
                    canvas.edit(|session| session.tool = each);
                }
            }
        });
        ui.menu_button(tr("menu-window"), |ui| window_items(ui, &mut panels.shown));
    });
}

/// The quick access bar: panels on the left, undo and redo on the right.
pub fn quick_access(ui: &mut egui::Ui, canvas: &mut Canvas, panels: &mut Panels) {
    ui.horizontal(|ui| {
        let button = widgets::icon_button(ui, Glyph::Panels, 22.0, tr("panels-tip"), true);
        egui::Popup::menu(&button).show(|ui| window_items(ui, &mut panels.shown));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let session = canvas.session();
            let (can_undo, can_redo) = (
                session.undo_label().is_some(),
                session.redo_label().is_some(),
            );
            if widgets::icon_button(ui, Glyph::Redo, 22.0, tr("edit-redo"), can_redo).clicked() {
                report_bool(canvas.edit(Session::redo));
            }
            if widgets::icon_button(ui, Glyph::Undo, 22.0, tr("edit-undo"), can_undo).clicked() {
                report_bool(canvas.edit(Session::undo));
            }
        });
    });
}

/// The tool rail.
pub fn rail(ui: &mut egui::Ui, canvas: &mut Canvas) {
    ui.spacing_mut().item_spacing.y = 2.0;
    let tool = canvas.session().tool;
    for (each, glyph, key, shortcut) in [
        (Tool::Pen, Glyph::Brush, "tool-brush", "B"),
        (Tool::Eraser, Glyph::Eraser, "tool-eraser", "E"),
    ] {
        if widgets::tool_button(ui, glyph, tr(key), shortcut, tool == each).clicked() {
            canvas.edit(|session| session.tool = each);
        }
    }
}

/// A dock's header: its name, and a button that closes it.
fn dock_header(ui: &mut egui::Ui, title: &str, open: &mut bool) {
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(title)
                .size(theme::SMALL)
                .color(theme::MUTED),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let close = ui
                .add(egui::Button::new(egui::RichText::new("×").color(theme::MUTED)).frame(false))
                .on_hover_text(tr("dock-close"));
            if close.clicked() {
                *open = false;
            }
        });
    });
}

/// A slider with its number field, on one row under a field label.
fn slider_row(
    ui: &mut egui::Ui,
    label: &str,
    name: &str,
    value: &mut f32,
    range: std::ops::RangeInclusive<f32>,
    suffix: &str,
    decimals: usize,
) -> egui::Response {
    ui.horizontal(|ui| {
        widgets::field_label(ui, label);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let field = widgets::number(ui, value, range.clone(), suffix, decimals, 64.0);
            let width = ui.available_width().max(40.0);
            let slider = widgets::slider(ui, value, range, name, width);
            slider.union(field)
        })
        .inner
    })
    .inner
}

pub fn tool_settings(ui: &mut egui::Ui, canvas: &mut Canvas, panels: &mut Panels) {
    dock_header(ui, tr("tool-settings"), &mut panels.shown.tool_settings);
    let tool = canvas.session().tool;
    let mut settings = match tool {
        Tool::Pen => canvas.session().pen,
        Tool::Eraser => canvas.session().eraser,
    };
    let before = settings;
    ui.add_space(4.0);
    let (size_name, range) = match tool {
        Tool::Pen => (tr("brush-size"), 1.0..=128.0),
        Tool::Eraser => (tr("eraser-size"), 1.0..=128.0),
    };
    slider_row(
        ui,
        tr("size"),
        size_name,
        &mut settings.width,
        range,
        " px",
        0,
    );
    let mut stabilization = settings.stabilizer * 100.0;
    slider_row(
        ui,
        tr("stabilization"),
        tr("stabilization-name"),
        &mut stabilization,
        0.0..=100.0,
        "%",
        0,
    );
    settings.stabilizer = stabilization / 100.0;
    if tool == Tool::Pen {
        widgets::check_row(
            ui,
            tr("antialiasing"),
            tr("antialiasing-name"),
            &mut settings.antialias,
        )
        .on_hover_text(tr("antialiasing-tip"));
    }
    if settings != before {
        canvas.edit(|session| match tool {
            Tool::Pen => session.pen = settings,
            Tool::Eraser => session.eraser = settings,
        });
    }
}

pub fn wobble(ui: &mut egui::Ui, canvas: &mut Canvas, panels: &mut Panels) {
    dock_header(ui, tr("wobble-dock"), &mut panels.shown.wobble);
    let document = canvas.session().document();
    let current = canvas.session().current_layer();
    let layer_wobble = match document.layer(current).map(|layer| &layer.kind) {
        Some(LayerKind::Paint(paint)) => Some(paint.wobble),
        _ => None,
    };
    let (drawing, frames, fps) = (document.wobble, document.frames, document.frames_per_second);
    let scope = |layer: bool| {
        if layer {
            tr("wobble-layer")
        } else {
            tr("wobble-whole")
        }
    };
    egui::ComboBox::from_id_salt("wobble-scope")
        .width(ui.available_width())
        .selected_text(scope(panels.wobble_layer))
        .show_ui(ui, |ui| {
            ui.selectable_value(&mut panels.wobble_layer, false, scope(false));
            ui.add_enabled_ui(layer_wobble.is_some(), |ui| {
                ui.selectable_value(&mut panels.wobble_layer, true, scope(true));
            });
        });
    let on_layer = panels.wobble_layer && layer_wobble.is_some();
    let amount = match layer_wobble {
        Some(Some(own)) if on_layer => own.amount,
        _ => drawing.amount,
    };
    if on_layer {
        let overridden = matches!(layer_wobble, Some(Some(_)));
        let follow = ui
            .add_enabled(overridden, egui::Button::new(tr("wobble-follow")))
            .on_hover_text(tr("wobble-follow-tip"));
        if follow.clicked() {
            panels.report(canvas.edit(|session| {
                session.update_layer(current, "Follow drawing settings", |layer| {
                    if let LayerKind::Paint(paint) = &mut layer.kind {
                        paint.wobble = None;
                    }
                })
            }));
        }
    }
    widgets::field_label(ui, tr("wobble"));
    let mut value = panels.wobble.unwrap_or(amount);
    let (min, max) = (*limits::WOBBLE_AMOUNT.start(), *limits::WOBBLE_AMOUNT.end());
    let response = ui
        .horizontal(|ui| {
            let preview = wobble_preview(ui, value, ui.ctx().input(|input| input.time));
            preview.on_hover_text(tr("wobble-preview-tip"));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let field = widgets::number(ui, &mut value, min..=max, " px", 1, 64.0);
                let width = ui.available_width().max(40.0);
                widgets::slider(ui, &mut value, min..=max, tr("wobble"), width).union(field)
            })
            .inner
        })
        .inner;
    value = (value * 10.0).round() / 10.0;
    if response.changed() {
        panels.wobble = Some(value);
    }
    let ended = response.drag_stopped() || (response.changed() && !response.dragged());
    if ended
        && let Some(value) = panels.wobble.take()
        && value != amount
    {
        let result = if on_layer {
            canvas.edit(|session| {
                session.update_layer(current, "Layer wobble", |layer| {
                    if let LayerKind::Paint(paint) = &mut layer.kind {
                        paint.wobble = Some(Wobble::classic(value));
                    }
                })
            })
        } else {
            canvas.edit(|session| session.set_animation(frames, fps, Wobble::classic(value)))
        };
        panels.report(result);
    }
}

/// A short line wobbling by `amount`, moving only while the pointer is on
/// it so that an idle window draws nothing.
fn wobble_preview(ui: &mut egui::Ui, amount: f32, time: f64) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(58.0, 24.0), egui::Sense::hover());
    let phase = if response.hovered() {
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(80));
        (time * 8.0).floor()
    } else {
        0.0
    };
    let points: Vec<egui::Pos2> = (0..=24)
        .map(|step| {
            let position = f64::from(step) / 24.0;
            let turn = std::f64::consts::TAU * position;
            let offset =
                0.62 * (1.25 * turn + phase).sin() + 0.38 * (2.75 * turn + 0.9 + phase * 1.3).sin();
            let x = rect.left() + 4.0 + (rect.width() - 8.0) * position as f32;
            let y = rect.center().y - (f64::from(amount) * 0.9 * offset) as f32;
            egui::pos2(x, y.clamp(rect.top() + 2.0, rect.bottom() - 2.0))
        })
        .collect();
    ui.painter().add(egui::Shape::line(
        points,
        egui::Stroke::new(2.0, theme::ACCENT),
    ));
    response
}

pub fn color(ui: &mut egui::Ui, canvas: &mut Canvas, panels: &mut Panels) {
    dock_header(ui, tr("color-dock"), &mut panels.shown.color);
    panels.color.show(ui, canvas);
}

pub fn layers(ui: &mut egui::Ui, canvas: &mut Canvas, panels: &mut Panels) {
    dock_header(ui, tr("layers"), &mut panels.shown.layers);
    let Panels {
        layers, refusal, ..
    } = panels;
    layers.show(ui, canvas, refusal);
}

/// The animation bar: play, the frame and frame count, the scrubber and the
/// playback speed.
pub fn animation_bar(ui: &mut egui::Ui, canvas: &mut Canvas, panels: &mut Panels) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 10.0;
        let playing = canvas.is_playing();
        let (rect, play) = ui.allocate_exact_size(egui::vec2(30.0, 30.0), egui::Sense::click());
        let fill = match (playing, play.hovered()) {
            (true, true) => theme::ACCENT_PRESSED,
            (true, false) => theme::ACCENT,
            (false, true) => theme::HOVER,
            (false, false) => theme::CONTROL,
        };
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::same(7), fill);
        let (glyph, ink) = if playing {
            (Glyph::Pause, theme::ACCENT_TEXT)
        } else {
            (Glyph::Play, theme::TEXT)
        };
        icons::paint(
            ui.painter(),
            egui::Rect::from_center_size(rect.center(), egui::Vec2::splat(20.0)),
            glyph,
            ink,
            0.0,
        );
        play.widget_info(|| {
            egui::WidgetInfo::selected(egui::WidgetType::Button, true, playing, tr("play"))
        });
        if play.on_hover_text(tr("play-tip")).clicked() {
            canvas.toggle_playback();
        }

        let document = canvas.session().document();
        let (frames, fps, wobble) = (document.frames, document.frames_per_second, document.wobble);
        let mut frame = frame_in_cycle(canvas.session().frame(), frames) as f32 + 1.0;
        let field = widgets::number(ui, &mut frame, 1.0..=frames as f32, "", 0, 46.0)
            .on_hover_text(tr("frame-current"));
        if field.changed() {
            if canvas.is_playing() {
                canvas.toggle_playback();
            }
            canvas.edit(|session| session.set_frame(frame as i64 - 1));
        }
        ui.label(egui::RichText::new("/").color(theme::MUTED));
        let (mut count, mut speed) = panels.animation.unwrap_or((frames as f32, fps));
        let (low, high) = (*limits::FRAMES.start() as f32, *limits::FRAMES.end() as f32);
        let count_field = widgets::number(ui, &mut count, low..=high, "", 0, 46.0)
            .on_hover_text(tr("frame-count"));

        let right = 46.0 + 30.0 + 90.0 + 30.0;
        let width = (ui.available_width() - right).max(160.0);
        let current = frame_in_cycle(canvas.session().frame(), frames);
        if let Some(chosen) = scrubber(ui, current, frames, canvas.is_playing(), width) {
            if canvas.is_playing() {
                canvas.toggle_playback();
            }
            canvas.edit(|session| session.set_frame(i64::from(chosen)));
        }

        ui.label(
            egui::RichText::new(tr("fps"))
                .size(theme::SMALL)
                .color(theme::MUTED),
        );
        let (low, high) = (
            *limits::FRAMES_PER_SECOND.start(),
            *limits::FRAMES_PER_SECOND.end(),
        );
        let speed_field =
            widgets::number(ui, &mut speed, low..=high, "", 0, 46.0).on_hover_text(tr("fps-tip"));
        let edited = count_field.changed() || speed_field.changed();
        if edited {
            panels.animation = Some((count, speed));
        }
        let ended = [&count_field, &speed_field].iter().any(|field| {
            field.drag_stopped() || (field.changed() && !field.dragged()) || field.lost_focus()
        });
        if ended && let Some((count, speed)) = panels.animation.take() {
            let result =
                canvas.edit(|session| session.set_animation(count.round() as u32, speed, wobble));
            panels.report(result);
        }
        let hide =
            widgets::text_icon_button(ui, Glyph::MoveDown, tr("bar-hide"), tr("bar-hide-tip"));
        if hide.clicked() {
            panels.shown.animation_bar = false;
        }
    });
}

/// One slot per frame along a track. Returns the frame clicked or dragged
/// to.
fn scrubber(
    ui: &mut egui::Ui,
    current: u32,
    frames: u32,
    playing: bool,
    width: f32,
) -> Option<u32> {
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(width, 26.0), egui::Sense::click_and_drag());
    let track = rect.shrink2(egui::vec2(0.0, 2.5));
    let slot = track.width() / frames.max(1) as f32;
    let slot_at = |x: f32| {
        (((x - track.left()) / slot).floor() as i64).clamp(0, i64::from(frames) - 1) as u32
    };
    let painter = ui.painter();
    let focused = response.has_focus();
    painter.rect(
        track,
        egui::CornerRadius::same(6),
        theme::STATUS,
        egui::Stroke::new(
            1.0,
            if focused {
                theme::ACCENT
            } else {
                theme::BORDER
            },
        ),
        egui::StrokeKind::Inside,
    );
    let slot_rect = |index: u32| {
        egui::Rect::from_min_size(
            egui::pos2(track.left() + slot * index as f32, track.top()),
            egui::vec2(slot, track.height()),
        )
        .shrink(2.0)
    };
    if let Some(hover) = response.hover_pos() {
        painter.rect_filled(
            slot_rect(slot_at(hover.x)),
            egui::CornerRadius::same(4),
            theme::CONTROL,
        );
    }
    for index in 0..frames {
        painter.circle_filled(slot_rect(index).center(), 1.4, theme::DISABLED);
    }
    let head = slot_rect(current);
    if playing {
        let bar = egui::Rect::from_min_max(egui::pos2(head.left(), head.bottom() - 3.0), head.max);
        painter.rect_filled(
            bar,
            egui::CornerRadius::same(1),
            theme::ACCENT.gamma_multiply(0.55),
        );
    } else {
        let head = egui::Rect::from_center_size(
            head.center(),
            egui::vec2(head.width().max(6.0), head.height()),
        );
        painter.rect_filled(head, egui::CornerRadius::same(4), theme::ACCENT);
    }
    let name = tr_with(
        "scrubber-frame",
        &args([
            ("frame", (current + 1).to_string()),
            ("count", frames.to_string()),
        ]),
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Slider, true, &name));
    let mut chosen = response
        .interact_pointer_pos()
        .map(|pointer| slot_at(pointer.x));
    if focused {
        let step = ui.input(|input| {
            i64::from(input.key_pressed(egui::Key::ArrowRight))
                - i64::from(input.key_pressed(egui::Key::ArrowLeft))
        });
        if step != 0 {
            chosen = Some((i64::from(current) + step).rem_euclid(i64::from(frames)) as u32);
        }
    }
    chosen.filter(|&frame| frame != current)
}

/// The status bar: the last message on the left; the pointer, zoom and fit
/// on the right.
pub fn status_bar(
    ui: &mut egui::Ui,
    canvas: &mut Canvas,
    message: Option<(&str, bool)>,
    pointer: Option<[f64; 2]>,
) {
    ui.horizontal(|ui| {
        match message {
            Some((text, true)) => ui.colored_label(theme::ACCENT, text),
            Some((text, false)) => ui.colored_label(theme::MUTED, text),
            None => ui.colored_label(theme::MUTED, tr("status-ready")),
        };
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let fit = widgets::text_icon_button(ui, Glyph::FitView, tr("fit"), tr("fit-tip"));
            if fit.clicked() {
                canvas.fit();
            }
            let scale = canvas.scale();
            let mut percent = (scale * 100.0) as f32;
            let (low, high) = (
                *ZOOM_RANGE.start() as f32 * 100.0,
                *ZOOM_RANGE.end() as f32 * 100.0,
            );
            let field = widgets::number(ui, &mut percent, low..=high, "%", 0, 72.0)
                .on_hover_text(tr("canvas-zoom-percent"));
            if field.changed() {
                canvas.zoom_to(f64::from(percent) / 100.0);
            }
            // Logarithmic, so every step reads as the same change in size.
            let mut position = (scale.ln() as f32 - low.ln() + 100f32.ln()) / ((high / low).ln());
            let slider = widgets::slider(ui, &mut position, 0.0..=1.0, tr("canvas-zoom"), 96.0);
            if slider.changed() {
                let percent = low * (high / low).powf(position);
                canvas.zoom_to(f64::from(percent) / 100.0);
            }
            let text = pointer.map_or_else(String::new, |[x, y]| {
                tr_with(
                    "status-pointer",
                    &args([("x", format!("{x:.0}")), ("y", format!("{y:.0}"))]),
                )
            });
            ui.add_sized(
                [150.0, 20.0],
                egui::Label::new(egui::RichText::new(text).color(theme::MUTED)),
            );
        });
    });
}

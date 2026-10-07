// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! The panels around the canvas: tools on top, layers on the right, status
//! at the bottom, and the keyboard shortcuts the canvas takes.

use ugu_core::document::limits;
use ugu_core::document::{LayerId, LayerKind};
use ugu_core::edit::{EditError, Outcome};
use ugu_core::motion::frame_in_cycle;
use ugu_core::ops::{Rgba8, Wobble};
use ugu_session::{Session, Tool};

use crate::canvas::Canvas;

/// Edits in progress in the panels, committed as one undo step each when
/// they end, not on every keystroke or drag step.
#[derive(Default)]
pub struct Panels {
    name: Option<(LayerId, String)>,
    opacity: Option<(LayerId, f32)>,
    /// Frames, frames per second and wobble amount being edited.
    animation: Option<(u32, f32, f32)>,
}

fn report(result: Result<Outcome, EditError>) {
    if let Err(error) = result {
        tracing::warn!(%error, "edit refused");
    }
}

/// Handles the canvas's shortcuts in this frame's input. Text fields keep
/// their keys, Enter that commits a composition included.
pub fn shortcuts(ctx: &egui::Context, canvas: &mut Canvas) {
    if ctx.egui_wants_keyboard_input() {
        return;
    }
    // A whole chord can arrive within one frame, so each key is matched with
    // the modifiers it was pressed with, not the frame's last state.
    let pressed = |modifiers, key| ctx.input_mut(|input| input.consume_key(modifiers, key));
    let command = egui::Modifiers::COMMAND;
    let command_shift = egui::Modifiers::COMMAND | egui::Modifiers::SHIFT;
    if pressed(command, egui::Key::Z) {
        tracing::debug!("canvas Ctrl+Z");
        report_bool(canvas.edit(Session::undo));
    }
    if pressed(command, egui::Key::Y) || pressed(command_shift, egui::Key::Z) {
        report_bool(canvas.edit(Session::redo));
    }
    if pressed(egui::Modifiers::NONE, egui::Key::Enter) {
        // Nothing on the canvas uses Enter yet; the IME test counts it.
        tracing::debug!("canvas Enter");
    }
    if pressed(egui::Modifiers::NONE, egui::Key::B) {
        canvas.edit(|session| session.tool = Tool::Pen);
    }
    if pressed(egui::Modifiers::NONE, egui::Key::E) {
        canvas.edit(|session| session.tool = Tool::Eraser);
    }
    if pressed(command, egui::Key::Plus) || pressed(command, egui::Key::Equals) {
        canvas.zoom_in_place(1.0);
    }
    if pressed(command, egui::Key::Minus) {
        canvas.zoom_in_place(-1.0);
    }
    if pressed(command, egui::Key::Num0) {
        canvas.fit();
    }
    if pressed(egui::Modifiers::NONE, egui::Key::P) {
        canvas.toggle_playback();
    }
}

fn report_bool(result: Result<bool, EditError>) {
    if let Err(error) = result {
        tracing::warn!(%error, "undo or redo failed");
    }
}

pub fn tools(ui: &mut egui::Ui, canvas: &mut Canvas) {
    ui.horizontal_wrapped(|ui| {
        let tool = canvas.session().tool;
        if ui.selectable_label(tool == Tool::Pen, "Pen").clicked() {
            canvas.edit(|session| session.tool = Tool::Pen);
        }
        if ui
            .selectable_label(tool == Tool::Eraser, "Eraser")
            .clicked()
        {
            canvas.edit(|session| session.tool = Tool::Eraser);
        }
        ui.separator();

        let mut settings = match tool {
            Tool::Pen => canvas.session().pen,
            Tool::Eraser => canvas.session().eraser,
        };
        let before = settings;
        if tool == Tool::Pen {
            let mut color = settings.color.0;
            let label = ui.label("Colour");
            ui.color_edit_button_srgba_unmultiplied(&mut color)
                .labelled_by(label.id);
            settings.color = Rgba8(color);
        }
        let label = ui.label("Size");
        ui.add(
            egui::DragValue::new(&mut settings.width)
                .range(1.0..=200.0)
                .speed(0.2)
                .max_decimals(1),
        )
        .labelled_by(label.id);
        let label = ui.label("Smoothing");
        let mut smoothing = settings.stabilizer * 100.0;
        ui.add(
            egui::Slider::new(&mut smoothing, 0.0..=100.0)
                .suffix("%")
                .integer(),
        )
        .labelled_by(label.id);
        settings.stabilizer = smoothing / 100.0;
        ui.checkbox(&mut settings.antialias, "Antialiasing");
        if settings != before {
            canvas.edit(|session| match tool {
                Tool::Pen => session.pen = settings,
                Tool::Eraser => session.eraser = settings,
            });
        }
        ui.separator();

        let session = canvas.session();
        let undo = session.undo_label().map(|label| format!("Undo {label}"));
        let redo = session.redo_label().map(|label| format!("Redo {label}"));
        let button = |ui: &mut egui::Ui, text: &str, tip: Option<String>| {
            ui.add_enabled(tip.is_some(), egui::Button::new(text))
                .on_hover_text(tip.unwrap_or_default())
                .clicked()
        };
        if button(ui, "Undo", undo) {
            report_bool(canvas.edit(Session::undo));
        }
        if button(ui, "Redo", redo) {
            report_bool(canvas.edit(Session::redo));
        }
    });
}

impl Panels {
    pub fn layers(&mut self, ui: &mut egui::Ui, canvas: &mut Canvas) {
        ui.heading("Layers");
        ui.horizontal_wrapped(|ui| {
            if ui.button("Add layer").clicked() {
                report(canvas.edit(Session::add_layer));
                tracing::info!("layer added");
            }
            let count = canvas.session().document().layers.len();
            if ui
                .add_enabled(count > 1, egui::Button::new("Delete"))
                .clicked()
            {
                report(canvas.edit(Session::remove_layer));
            }
            if ui.button("Up").clicked() {
                report(canvas.edit(|session| session.move_layer(1)));
            }
            if ui.button("Down").clicked() {
                report(canvas.edit(|session| session.move_layer(-1)));
            }
            if ui.button("Merge down").clicked() {
                report(canvas.edit(Session::merge_down));
            }
        });
        ui.separator();

        let current = canvas.session().current_layer();
        // Top first, as the layers stack on the canvas.
        let rows: Vec<(LayerId, String, bool)> = canvas
            .session()
            .document()
            .layers
            .iter()
            .rev()
            .map(|layer| (layer.id, layer.name.clone(), layer.visible))
            .collect();
        egui::ScrollArea::vertical()
            .max_height(ui.available_height() * 0.6)
            .show(ui, |ui| {
                for (id, name, visible) in rows {
                    ui.horizontal(|ui| {
                        let mut shown = visible;
                        let toggle = ui.checkbox(&mut shown, "");
                        toggle.widget_info(|| {
                            egui::WidgetInfo::selected(
                                egui::WidgetType::Checkbox,
                                true,
                                shown,
                                format!("Show {name}"),
                            )
                        });
                        if shown != visible {
                            report(canvas.edit(|session| {
                                session.update_layer(id, "Show or hide layer", |layer| {
                                    layer.visible = shown;
                                })
                            }));
                        }
                        if ui.selectable_label(id == current, &name).clicked() {
                            canvas.edit(|session| session.select_layer(id));
                        }
                    });
                }
            });
        ui.separator();
        self.properties(ui, canvas, current);
    }

    pub fn timeline(&mut self, ui: &mut egui::Ui, canvas: &mut Canvas) {
        ui.horizontal_wrapped(|ui| {
            let playing = canvas.is_playing();
            if ui.button(if playing { "Stop" } else { "Play" }).clicked() {
                canvas.toggle_playback();
            }
            let document = canvas.session().document();
            let (frames, fps, wobble) = (
                document.frames,
                document.frames_per_second,
                document.wobble.amount,
            );
            let label = ui.label("Frame");
            let mut frame = frame_in_cycle(canvas.session().frame(), frames) + 1;
            let slider = ui
                .add(egui::Slider::new(&mut frame, 1..=frames))
                .labelled_by(label.id);
            if slider.changed() {
                if canvas.is_playing() {
                    canvas.toggle_playback();
                }
                canvas.edit(|session| session.set_frame(i64::from(frame) - 1));
            }
            ui.separator();

            let (mut new_frames, mut new_fps, mut new_wobble) =
                self.animation.unwrap_or((frames, fps, wobble));
            let mut ended = false;
            let mut field = |response: egui::Response| {
                if response.drag_stopped() || (response.changed() && !response.dragged()) {
                    ended = true;
                }
                response.changed()
            };
            let label = ui.label("Frames");
            let response = ui
                .add(egui::DragValue::new(&mut new_frames).range(limits::FRAMES))
                .labelled_by(label.id);
            let mut changed = field(response);
            let label = ui.label("FPS");
            let response = ui
                .add(
                    egui::DragValue::new(&mut new_fps)
                        .range(limits::FRAMES_PER_SECOND)
                        .speed(0.1)
                        .max_decimals(1),
                )
                .labelled_by(label.id);
            changed |= field(response);
            let label = ui.label("Wobble");
            let response = ui
                .add(egui::Slider::new(&mut new_wobble, limits::WOBBLE_AMOUNT).max_decimals(1))
                .labelled_by(label.id);
            changed |= field(response);
            if changed {
                self.animation = Some((new_frames, new_fps, new_wobble));
            }
            if ended {
                self.animation = None;
                report(canvas.edit(|session| {
                    session.set_animation(new_frames, new_fps, Wobble::classic(new_wobble))
                }));
            }
        });
    }

    fn properties(&mut self, ui: &mut egui::Ui, canvas: &mut Canvas, current: LayerId) {
        let Some(layer) = canvas.session().document().layer(current) else {
            return;
        };
        let LayerKind::Paint(paint) = &layer.kind else {
            return;
        };
        let (name, opacity) = (layer.name.clone(), paint.opacity);

        if self.name.as_ref().is_none_or(|(id, _)| *id != current) {
            self.name = Some((current, name.clone()));
        }
        let label = ui.label("Name");
        let field = ui
            .add(
                egui::TextEdit::singleline(&mut self.name.as_mut().expect("set").1)
                    .id_salt("layer-name"),
            )
            .labelled_by(label.id);
        if field.lost_focus() {
            let edited = self.name.take().map(|(_, text)| text).unwrap_or_default();
            let edited: String = edited
                .trim()
                .chars()
                .take(ugu_core::document::limits::LAYER_NAME_CHARS)
                .collect();
            if !edited.is_empty() && edited != name {
                report(canvas.edit(|session| {
                    session.update_layer(current, "Rename layer", |layer| layer.name = edited)
                }));
                tracing::info!("layer renamed");
            }
        }

        if self.opacity.is_none_or(|(id, _)| id != current) {
            self.opacity = Some((current, opacity));
        }
        let label = ui.label("Opacity");
        let mut percent = self.opacity.expect("set").1 * 100.0;
        let slider = ui
            .add(
                egui::Slider::new(&mut percent, 0.0..=100.0)
                    .suffix("%")
                    .integer(),
            )
            .labelled_by(label.id);
        self.opacity = Some((current, percent / 100.0));
        // One undo step when a drag ends or a value is typed, not per step.
        if (slider.drag_stopped() || (slider.changed() && !slider.dragged()))
            && percent / 100.0 != opacity
        {
            let value = percent / 100.0;
            report(canvas.edit(|session| {
                session.update_layer(current, "Layer opacity", |layer| {
                    if let LayerKind::Paint(paint) = &mut layer.kind {
                        paint.opacity = value;
                    }
                })
            }));
            self.opacity = None;
        }
    }
}

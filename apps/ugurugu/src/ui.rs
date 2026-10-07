// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! The panels around the canvas: tools on top, layers on the right, status
//! at the bottom, and the keyboard shortcuts the canvas takes.

use std::collections::HashSet;

use ugu_core::command;
use ugu_core::document::limits;
use ugu_core::document::{Document, DocumentError, Layer, LayerId, LayerKind};
use ugu_core::edit::{EditError, Outcome};
use ugu_core::motion::frame_in_cycle;
use ugu_core::ops::{Blend, MergeRefusal, Rgba8, Wobble};
use ugu_session::{Session, Tool};

use crate::canvas::Canvas;
use crate::files::{Action, Files};

/// Edits in progress in the panels, committed as one undo step each when
/// they end, not on every keystroke or drag step.
#[derive(Default)]
pub struct Panels {
    name: Option<(LayerId, String)>,
    opacity: Option<(LayerId, f32)>,
    /// Frames, frames per second and wobble amount being edited.
    animation: Option<(u32, f32, f32)>,
    /// Groups whose children the layer list leaves out.
    collapsed: HashSet<LayerId>,
    /// Why the last edit from the panels was refused.
    refusal: Option<String>,
}

/// Handles the canvas's shortcuts in this frame's input. Text fields keep
/// their keys, Enter that commits a composition included.
pub fn shortcuts(ctx: &egui::Context, canvas: &mut Canvas, files: &mut Files) {
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
}

fn report_bool(result: Result<bool, EditError>) {
    if let Err(error) = result {
        tracing::warn!(%error, "undo or redo failed");
    }
}

pub fn tools(ui: &mut egui::Ui, canvas: &mut Canvas, files: &mut Files) {
    ui.horizontal_wrapped(|ui| {
        if ui.button("New").clicked() {
            files.request(Action::New, canvas);
        }
        if ui.button("Open").clicked() {
            files.request(Action::Open, canvas);
        }
        if ui.button("Save").clicked() {
            files.save(canvas);
        }
        if ui.button("Save as").clicked() {
            files.save_as();
        }
        if ui.button("Export PNG").clicked() {
            files.export_png();
        }
        ui.separator();

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
    /// Reports a refused edit under the layer buttons until the next edit.
    fn report(&mut self, result: Result<Outcome, EditError>) {
        match result {
            Ok(Outcome::Committed(_)) => self.refusal = None,
            Ok(Outcome::NoChange) => {}
            Err(error) => {
                tracing::warn!(%error, "edit refused");
                self.refusal = Some(refusal_text(&error));
            }
        }
    }

    pub fn layers(&mut self, ui: &mut egui::Ui, canvas: &mut Canvas) {
        ui.heading("Layers");
        let current = canvas.session().current_layer();
        let document = canvas.session().document();
        let is_group = matches!(
            document.layer(current).map(|layer| &layer.kind),
            Some(LayerKind::Group(_))
        );
        let (count, index) = document
            .position(current)
            .and_then(|(parent, index)| Some((command::siblings(document, parent)?.len(), index)))
            .unwrap_or((0, 0));
        let merge_refused = merge_refusal(document, current);
        let removable = canvas.session().can_remove_layer();
        ui.horizontal_wrapped(|ui| {
            if ui.button("Add layer").clicked() {
                self.report(canvas.edit(Session::add_layer));
                tracing::info!("layer added");
            }
            if ui
                .button("Add group")
                .on_hover_text("Add a group containing the selected layer")
                .clicked()
            {
                self.report(canvas.edit(Session::add_group));
            }
            if ui
                .add_enabled(is_group, egui::Button::new("Ungroup"))
                .clicked()
            {
                self.report(canvas.edit(Session::ungroup));
            }
            if ui
                .add_enabled(removable, egui::Button::new("Delete"))
                .clicked()
            {
                self.report(canvas.edit(Session::remove_layer));
            }
            if ui
                .add_enabled(index + 1 < count, egui::Button::new("Up"))
                .clicked()
            {
                self.report(canvas.edit(|session| session.move_layer(1)));
            }
            if ui
                .add_enabled(index > 0, egui::Button::new("Down"))
                .clicked()
            {
                self.report(canvas.edit(|session| session.move_layer(-1)));
            }
            let merge = ui
                .add_enabled(merge_refused.is_none(), egui::Button::new("Merge down"))
                .on_disabled_hover_text(merge_refused.unwrap_or_default());
            if merge.clicked() {
                self.report(canvas.edit(Session::merge_down));
            }
        });
        if let Some(refusal) = &self.refusal {
            ui.colored_label(ui.visuals().warn_fg_color, refusal);
        }
        ui.separator();

        let mut rows = Vec::new();
        flatten(
            &canvas.session().document().layers,
            0,
            &self.collapsed,
            &mut rows,
        );
        egui::ScrollArea::vertical()
            .max_height(ui.available_height() * 0.6)
            .show(ui, |ui| {
                for row in rows {
                    self.row(ui, canvas, &row, row.id == current);
                }
            });
        ui.separator();
        self.properties(ui, canvas, current);
    }

    fn row(&mut self, ui: &mut egui::Ui, canvas: &mut Canvas, row: &Row, selected: bool) {
        ui.horizontal(|ui| {
            ui.add_space(row.depth as f32 * INDENT);
            let fold = ui.spacing().interact_size.y;
            match row.collapsed {
                Some(collapsed) => {
                    let (text, action) = if collapsed {
                        ("⏵", "Expand")
                    } else {
                        ("⏷", "Collapse")
                    };
                    let button = ui.add_sized([fold, fold], egui::Button::new(text).frame(false));
                    button.widget_info(|| {
                        egui::WidgetInfo::labeled(
                            egui::WidgetType::Button,
                            true,
                            format!("{action} {}", row.name),
                        )
                    });
                    if button.clicked() && !self.collapsed.remove(&row.id) {
                        self.collapsed.insert(row.id);
                    }
                }
                None => {
                    ui.add_space(fold + ui.spacing().item_spacing.x);
                }
            }
            let mut shown = row.visible;
            let toggle = ui.checkbox(&mut shown, "");
            toggle.widget_info(|| {
                egui::WidgetInfo::selected(
                    egui::WidgetType::Checkbox,
                    true,
                    shown,
                    format!("Show {}", row.name),
                )
            });
            if shown != row.visible {
                let id = row.id;
                self.report(canvas.edit(|session| {
                    session.update_layer(id, "Show or hide layer", |layer| {
                        layer.visible = shown;
                    })
                }));
            }
            if row.clipped {
                ui.weak("clip").on_hover_text("Clipped to the layer below");
            }
            let mut text = egui::RichText::new(&row.name);
            if row.collapsed.is_some() {
                text = text.strong();
            }
            if ui.selectable_label(selected, text).clicked() {
                let id = row.id;
                canvas.edit(|session| session.select_layer(id));
            }
            if row.blend != Blend::Normal {
                ui.weak(blend_name(row.blend));
            }
        });
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
                self.report(canvas.edit(|session| {
                    session.set_animation(new_frames, new_fps, Wobble::classic(new_wobble))
                }));
            }
        });
    }

    fn properties(&mut self, ui: &mut egui::Ui, canvas: &mut Canvas, current: LayerId) {
        let document = canvas.session().document();
        let Some(layer) = document.layer(current) else {
            return;
        };
        let (name, opacity, blend, clipped) = (
            layer.name.clone(),
            layer.opacity(),
            layer.blend(),
            layer.clip_to_below(),
        );
        let is_paint = matches!(layer.kind, LayerKind::Paint(_));
        let parent = document.position(current).and_then(|(parent, _)| parent);
        let mut groups = Vec::new();
        groups_outside(&document.layers, current, &mut groups);

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
                self.report(canvas.edit(|session| {
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
            self.report(canvas.edit(|session| {
                session.update_layer(current, "Layer opacity", |layer| match &mut layer.kind {
                    LayerKind::Paint(paint) => paint.opacity = value,
                    LayerKind::Group(group) => group.opacity = value,
                })
            }));
            self.opacity = None;
        }

        let label = ui.label("Blend mode");
        let mut chosen = blend;
        egui::ComboBox::from_id_salt("layer-blend")
            .selected_text(blend_name(blend))
            .show_ui(ui, |ui| {
                for each in BLENDS {
                    ui.selectable_value(&mut chosen, each, blend_name(each));
                }
            })
            .response
            .labelled_by(label.id);
        if chosen != blend {
            self.report(canvas.edit(|session| {
                session.update_layer(current, "Change layer blend mode", |layer| match &mut layer
                    .kind
                {
                    LayerKind::Paint(paint) => paint.blend = chosen,
                    LayerKind::Group(group) => group.blend = chosen,
                })
            }));
        }

        // As in 2.2.13, only paint layers are clipped from the panel; a
        // group clipped in a file still shows it in the list.
        if is_paint {
            let mut clip = clipped;
            if ui.checkbox(&mut clip, "Clip to layer below").changed() {
                self.report(canvas.edit(|session| {
                    session.update_layer(current, "Change layer clipping", |layer| {
                        if let LayerKind::Paint(paint) = &mut layer.kind {
                            paint.clip_to_below = clip;
                        }
                    })
                }));
            }
        }

        let label = ui.label("Group");
        let mut target = parent;
        let shown = parent
            .and_then(|id| groups.iter().find(|(group, _)| *group == id))
            .map_or(NO_GROUP, |(_, name)| name.as_str());
        egui::ComboBox::from_id_salt("layer-group")
            .selected_text(shown)
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut target, None, NO_GROUP);
                for (id, name) in &groups {
                    ui.selectable_value(&mut target, Some(*id), name);
                }
            })
            .response
            .labelled_by(label.id);
        if target != parent {
            self.report(canvas.edit(|session| session.move_to_group(target)));
        }
    }
}

const INDENT: f32 = 14.0;
const NO_GROUP: &str = "No group";
const BLENDS: [Blend; 4] = [
    Blend::Normal,
    Blend::Multiply,
    Blend::Screen,
    Blend::Overlay,
];

/// A line of the layer list.
struct Row {
    id: LayerId,
    name: String,
    visible: bool,
    depth: usize,
    /// For a group, whether its children are hidden from the list.
    collapsed: Option<bool>,
    clipped: bool,
    blend: Blend,
}

/// The layers top first, as they stack on the canvas, each group followed by
/// its children unless it is collapsed.
fn flatten(layers: &[Layer], depth: usize, collapsed: &HashSet<LayerId>, rows: &mut Vec<Row>) {
    for layer in layers.iter().rev() {
        let folded = collapsed.contains(&layer.id);
        rows.push(Row {
            id: layer.id,
            name: layer.name.clone(),
            visible: layer.visible,
            depth,
            collapsed: matches!(layer.kind, LayerKind::Group(_)).then_some(folded),
            clipped: layer.clip_to_below(),
            blend: layer.blend(),
        });
        if let LayerKind::Group(group) = &layer.kind
            && !folded
        {
            flatten(&group.children, depth + 1, collapsed, rows);
        }
    }
}

/// The groups `id` can be moved into: all but itself and what it holds,
/// top first.
fn groups_outside(layers: &[Layer], id: LayerId, groups: &mut Vec<(LayerId, String)>) {
    for layer in layers.iter().rev() {
        if layer.id == id {
            continue;
        }
        if let LayerKind::Group(group) = &layer.kind {
            groups.push((layer.id, layer.name.clone()));
            groups_outside(&group.children, id, groups);
        }
    }
}

fn blend_name(blend: Blend) -> &'static str {
    match blend {
        Blend::Normal => "Normal",
        Blend::Multiply => "Multiply",
        Blend::Screen => "Screen",
        Blend::Overlay => "Overlay",
    }
}

/// Why the current layer cannot be merged down, or `None` when it can.
fn merge_refusal(document: &Document, id: LayerId) -> Option<&'static str> {
    let error = command::merge_down(document, id).err()?;
    Some(match error {
        EditError::Merge(MergeRefusal::Blend) => "Both layers must use the Normal blend mode",
        EditError::Merge(MergeRefusal::Clipping) => {
            "Neither layer may be clipped, and no clipping layer may rest on this one"
        }
        EditError::Merge(MergeRefusal::CanvasEpoch) => {
            "The layers use incompatible canvas histories"
        }
        EditError::Merge(MergeRefusal::CanvasChange) => {
            "This layer resizes the canvas, so merging would change the picture"
        }
        _ => {
            let paint = |id: LayerId| {
                matches!(
                    document.layer(id).map(|layer| &layer.kind),
                    Some(LayerKind::Paint(_))
                )
            };
            let below = document.position(id).and_then(|(parent, index)| {
                Some(
                    command::siblings(document, parent)?
                        .get(index.checked_sub(1)?)?
                        .id,
                )
            });
            if !paint(id) {
                "Select a paint layer to merge"
            } else if !below.is_some_and(paint) {
                "No paint layer is directly below"
            } else {
                "Both layers must be shown or hidden alike, and be reference layers alike"
            }
        }
    })
}

fn refusal_text(error: &EditError) -> String {
    match error {
        EditError::Document(DocumentError::TooDeep(_)) => format!(
            "Groups can be nested at most {} deep",
            ugu_core::document::limits::LAYER_DEPTH
        ),
        EditError::Document(DocumentError::TooManyLayers(_)) => format!(
            "A document holds at most {} layers and groups",
            ugu_core::document::limits::LAYERS
        ),
        EditError::Document(DocumentError::TooManyOperations(_)) => format!(
            "A document holds at most {} operations",
            ugu_core::document::limits::OPERATIONS
        ),
        other => format!("The edit was refused: {other}"),
    }
}

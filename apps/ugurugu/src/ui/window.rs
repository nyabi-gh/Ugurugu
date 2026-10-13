// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! The whole window around the canvas for one frame, without the GPU, so
//! that tests can drive it as the render thread does.

use std::sync::Arc;
use std::time::{Duration, Instant};

use ugu_core::ops::Affine;
use ugu_core::selection::Selection;

use crate::canvas::Canvas;
use crate::clipboard::Clipboard;
use crate::files::Files;
use crate::settings::Store;
use crate::shortcuts::Chord;
use crate::theme;

use super::Panels;

/// What one frame works on.
pub struct Parts<'a> {
    pub canvas: &'a mut Canvas,
    pub files: &'a mut Files,
    pub clipboard: &'a mut Clipboard,
    pub settings: &'a mut Store,
    pub panels: &'a mut Panels,
    /// Chords egui turns into clipboard events (see `shortcuts`).
    pub typed: &'a [Chord],
    /// Shown in the status bar beside messages, as it lasts.
    pub warning: Option<&'a str>,
    /// A diagnostics row under the status bar.
    pub diagnostics: Option<&'a mut dyn FnMut(&mut egui::Ui)>,
    /// A diagnostics panel after the docked ones.
    pub probe: Option<&'a mut dyn FnMut(&mut egui::Ui)>,
}

/// What the frame left for drawing the canvas.
pub struct Shown {
    /// Left, top, right and bottom in physical pixels.
    pub canvas_area: [i32; 4],
    /// See `selection_overlay`.
    pub ants: Option<(Arc<Selection>, Affine, f32)>,
}

pub fn window(ui: &mut egui::Ui, parts: Parts<'_>) -> Shown {
    let Parts {
        canvas,
        files,
        clipboard,
        settings,
        panels,
        typed,
        warning,
        diagnostics,
        probe,
    } = parts;
    let mut canvas_area = [0; 4];
    let mut ants = None;
    panels.follow_document(canvas);
    panels.focus.begin(ui.ctx());
    // Before the widgets run, so focus is what the key was pressed in.
    super::shortcuts(ui.ctx(), canvas, files, clipboard, panels, typed);
    files.release_held(canvas);
    files.confirm(ui.ctx(), canvas);
    files.ask_export_close(ui.ctx(), canvas);
    files.ask_recovery(ui.ctx(), canvas);
    let telling = crate::logs::tell_about_crashes(ui.ctx(), files.asking());
    files.ask_update(ui.ctx(), telling, canvas);
    files.resume_closing(telling, canvas);
    files.ask_animation(ui.ctx(), canvas);
    files.ask_new_document(ui.ctx(), canvas);
    super::dialogs(ui.ctx(), canvas, files, settings, panels, typed);
    let bar = |fill, x, y| {
        egui::Frame::new()
            .fill(fill)
            .inner_margin(egui::Margin::symmetric(x, y))
    };
    egui::Panel::top("menu")
        .frame(bar(theme::chrome(), 6, 2))
        .show_separator_line(false)
        .show(ui, |ui| {
            super::menu_bar(ui, canvas, files, clipboard, panels)
        });
    egui::Panel::top("quick access")
        .frame(bar(theme::chrome(), 10, 5))
        .show(ui, |ui| {
            super::quick_access(ui, canvas, files, clipboard, panels)
        });
    egui::Panel::bottom("status")
        .frame(bar(theme::status(), 8, 2))
        .show_separator_line(false)
        .show(ui, |ui| {
            // What failed first, then what was refused; the slow display is
            // said beside them, as it lasts.
            let message = files
                .message()
                .filter(|_| files.failing())
                .or(canvas.notice())
                .map(|text| (text, true))
                .or(files.message().map(|text| (text, false)));
            let message = message.map(|(text, warn)| (text.to_owned(), warn));
            if let Some(due) = files.message_due() {
                ui.ctx()
                    .request_repaint_after(due.saturating_duration_since(Instant::now()));
            }
            let pointer = panels.pointer;
            let export = files.export_status();
            if export.is_some() {
                // The progress moves on without input.
                ui.ctx().request_repaint_after(Duration::from_millis(200));
            }
            let cancel = super::status_bar(
                ui,
                canvas,
                message.as_ref().map(|(text, warn)| (text.as_str(), *warn)),
                warning,
                export.as_deref(),
                pointer,
                &panels.keys,
            );
            if cancel {
                files.cancel_export();
            }
            if let Some(diagnostics) = diagnostics {
                ui.horizontal(diagnostics);
            }
        });
    egui::Panel::left("tool rail")
        .resizable(false)
        .exact_size(48.0)
        .frame(bar(theme::chrome(), 4, 8))
        .show(ui, |ui| super::rail(ui, canvas, &panels.keys));
    super::dock_areas(ui, canvas, panels, probe);
    if panels.layout.animation_bar {
        egui::Panel::bottom("animation bar")
            .frame(
                egui::Frame::new()
                    .fill(theme::chrome())
                    .inner_margin(egui::Margin {
                        left: 12,
                        right: 14,
                        top: 9,
                        bottom: 9,
                    }),
            )
            .show(ui, |ui| super::animation_bar(ui, canvas, panels));
    }
    egui::Panel::top("document tabs")
        .exact_size(super::tabs::HEIGHT)
        .frame(bar(
            theme::chrome(),
            theme::space::XS as i8,
            theme::space::XS as i8,
        ))
        .show_separator_line(false)
        .show(ui, |ui| super::tabs::tab_bar(ui, canvas, files, panels));
    // The tab shown may have changed.
    panels.follow_document(canvas);
    // No panel fill: the canvas is drawn under egui.
    egui::CentralPanel::default()
        .frame(egui::Frame::NONE)
        .show(ui, |ui| {
            canvas_area = canvas.layout(ui);
            ants = super::selection_overlay(ui, canvas);
            super::text_overlay(ui, canvas);
            super::pick_cursor(ui, canvas);
            super::brush_cursor(ui, canvas);
            super::selection_actions(ui, canvas, panels);
            let ppp = f64::from(ui.ctx().pixels_per_point());
            let area = ui.max_rect();
            panels.pointer = ui
                .input(|input| input.pointer.hover_pos())
                .filter(|pointer| area.contains(*pointer))
                .map(|pointer| {
                    canvas.document_point([f64::from(pointer.x) * ppp, f64::from(pointer.y) * ppp])
                });
        });
    super::dock_floating(ui, canvas, panels);
    super::actions::end_frame(ui.ctx());
    Shown { canvas_area, ants }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use egui::accesskit::{NodeId, Rect, Role};

    use super::*;

    /// The controls in the frame, and where they are.
    fn controls(output: &egui::FullOutput) -> HashMap<NodeId, Rect> {
        let tree = output
            .platform_output
            .accesskit_update
            .as_ref()
            .expect("the frame describes itself");
        tree.nodes
            .iter()
            .filter(|(_, node)| {
                matches!(
                    node.role(),
                    Role::Button
                        | Role::CheckBox
                        | Role::ComboBox
                        | Role::Slider
                        | Role::TextInput
                        | Role::SpinButton
                )
            })
            .filter_map(|(id, node)| Some((*id, node.bounds()?)))
            .collect()
    }

    #[test]
    fn every_control_of_the_default_panels_can_be_scrolled_to_in_a_low_window() {
        for [width, height] in [
            [1600.0, 900.0],
            [1280.0, 720.0],
            [960.0, 540.0],
            [800.0, 600.0],
        ] {
            let ctx = egui::Context::default();
            theme::apply(&ctx);
            ctx.enable_accesskit();
            let mut canvas = Canvas::new(ugu_core::document::Document::new([64, 64]), |_| {});
            let mut files = Files::new(0, |_| {});
            let mut clipboard = Clipboard::new(|_| {});
            let mut settings = Store::open(None);
            let mut panels = Panels::default();
            let mut frame = |events: Vec<egui::Event>| {
                let input = egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(width, height),
                    )),
                    focused: true,
                    events,
                    ..Default::default()
                };
                let mut area = [0; 4];
                let mut output = ctx.run_ui(input, |ui| {
                    area = window(
                        ui,
                        Parts {
                            canvas: &mut canvas,
                            files: &mut files,
                            clipboard: &mut clipboard,
                            settings: &mut settings,
                            panels: &mut panels,
                            typed: &[],
                            warning: None,
                            diagnostics: None,
                            probe: None,
                        },
                    )
                    .canvas_area;
                });
                output.textures_delta.clear();
                (output, area)
            };
            let (mut output, mut area) = frame(Vec::new());
            for _ in 0..2 {
                (output, area) = frame(Vec::new());
            }
            let below: Vec<NodeId> = controls(&output)
                .into_iter()
                .filter(|(_, bounds)| bounds.y1 > f64::from(height))
                .map(|(id, _)| id)
                .collect();
            // The wheel over the foot of each area.
            let y = area[3] as f32 - 20.0;
            for x in [(48 + area[0]) as f32 / 2.0, (area[2] as f32 + width) / 2.0] {
                for _ in 0..6 {
                    (output, _) = frame(vec![
                        egui::Event::PointerMoved(egui::pos2(x, y)),
                        egui::Event::MouseWheel {
                            unit: egui::MouseWheelUnit::Point,
                            delta: egui::vec2(0.0, -400.0),
                            phase: egui::TouchPhase::Move,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ]);
                }
            }
            for _ in 0..2 {
                (output, _) = frame(Vec::new());
            }
            let now = controls(&output);
            for id in below {
                let bounds = now[&id];
                assert!(
                    bounds.y0 >= 0.0 && bounds.y1 <= f64::from(height),
                    "{width}×{height}: a control stays out of reach at {bounds:?}"
                );
            }
            drop(frame);
            files.end();
        }
    }
}

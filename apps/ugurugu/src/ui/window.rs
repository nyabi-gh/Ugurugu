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
    /// Shown in the status bar in place of messages.
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
    panels.focus.begin(ui.ctx());
    // Before the widgets run, so focus is what the key was pressed in.
    super::shortcuts(ui.ctx(), canvas, files, clipboard, panels, typed);
    files.confirm(ui.ctx(), canvas);
    files.ask_recovery(ui.ctx(), canvas);
    files.ask_animation(ui.ctx(), canvas);
    files.ask_new_document(ui.ctx(), canvas);
    super::dialogs(ui.ctx(), canvas, files, settings, panels, typed);
    let bar = |fill, x, y| {
        egui::Frame::new()
            .fill(fill)
            .inner_margin(egui::Margin::symmetric(x, y))
    };
    egui::Panel::top("menu")
        .frame(bar(theme::CHROME, 6, 2))
        .show_separator_line(false)
        .show(ui, |ui| {
            super::menu_bar(ui, canvas, files, clipboard, panels)
        });
    egui::Panel::top("quick access")
        .frame(bar(theme::CHROME, 10, 5))
        .show(ui, |ui| {
            super::quick_access(ui, canvas, files, clipboard, panels)
        });
    egui::Panel::bottom("status")
        .frame(bar(theme::STATUS, 8, 2))
        .show_separator_line(false)
        .show(ui, |ui| {
            let message = warning
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
                export.as_deref(),
                pointer,
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
        .frame(bar(theme::CHROME, 4, 8))
        .show(ui, |ui| super::rail(ui, canvas, &panels.keys));
    super::dock_areas(ui, canvas, panels, probe);
    if panels.layout.animation_bar {
        egui::Panel::bottom("animation bar")
            .frame(
                egui::Frame::new()
                    .fill(theme::CHROME)
                    .inner_margin(egui::Margin {
                        left: 12,
                        right: 14,
                        top: 9,
                        bottom: 9,
                    }),
            )
            .show(ui, |ui| super::animation_bar(ui, canvas, panels));
    }
    // No panel fill: the canvas is drawn under egui.
    egui::CentralPanel::default()
        .frame(egui::Frame::NONE)
        .show(ui, |ui| {
            canvas_area = canvas.layout(ui);
            ants = super::selection_overlay(ui, canvas);
            super::text_overlay(ui, canvas);
            super::pick_cursor(ui, canvas);
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

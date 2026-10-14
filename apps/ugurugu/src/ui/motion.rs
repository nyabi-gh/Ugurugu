// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! The move tool's settings: where the current layer is on the frame
//! shown, as numbers, and its key there.

use ugu_core::keys::{Ease, Pose, limits};

use crate::canvas::Canvas;
use crate::i18n::tr;
use crate::icons::Glyph;
use crate::shortcuts::{Action, Keymap};
use crate::theme;
use crate::widgets;

pub fn settings(ui: &mut egui::Ui, canvas: &mut Canvas, keys: &Keymap) {
    let session = canvas.session();
    let layer = session.current_layer();
    let shown = session.pose(layer);
    let key = session.key_here(layer);
    widgets::panel_heading(ui, tr("motion"));
    ui.label(
        egui::RichText::new(if key.is_some() {
            tr("motion-keyed")
        } else {
            tr("motion-unkeyed")
        })
        .size(theme::SMALL)
        .color(theme::muted()),
    );
    let mut pose = shown;
    let mut fields = Vec::new();
    let mut row = |ui: &mut egui::Ui,
                   label: &str,
                   value: &mut f32,
                   range: std::ops::RangeInclusive<f32>,
                   suffix: &str| {
        widgets::form_row(ui, label, |ui| {
            fields.push(widgets::number(ui, value, range, suffix, 1, 96.0, label));
        });
    };
    let offset = limits::OFFSET as f32;
    let mut x = pose.offset[0] as f32;
    let mut y = pose.offset[1] as f32;
    let mut rotation = pose.rotation as f32;
    let mut scale = pose.scale as f32 * 100.0;
    row(ui, tr("motion-x"), &mut x, -offset..=offset, " px");
    row(ui, tr("motion-y"), &mut y, -offset..=offset, " px");
    let turn = limits::ROTATION as f32;
    row(ui, tr("motion-rotation"), &mut rotation, -turn..=turn, "°");
    let (low, high) = (*limits::SCALE.start() as f32, *limits::SCALE.end() as f32);
    row(
        ui,
        tr("motion-scale"),
        &mut scale,
        low * 100.0..=high * 100.0,
        "%",
    );
    let mut flip = pose.flip;
    let flipped = widgets::check_row(ui, tr("motion-flip"), tr("motion-flip"), &mut flip);
    pose = Pose {
        offset: [f64::from(x), f64::from(y)],
        rotation: f64::from(rotation),
        scale: f64::from(scale) / 100.0,
        flip,
    };
    let dragging = fields.iter().any(egui::Response::dragged);
    let ended = flipped.changed()
        || fields.iter().any(|field| {
            field.drag_stopped() || (field.changed() && !field.dragged()) || field.lost_focus()
        });
    if pose != shown {
        if dragging {
            canvas.preview_pose(pose);
        } else {
            canvas.set_pose(pose);
        }
    } else if ended && canvas.session().posing().is_some() {
        canvas.set_pose(pose);
    }

    ui.add_space(theme::space::S);
    widgets::panel_heading(ui, tr("ease"));
    ui.add_enabled_ui(key.is_some(), |ui| {
        let mut ease = key.map_or(Ease::Linear, |key| key.ease);
        let changed = widgets::segmented(
            ui,
            &mut ease,
            &[
                (Ease::Linear, tr("ease-linear")),
                (Ease::Smooth, tr("ease-smooth")),
                (Ease::Hold, tr("ease-hold")),
            ],
            3,
        )
        .changed();
        if changed {
            canvas.set_ease(ease);
        }
    });

    ui.add_space(theme::space::S);
    ui.horizontal_wrapped(|ui| {
        let (glyph, text) = if key.is_some() {
            (Glyph::Remove, tr("key-remove"))
        } else {
            (Glyph::Key, tr("key-add"))
        };
        let tip = widgets::with_key(text, &keys.text(Action::ToggleKey));
        if widgets::text_icon_button(ui, glyph, text, &tip).clicked() {
            canvas.toggle_key();
        }
        let tip = widgets::with_key(tr("key-previous"), &keys.text(Action::PreviousKey));
        if widgets::icon_button_tip(ui, Glyph::KeyPrevious, 22.0, tr("key-previous"), &tip, true)
            .clicked()
        {
            canvas.step_key(-1);
        }
        let tip = widgets::with_key(tr("key-next"), &keys.text(Action::NextKey));
        if widgets::icon_button_tip(ui, Glyph::KeyNext, 22.0, tr("key-next"), &tip, true).clicked()
        {
            canvas.step_key(1);
        }
    });
    ui.add_space(theme::space::XS);
    ui.label(
        egui::RichText::new(tr("move-hint"))
            .size(theme::SMALL)
            .color(theme::muted()),
    );
}

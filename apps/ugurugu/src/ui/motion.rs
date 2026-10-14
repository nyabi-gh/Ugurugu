// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! The move tool's settings: where the current layer is on the frame
//! shown, as numbers, and its key there.

use ugu_core::keys::{Ease, limits};

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
    // As tall either way, so that what follows holds still while playing
    // through frames with and without keys.
    let width = ui.available_width();
    let tallest = [tr("motion-keyed"), tr("motion-unkeyed")]
        .map(|text| {
            ui.painter()
                .layout(
                    text.to_owned(),
                    egui::FontId::proportional(theme::SMALL),
                    theme::muted(),
                    width,
                )
                .size()
                .y
        })
        .into_iter()
        .fold(0.0, f32::max);
    ui.allocate_ui(egui::vec2(width, tallest), |ui| {
        ui.set_min_height(tallest);
        ui.label(
            egui::RichText::new(if key.is_some() {
                tr("motion-keyed")
            } else {
                tr("motion-unkeyed")
            })
            .size(theme::SMALL)
            .color(theme::muted()),
        );
    });
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
    // Only what was edited here is taken back: the fields hold f32, and a
    // pose dragged on the canvas is not one.
    let [moved_x, moved_y, turned, scaled] = [0, 1, 2, 3].map(|at| fields[at].changed());
    if moved_x {
        pose.offset[0] = f64::from(x);
    }
    if moved_y {
        pose.offset[1] = f64::from(y);
    }
    if turned {
        pose.rotation = f64::from(rotation);
    }
    if scaled {
        pose.scale = f64::from(scale) / 100.0;
    }
    pose.flip = flip;
    let changed = moved_x || moved_y || turned || scaled || flipped.changed();
    let dragging = fields.iter().any(egui::Response::dragged);
    let ended = fields
        .iter()
        .any(|field| field.drag_stopped() || field.lost_focus());
    if changed {
        if dragging {
            canvas.preview_pose(pose);
        } else {
            canvas.set_pose(pose);
        }
    } else if ended && canvas.session().posing().is_some() && !canvas.posing_on_canvas() {
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
        let others = [tr("key-add"), tr("key-remove")];
        if widgets::text_icon_button_as_wide(ui, glyph, text, &tip, &others).clicked() {
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

#[cfg(test)]
mod tests {
    use egui::{Pos2, RawInput, Rect, vec2};
    use ugu_core::document::Document;
    use ugu_session::Tool;
    use ugu_win::clock::Ticks;
    use ugu_win::pointer::{Buttons, PointerKind, PointerSample};

    use super::*;
    use crate::input::{CanvasInput, Gesture};

    fn at(x: f64, y: f64) -> PointerSample {
        PointerSample {
            position: [x, y],
            pressure: None,
            tilt: None,
            twist: None,
            buttons: Buttons::empty(),
            in_contact: true,
            inverted: false,
            time: Ticks(0),
            frame_id: 0,
        }
    }

    #[test]
    fn a_drag_on_the_canvas_keys_where_it_ends_while_the_settings_show() {
        let ctx = egui::Context::default();
        let keys = Keymap::default();
        let mut canvas = Canvas::new(Document::new([400, 300]), |_| {});
        canvas.set_tool(Tool::Move).unwrap();
        let show = |canvas: &mut Canvas| {
            let mut output = ctx.run_ui(
                RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(300.0, 600.0))),
                    ..RawInput::default()
                },
                |ui| settings(ui, canvas, &keys),
            );
            output.textures_delta.clear();
        };
        show(&mut canvas);
        let start = [200.0, 150.0];
        canvas.apply(CanvasInput::Begin(
            Gesture::Draw,
            PointerKind::Mouse,
            at(start[0], start[1]),
        ));
        // Steps that land between f32 values, as a zoomed view's do.
        let mut x = start[0];
        for _ in 0..30 {
            x += 4.0 / 0.65;
            canvas.apply(CanvasInput::Extend(at(x, start[1])));
            show(&mut canvas);
        }
        canvas.apply(CanvasInput::End(at(x, start[1])));
        show(&mut canvas);
        let layer = canvas.session().current_layer();
        let key = canvas.session().key_here(layer).unwrap();
        let moved = canvas.document_point([x, start[1]])[0] - canvas.document_point(start)[0];
        assert!(
            (key.pose.offset[0] - moved).abs() < 1e-3,
            "{:?} for {moved}",
            key.pose.offset
        );
        assert_eq!(
            canvas.session().undo_label(),
            Some("Move layer on this frame")
        );
        assert!(canvas.edit(|session| session.undo()).unwrap());
        assert!(canvas.session().key_here(layer).is_none(), "one step");
    }
    #[test]
    fn the_settings_keep_their_place_on_frames_with_and_without_a_key() {
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        let keys = Keymap::default();
        let mut canvas = Canvas::new(Document::new([400, 300]), |_| {});
        canvas.set_tool(Tool::Move).unwrap();
        let place = |canvas: &mut Canvas| {
            let mut output = ctx.run_ui(
                RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(260.0, 600.0))),
                    ..RawInput::default()
                },
                |ui| settings(ui, canvas, &keys),
            );
            output.textures_delta.clear();
            let tree = output.platform_output.accesskit_update.unwrap();
            tree.nodes
                .iter()
                .find(|(_, node)| node.label().is_some_and(|label| label == tr("key-next")))
                .and_then(|(_, node)| node.bounds())
                .unwrap()
        };
        // Twice, as egui settles a new layout on its second pass.
        place(&mut canvas);
        let unkeyed = place(&mut canvas);
        canvas.toggle_key();
        assert!(
            canvas
                .session()
                .key_here(canvas.session().current_layer())
                .is_some()
        );
        place(&mut canvas);
        let keyed = place(&mut canvas);
        assert_eq!(unkeyed, keyed);
    }
}

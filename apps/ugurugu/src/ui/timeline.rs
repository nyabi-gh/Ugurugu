// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! The timeline rows under the animation bar: the keys of the current
//! layer and the groups holding it, and the cels of its animation folder,
//! one slot per frame lined up with the scrubber.

use ugu_core::document::{LayerId, LayerKind};
use ugu_core::keys::{Ease, Exposure};
use ugu_core::motion::frame_in_cycle;
use ugu_session::Session;

use crate::canvas::Canvas;
use crate::i18n::{args, tr, tr_with};
use crate::theme;
use crate::widgets;

use super::Panels;

const ROW: f32 = 22.0;

/// What a row shows.
enum Row {
    Keys(LayerId),
    Cels(LayerId),
}

/// The rows, under the bar's controls; `track` is the scrubber's span, which
/// the frames line up with.
pub fn rows(ui: &mut egui::Ui, canvas: &mut Canvas, panels: &mut Panels, track: egui::Rangef) {
    let session = canvas.session();
    let document = session.document();
    let current = session.current_layer();
    let Some(layer) = document.layer(current) else {
        return;
    };
    let mut rows = Vec::new();
    for group in document.ancestors(current).unwrap_or_default() {
        if !group.keys().is_empty() {
            rows.push(Row::Keys(group.id));
        }
    }
    rows.push(Row::Keys(layer.id));
    if let Some(folder) = session.folder() {
        rows.push(Row::Cels(folder));
    }
    for row in rows {
        match row {
            Row::Keys(id) => keys_row(ui, canvas, panels, id, track),
            Row::Cels(id) => cels_row(ui, canvas, panels, id, track),
        }
    }
}

/// Allocates a row: its name on the left, the frames' track on the right.
fn row(ui: &mut egui::Ui, name: &str, track: egui::Rangef) -> egui::Rect {
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), ROW), egui::Sense::hover());
    let label = egui::Rect::from_x_y_ranges(rect.left()..=track.min - 8.0, rect.y_range());
    let galley = ui.painter().layout(
        name.to_owned(),
        egui::FontId::proportional(theme::SMALL),
        theme::muted(),
        label.width().max(1.0),
    );
    ui.painter().with_clip_rect(label).galley(
        egui::pos2(
            label.right() - galley.size().x.min(label.width()),
            label.center().y - galley.size().y / 2.0,
        ),
        galley,
        theme::muted(),
    );
    egui::Rect::from_x_y_ranges(track, rect.y_range())
}

/// The slot of each frame along `track`.
struct Slots {
    track: egui::Rect,
    frames: u32,
}

impl Slots {
    fn width(&self) -> f32 {
        self.track.width() / self.frames.max(1) as f32
    }

    fn at(&self, x: f32) -> u32 {
        (((x - self.track.left()) / self.width()).floor() as i64)
            .clamp(0, i64::from(self.frames) - 1) as u32
    }

    fn rect(&self, frame: u32) -> egui::Rect {
        egui::Rect::from_min_size(
            egui::pos2(
                self.track.left() + self.width() * frame as f32,
                self.track.top(),
            ),
            egui::vec2(self.width(), self.track.height()),
        )
    }

    /// The well behind the slots and the frame shown.
    fn paint(&self, painter: &egui::Painter, current: u32, focused: bool) {
        painter.rect(
            self.track,
            egui::CornerRadius::same(theme::radius::SMALL),
            theme::status(),
            egui::Stroke::new(
                1.0,
                if focused {
                    theme::accent_ink()
                } else {
                    theme::border()
                },
            ),
            egui::StrokeKind::Inside,
        );
        let shown = self.rect(current);
        painter.rect_filled(
            shown.shrink2(egui::vec2(0.0, 1.0)),
            egui::CornerRadius::same(theme::radius::SMALL),
            theme::control(),
        );
    }
}

/// Frames moved by the arrow keys while `response` has the focus.
fn stepped(ui: &egui::Ui, response: &egui::Response, current: u32, frames: u32) -> Option<u32> {
    if !response.has_focus() {
        return None;
    }
    let step = ui.input(|input| {
        i64::from(input.key_pressed(egui::Key::ArrowRight))
            - i64::from(input.key_pressed(egui::Key::ArrowLeft))
    });
    (step != 0).then(|| (i64::from(current) + step).rem_euclid(i64::from(frames)) as u32)
}

fn keys_row(
    ui: &mut egui::Ui,
    canvas: &mut Canvas,
    panels: &mut Panels,
    id: LayerId,
    track: egui::Rangef,
) {
    let session = canvas.session();
    let document = session.document();
    let Some(layer) = document.layer(id) else {
        return;
    };
    let frames = document.frames;
    let current = frame_in_cycle(session.frame(), frames);
    let keys: Vec<(u32, Ease)> = layer
        .keys()
        .keys
        .iter()
        .map(|key| (key.frame, key.ease))
        .collect();
    let name = layer.name.clone();
    let slots = Slots {
        track: row(ui, &layer.name, track).shrink2(egui::vec2(0.0, 1.0)),
        frames,
    };
    let response = ui.interact(
        slots.track,
        ui.id().with(("keys", id.0)),
        egui::Sense::click_and_drag(),
    );
    let count = keys.len();
    response.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::Slider,
            true,
            tr_with(
                "timeline-keys-name",
                &args([("layer", name.clone()), ("count", count.to_string())]),
            ),
        )
    });
    let painter = ui.painter();
    slots.paint(painter, current, response.has_focus());
    let dragged_from = ui.id().with(("key dragged", id.0));
    let dragging: Option<u32> = ui.data(|data| data.get_temp(dragged_from));
    for &(frame, ease) in &keys {
        let mut centre = slots.rect(frame).center();
        if dragging == Some(frame)
            && let Some(pointer) = response.interact_pointer_pos()
        {
            centre.x = slots.rect(slots.at(pointer.x)).center().x;
        }
        let radius = (slots.width() / 2.0 - 1.0).clamp(3.0, 6.0);
        let points = vec![
            centre + egui::vec2(0.0, -radius),
            centre + egui::vec2(radius, 0.0),
            centre + egui::vec2(0.0, radius),
            centre + egui::vec2(-radius, 0.0),
        ];
        let fill = if frame == current {
            theme::accent()
        } else {
            theme::text()
        };
        painter.add(egui::Shape::convex_polygon(
            points,
            fill,
            egui::Stroke::NONE,
        ));
        // A held key shows a bar to the next.
        if ease == Ease::Hold {
            painter.hline(
                centre.x + radius..=centre.x + radius + slots.width() / 2.0,
                centre.y,
                egui::Stroke::new(1.5, theme::muted()),
            );
        }
    }
    let key_under = |x: f32| {
        keys.iter()
            .map(|(frame, _)| *frame)
            .find(|frame| *frame == slots.at(x))
    };
    if response.drag_started()
        && let Some(pointer) = response.interact_pointer_pos()
        && let Some(frame) = key_under(pointer.x)
    {
        ui.data_mut(|data| data.insert_temp(dragged_from, frame));
    }
    if response.drag_stopped() {
        ui.data_mut(|data| data.remove::<u32>(dragged_from));
        if let (Some(from), Some(pointer)) = (
            dragging,
            response
                .interact_pointer_pos()
                .or(ui.input(|input| input.pointer.latest_pos())),
        ) {
            let to = slots.at(pointer.x);
            let result = canvas.edit(|session| session.move_key(id, from, to));
            panels.report(result);
            canvas.go_to_frame(to);
        }
    } else if (response.clicked() || (response.dragged() && dragging.is_none()))
        && let Some(pointer) = response.interact_pointer_pos()
    {
        let frame = slots.at(pointer.x);
        if frame != current {
            canvas.go_to_frame(frame);
        }
    } else if let Some(frame) = stepped(ui, &response, current, frames) {
        canvas.go_to_frame(frame);
    }
    // The key clicked with the other button, for its menu.
    let menu_key = ui.id().with(("key menu", id.0));
    if response.secondary_clicked()
        && let Some(pointer) = response.interact_pointer_pos()
    {
        let frame = key_under(pointer.x);
        ui.data_mut(|data| data.insert_temp(menu_key, frame));
    }
    response.context_menu(|ui| {
        let Some(Some(frame)) = ui.data(|data| data.get_temp::<Option<u32>>(menu_key)) else {
            ui.label(tr("timeline-no-key"));
            return;
        };
        let mut ease = keys
            .iter()
            .find(|(each, _)| *each == frame)
            .map_or(Ease::Linear, |(_, ease)| *ease);
        let before = ease;
        widgets::choice(ui, &mut ease, Ease::Linear, tr("ease-linear"));
        widgets::choice(ui, &mut ease, Ease::Smooth, tr("ease-smooth"));
        widgets::choice(ui, &mut ease, Ease::Hold, tr("ease-hold"));
        if ease != before {
            let result = canvas.edit(|session| session.set_ease(id, frame, ease));
            panels.report(result);
        }
        let width = widgets::menu_item_width(ui, tr("key-remove"), "");
        if widgets::menu_item(ui, width, tr("key-remove"), "", None, true).clicked() {
            let result = canvas.edit(|session| session.remove_key(id, frame));
            panels.report(result);
            ui.close();
        }
    });
}

/// The frames each exposure holds, as start and end slots, including those
/// before the first that the last holds round the loop.
fn spans(exposures: &[Exposure], frames: u32) -> Vec<(u32, u32, Option<LayerId>)> {
    let mut spans = Vec::new();
    for (index, exposure) in exposures.iter().enumerate() {
        let end = exposures.get(index + 1).map_or(frames, |next| next.frame);
        spans.push((exposure.frame, end, exposure.cel));
    }
    if let (Some(first), Some(last)) = (exposures.first(), exposures.last())
        && first.frame > 0
    {
        spans.push((0, first.frame, last.cel));
    }
    spans
}

fn cels_row(
    ui: &mut egui::Ui,
    canvas: &mut Canvas,
    panels: &mut Panels,
    folder: LayerId,
    track: egui::Rangef,
) {
    let session = canvas.session();
    let document = session.document();
    let Some(layer) = document.layer(folder) else {
        return;
    };
    let LayerKind::Group(group) = &layer.kind else {
        return;
    };
    let frames = document.frames;
    let current = frame_in_cycle(session.frame(), frames);
    let exposures = group.cels.clone().unwrap_or_default();
    let children: Vec<(LayerId, String)> = group
        .children
        .iter()
        .rev()
        .map(|child| (child.id, child.name.clone()))
        .collect();
    let name_of = |id: LayerId| {
        children
            .iter()
            .find(|(each, _)| *each == id)
            .map_or_else(String::new, |(_, name)| name.clone())
    };
    let slots = Slots {
        track: row(ui, &layer.name, track).shrink2(egui::vec2(0.0, 1.0)),
        frames,
    };
    let response = ui.interact(
        slots.track,
        ui.id().with(("cels", folder.0)),
        egui::Sense::click_and_drag(),
    );
    let shown = ugu_core::keys::exposed(&exposures, current).map(name_of);
    response.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::Slider,
            true,
            tr_with(
                "timeline-cels-name",
                &args([
                    ("folder", layer.name.clone()),
                    (
                        "cel",
                        shown.clone().unwrap_or_else(|| tr("cel-empty").to_owned()),
                    ),
                ]),
            ),
        )
    });
    let painter = ui.painter();
    slots.paint(painter, current, response.has_focus());
    for (start, end, cel) in spans(&exposures, frames) {
        let Some(cel) = cel else {
            continue;
        };
        let bar = egui::Rect::from_min_max(
            slots.rect(start).left_top() + egui::vec2(1.0, 3.0),
            slots.rect(end - 1).right_bottom() - egui::vec2(1.0, 3.0),
        );
        painter.rect(
            bar,
            egui::CornerRadius::same(theme::radius::SMALL),
            theme::accent().gamma_multiply(0.35),
            egui::Stroke::new(1.0, theme::accent_ink()),
            egui::StrokeKind::Inside,
        );
        painter.with_clip_rect(bar).text(
            bar.left_center() + egui::vec2(4.0, 0.0),
            egui::Align2::LEFT_CENTER,
            name_of(cel),
            egui::FontId::proportional(theme::SMALL),
            theme::text(),
        );
    }
    if (response.clicked() || response.dragged())
        && let Some(pointer) = response.interact_pointer_pos()
    {
        let frame = slots.at(pointer.x);
        if frame != current {
            canvas.go_to_frame(frame);
        }
    } else if let Some(frame) = stepped(ui, &response, current, frames) {
        canvas.go_to_frame(frame);
    }
    if response.secondary_clicked()
        && let Some(pointer) = response.interact_pointer_pos()
    {
        canvas.go_to_frame(slots.at(pointer.x));
    }
    response.context_menu(|ui| {
        let width = children
            .iter()
            .map(|(_, name)| widgets::menu_item_width(ui, name, ""))
            .chain([
                widgets::menu_item_width(ui, tr("cel-empty"), ""),
                widgets::menu_item_width(ui, tr("cel-new"), ""),
            ])
            .fold(widgets::MENU_WIDTH, f32::max);
        let exposed =
            ugu_core::keys::exposed(&exposures, frame_in_cycle(canvas.session().frame(), frames));
        for (id, name) in &children {
            if widgets::menu_item(ui, width, name, "", Some(exposed == Some(*id)), true).clicked() {
                let result = canvas.edit(|session| session.expose(Some(*id)));
                panels.report(result);
                ui.close();
            }
        }
        if widgets::menu_item(
            ui,
            width,
            tr("cel-empty"),
            "",
            Some(exposed.is_none()),
            true,
        )
        .clicked()
        {
            let result = canvas.edit(|session| session.expose(None));
            panels.report(result);
            ui.close();
        }
        if widgets::menu_item(ui, width, tr("cel-new"), "", None, true).clicked() {
            let result = canvas.edit(Session::new_cel);
            panels.report(result);
            ui.close();
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_exposure_holds_until_the_next_and_the_last_round_the_loop() {
        let cel = |id| Some(LayerId(id));
        let exposures = [
            Exposure {
                frame: 3,
                cel: cel(1),
            },
            Exposure {
                frame: 6,
                cel: None,
            },
            Exposure {
                frame: 8,
                cel: cel(2),
            },
        ];
        assert_eq!(
            spans(&exposures, 10),
            [
                (3, 6, cel(1)),
                (6, 8, None),
                (8, 10, cel(2)),
                (0, 3, cel(2))
            ]
        );
    }
}

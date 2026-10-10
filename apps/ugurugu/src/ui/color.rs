// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! The colour dock, after 2.2.13's `ColorWheel` and `ColorPairSwatch`: a
//! hue ring around a saturation and value square, and the current colour
//! over the previous one. 2.2.13's triangle field is left for later. Unlike
//! 2.2.13, whose wheel takes only the pointer, the colour's `#AARRGGBB` can
//! be typed, so that a colour can be chosen from the keyboard.

use egui::epaint::{Mesh, Vertex, WHITE_UV};
use egui::{Color32, CornerRadius, Pos2, Rect, Sense, Stroke, Ui, Vec2};
use ugu_core::ops::Rgba8;

use crate::canvas::Canvas;
use crate::i18n::{tr, tr_with};
use crate::theme;

use super::history::hex;

const MARGIN: f32 = 4.0;
const RING_GAP: f32 = 6.0;
const MARKER: f32 = 6.0;

#[derive(Default)]
pub struct ColorDock {
    /// Hue, saturation and value of the pen colour; kept here so that a grey
    /// leaves the ring where the artist left it.
    hsv: Option<[f32; 3]>,
    /// The colour before the current one; clicking it swaps the two.
    previous: Option<Rgba8>,
    /// The colour when the current drag began.
    dragged_from: Option<Rgba8>,
    /// Which part a drag started in.
    dragging_ring: bool,
    /// The pen colour when the dock was last shown.
    seen: Option<Rgba8>,
    /// The colour being typed.
    pub typing: Option<String>,
}

/// A colour typed as `#AARRGGBB`, or `#RRGGBB` keeping `alpha`; the `#` may
/// be left out.
fn parse_hex(text: &str, alpha: u8) -> Option<Rgba8> {
    let digits = text.trim().trim_start_matches('#');
    if !digits.chars().all(|digit| digit.is_ascii_hexdigit()) {
        return None;
    }
    let byte = |at: usize| u8::from_str_radix(digits.get(at..at + 2)?, 16).ok();
    match digits.len() {
        6 => Some(Rgba8([byte(0)?, byte(2)?, byte(4)?, alpha])),
        8 => Some(Rgba8([byte(2)?, byte(4)?, byte(6)?, byte(0)?])),
        _ => None,
    }
}

fn hsv_to_rgb([h, s, v]: [f32; 3]) -> [f32; 3] {
    let h = h.rem_euclid(1.0) * 6.0;
    let sector = h.floor();
    let f = h - sector;
    let (p, q, t) = (v * (1.0 - s), v * (1.0 - s * f), v * (1.0 - s * (1.0 - f)));
    match sector as u8 {
        0 => [v, t, p],
        1 => [q, v, p],
        2 => [p, v, t],
        3 => [p, q, v],
        4 => [t, p, v],
        _ => [v, p, q],
    }
}

fn rgb_to_hsv([r, g, b]: [f32; 3]) -> [f32; 3] {
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let delta = max - min;
    let hue = if delta == 0.0 {
        0.0
    } else if max == r {
        ((g - b) / delta).rem_euclid(6.0) / 6.0
    } else if max == g {
        ((b - r) / delta + 2.0) / 6.0
    } else {
        ((r - g) / delta + 4.0) / 6.0
    };
    [hue, if max == 0.0 { 0.0 } else { delta / max }, max]
}

fn color32([r, g, b]: [f32; 3]) -> Color32 {
    let byte = |value: f32| (value.clamp(0.0, 1.0) * 255.0).round() as u8;
    Color32::from_rgb(byte(r), byte(g), byte(b))
}

fn rgba(color: Rgba8) -> Color32 {
    let [r, g, b, a] = color.0;
    Color32::from_rgba_unmultiplied(r, g, b, a)
}

/// The point at `hue` on a circle of `radius`, counter-clockwise from the
/// right as a colour circle is drawn.
fn on_ring(center: Pos2, hue: f32, radius: f32) -> Pos2 {
    let angle = hue * std::f32::consts::TAU;
    center + egui::vec2(angle.cos() * radius, -angle.sin() * radius)
}

impl ColorDock {
    /// Makes the colour shown last the previous one when the pen colour was
    /// changed elsewhere: by the eyedropper, the history or a restore, as
    /// 2.2.13's pair follows every change of colour.
    fn follow(&mut self, color: Rgba8) {
        if let Some(seen) = self.seen
            && seen != color
            && self.dragged_from.is_none()
        {
            self.previous = Some(seen);
        }
        self.seen = Some(color);
    }

    pub fn show(&mut self, ui: &mut Ui, canvas: &mut Canvas) {
        let color = canvas.session().pen.color;
        self.follow(color);
        let [r, g, b, alpha] = color.0.map(|channel| f32::from(channel) / 255.0);
        let stored = self
            .hsv
            .filter(|hsv| color32(hsv_to_rgb(*hsv)) == color32([r, g, b]));
        let mut hsv = stored.unwrap_or_else(|| {
            let fresh = rgb_to_hsv([r, g, b]);
            // A grey keeps the last hue.
            match self.hsv {
                Some([hue, ..]) if fresh[1] == 0.0 => [hue, fresh[1], fresh[2]],
                _ => fresh,
            }
        });

        let side = ui.available_width().min(240.0);
        let (rect, response) = ui.allocate_exact_size(Vec2::splat(side), Sense::click_and_drag());
        let center = rect.center();
        let outer = side * 0.5 - MARGIN;
        let thickness = (side * 0.11).max(10.0);
        let inner = outer - thickness - RING_GAP;
        let half = inner / std::f32::consts::SQRT_2;
        let square = Rect::from_center_size(center, Vec2::splat(half * 2.0));
        let painter = ui.painter();

        let mut ring = Mesh::default();
        const SEGMENTS: u32 = 96;
        for step in 0..=SEGMENTS {
            let hue = step as f32 / SEGMENTS as f32;
            let color = color32(hsv_to_rgb([hue, 1.0, 1.0]));
            ring.vertices.push(Vertex {
                pos: on_ring(center, hue, outer),
                uv: WHITE_UV,
                color,
            });
            ring.vertices.push(Vertex {
                pos: on_ring(center, hue, outer - thickness),
                uv: WHITE_UV,
                color,
            });
            if step > 0 {
                let base = step * 2;
                ring.add_triangle(base - 2, base - 1, base);
                ring.add_triangle(base - 1, base + 1, base);
            }
        }
        painter.add(egui::Shape::mesh(ring));

        let mut field = Mesh::default();
        const CELLS: u32 = 16;
        for row in 0..=CELLS {
            for column in 0..=CELLS {
                let (s, v) = (
                    column as f32 / CELLS as f32,
                    1.0 - row as f32 / CELLS as f32,
                );
                let pos = square.min + egui::vec2(square.width() * s, square.height() * (1.0 - v));
                field.vertices.push(Vertex {
                    pos,
                    uv: WHITE_UV,
                    color: color32(hsv_to_rgb([hsv[0], s, v])),
                });
                if row > 0 && column > 0 {
                    let at = |r: u32, c: u32| r * (CELLS + 1) + c;
                    field.add_triangle(
                        at(row - 1, column - 1),
                        at(row - 1, column),
                        at(row, column - 1),
                    );
                    field.add_triangle(at(row - 1, column), at(row, column), at(row, column - 1));
                }
            }
        }
        painter.add(egui::Shape::mesh(field));

        if response.drag_started() || response.clicked() {
            self.dragged_from = Some(color);
            self.dragging_ring = response
                .interact_pointer_pos()
                .is_some_and(|pointer| pointer.distance(center) > inner + RING_GAP / 2.0);
        }
        if let Some(pointer) = response.interact_pointer_pos() {
            if self.dragging_ring {
                let offset = pointer - center;
                hsv[0] = (f32::atan2(-offset.y, offset.x) / std::f32::consts::TAU).rem_euclid(1.0);
            } else {
                hsv[1] = ((pointer.x - square.left()) / square.width()).clamp(0.0, 1.0);
                hsv[2] = (1.0 - (pointer.y - square.top()) / square.height()).clamp(0.0, 1.0);
            }
        }
        let marker = |at: Pos2| {
            painter.circle_stroke(at, MARKER, Stroke::new(3.0, Color32::from_black_alpha(170)));
            painter.circle_stroke(at, MARKER, Stroke::new(1.6, Color32::WHITE));
        };
        marker(on_ring(center, hsv[0], outer - thickness * 0.5));
        marker(egui::pos2(
            square.left() + square.width() * hsv[1],
            square.top() + square.height() * (1.0 - hsv[2]),
        ));
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Other, true, tr("color-current"))
        });

        self.hsv = Some(hsv);
        let [r, g, b] =
            hsv_to_rgb(hsv).map(|channel| (channel.clamp(0.0, 1.0) * 255.0).round() as u8);
        let picked = Rgba8([r, g, b, (alpha * 255.0).round() as u8]);
        if picked != color {
            canvas.edit(|session| session.pen.color = picked);
        }
        self.seen = Some(picked);
        if (response.drag_stopped() || response.clicked())
            && let Some(from) = self.dragged_from.take()
            && from != picked
        {
            self.previous = Some(from);
        }

        ui.add_space(6.0);
        ui.horizontal(|ui| {
            // The current colour over the previous one; the rear one swaps.
            let (pair, _) = ui.allocate_exact_size(egui::vec2(68.0, 58.0), Sense::hover());
            let back =
                Rect::from_min_size(pair.min + egui::vec2(24.0, 22.0), egui::vec2(40.0, 34.0));
            let front = Rect::from_min_size(pair.min, egui::vec2(48.0, 42.0));
            let previous = self.previous.unwrap_or(Rgba8([255, 255, 255, 255]));
            let back_response = ui
                .interact(back, ui.id().with("previous colour"), Sense::click())
                .on_hover_text(tr("color-pair-tip"));
            back_response.widget_info(|| {
                let names = [("current", hex(picked)), ("previous", hex(previous))];
                egui::WidgetInfo::labeled(
                    egui::WidgetType::Button,
                    true,
                    tr_with("color-pair", &super::args(names)),
                )
            });
            let painter = ui.painter();
            painter.rect(
                back,
                CornerRadius::same(6),
                rgba(previous),
                Stroke::new(1.0, theme::BORDER),
                egui::StrokeKind::Inside,
            );
            painter.rect(
                front,
                CornerRadius::same(6),
                rgba(picked),
                Stroke::new(3.0, theme::accent()),
                egui::StrokeKind::Inside,
            );
            if back_response.clicked() {
                self.previous = Some(picked);
                self.seen = Some(previous);
                canvas.edit(|session| session.pen.color = previous);
            }
            ui.vertical(|ui| {
                let label = ui.label(
                    egui::RichText::new(tr("color-current"))
                        .size(theme::SMALL)
                        .color(theme::MUTED),
                );
                if let Some(typed) = self.hex_field(ui, picked, label.id)
                    && typed != picked
                {
                    self.previous = Some(picked);
                    self.seen = Some(typed);
                    canvas.edit(|session| session.pen.color = typed);
                }
            });
        });
    }

    /// The colour as text, which takes a typed colour when Enter or Tab
    /// leaves it. Escape, or text that is not a colour, leaves the colour.
    fn hex_field(&mut self, ui: &mut Ui, picked: Rgba8, label: egui::Id) -> Option<Rgba8> {
        let id = ui.id().with("colour hex");
        let mut text = match &self.typing {
            Some(text) if ui.memory(|memory| memory.has_focus(id)) => text.clone(),
            _ => hex(picked),
        };
        let response = ui
            .add(
                egui::TextEdit::singleline(&mut text)
                    .id(id)
                    .desired_width(84.0)
                    .char_limit(9),
            )
            .labelled_by(label);
        if response.has_focus() {
            self.typing = Some(text);
            return None;
        }
        let typed = self.typing.take();
        if !response.lost_focus() || ui.input(|input| input.key_pressed(egui::Key::Escape)) {
            return None;
        }
        parse_hex(&typed?, picked.0[3])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hsv_round_trips_through_rgb() {
        for rgb in [
            [1.0, 0.0, 0.0],
            [0.2, 0.6, 0.4],
            [0.5, 0.5, 0.5],
            [0.0, 0.0, 0.0],
            [0.9, 0.1, 0.8],
        ] {
            let back = hsv_to_rgb(rgb_to_hsv(rgb));
            for axis in 0..3 {
                assert!((back[axis] - rgb[axis]).abs() < 1e-5, "{rgb:?} -> {back:?}");
            }
        }
    }

    #[test]
    fn a_colour_changed_elsewhere_makes_the_last_one_previous() {
        let (red, blue, green) = (
            Rgba8([255, 0, 0, 255]),
            Rgba8([0, 0, 255, 255]),
            Rgba8([0, 255, 0, 255]),
        );
        let mut dock = ColorDock::default();
        dock.follow(red);
        assert_eq!(dock.previous, None);
        dock.follow(red);
        assert_eq!(dock.previous, None);
        // The eyedropper picked blue.
        dock.follow(blue);
        assert_eq!(dock.previous, Some(red));
        // A drag on the wheel sets the previous colour when it ends.
        dock.dragged_from = Some(blue);
        dock.follow(green);
        assert_eq!(dock.previous, Some(red));
    }

    #[test]
    fn a_typed_colour_reads_as_2_2_13_names_it() {
        assert_eq!(parse_hex("#80FF0000", 255), Some(Rgba8([255, 0, 0, 128])));
        assert_eq!(parse_hex(" 00ff7f ", 64), Some(Rgba8([0, 255, 127, 64])));
        for wrong in ["", "#", "#12345", "#1234567", "#GG0000", "#+1FF00"] {
            assert_eq!(parse_hex(wrong, 255), None, "{wrong}");
        }
    }
}

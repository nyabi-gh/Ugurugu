// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! The colour dock, after 2.2.13's `ColorWheel` and `ColorPairSwatch`: a
//! hue ring around a saturation and value square or triangle, and the
//! current colour over the previous one. Unlike
//! 2.2.13, whose wheel takes only the pointer, the colour's `#AARRGGBB` can
//! be typed, so that a colour can be chosen from the keyboard.

use egui::epaint::{Mesh, Vertex, WHITE_UV};
use egui::{Color32, CornerRadius, Pos2, Rect, Sense, Stroke, Ui, Vec2};
use ugu_core::ops::Rgba8;

use crate::canvas::Canvas;
use crate::i18n::{tr, tr_with};
use crate::layout::ColorField;
use crate::{theme, widgets};

use super::history::hex;

const MARGIN: f32 = 4.0;
const RING_GAP: f32 = 6.0;
const MARKER: f32 = 6.0;
/// The wheel's side at most, and the least a short dock makes it.
const WHEEL_MOST: f32 = 240.0;
const WHEEL_LEAST: f32 = 120.0;
/// The panel apart from the wheel, until it has been drawn.
const REST_GUESS: f32 = 140.0;

#[derive(Default)]
pub struct ColorDock {
    /// Hue, saturation and value of the pen colour; kept here so that a grey
    /// leaves the ring where the artist left it.
    hsv: Option<[f32; 3]>,
    /// The colour before the current one; clicking it swaps the two.
    previous: Option<Rgba8>,
    /// The colour when the current press began.
    dragged_from: Option<Rgba8>,
    /// The pen colour when the dock was last shown.
    seen: Option<Rgba8>,
    /// The colour being typed.
    pub typing: Option<String>,
    /// How tall the panel was apart from the wheel, when last shown.
    rest: Option<f32>,
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

/// The triangle's corners inside a ring of `inner` radius: the hue at the
/// right, white at the top left and black at the bottom left, as 2.2.13's.
fn triangle(center: Pos2, inner: f32) -> [Pos2; 3] {
    let up = inner * 3f32.sqrt() * 0.5;
    [
        center + egui::vec2(inner, 0.0),
        center + egui::vec2(-inner * 0.5, -up),
        center + egui::vec2(-inner * 0.5, up),
    ]
}

/// How much of the hue, white and black corners make saturation `s` and
/// value `v`: a tint is hue and white, a shade takes black.
fn weights(s: f32, v: f32) -> [f32; 3] {
    [v * s, v * (1.0 - s), 1.0 - v]
}

/// Saturation and value at `point`, slid onto the triangle when outside it
/// so that a drag past an edge follows the edge.
fn saturation_value(point: Pos2, [a, b, c]: [Pos2; 3]) -> [f32; 2] {
    let denominator = (b.y - c.y) * (a.x - c.x) + (c.x - b.x) * (a.y - c.y);
    let first = ((b.y - c.y) * (point.x - c.x) + (c.x - b.x) * (point.y - c.y)) / denominator;
    let second = ((c.y - a.y) * (point.x - c.x) + (a.x - c.x) * (point.y - c.y)) / denominator;
    let clamped = [first, second, 1.0 - first - second].map(|weight| weight.max(0.0));
    let total: f32 = clamped.iter().sum();
    let [hue, white, _] = clamped.map(|weight| weight / total);
    let value = (hue + white).clamp(0.0, 1.0);
    let saturation = if value > 0.0 {
        (hue / value).clamp(0.0, 1.0)
    } else {
        0.0
    };
    [saturation, value]
}

fn blend(corners: [Pos2; 3], weights: [f32; 3]) -> Pos2 {
    let mut point = Pos2::ZERO;
    for (corner, weight) in corners.iter().zip(weights) {
        point += corner.to_vec2() * weight;
    }
    point
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

    /// Swaps the pen colour and the previous one, as clicking the rear
    /// swatch does; white is the previous colour before there is one.
    pub fn swap(&mut self, canvas: &mut Canvas) {
        let picked = canvas.tools().color;
        self.follow(picked);
        let previous = self.previous.unwrap_or(Rgba8([255, 255, 255, 255]));
        self.previous = Some(picked);
        self.seen = Some(previous);
        canvas.tools_mut().color = previous;
    }

    /// How tall the panel is with the wheel as wide as `width` allows.
    pub fn natural_height(&self, width: f32) -> f32 {
        self.rest.unwrap_or(REST_GUESS) + width.min(WHEEL_MOST)
    }

    /// How tall the panel is with the wheel at its least.
    pub fn least_height(&self, width: f32) -> f32 {
        self.rest.unwrap_or(REST_GUESS) + width.min(WHEEL_LEAST)
    }

    /// Shows the panel; given the `room` it has, the wheel takes what the
    /// rest of the panel leaves, down to its least.
    pub fn show(
        &mut self,
        ui: &mut Ui,
        canvas: &mut Canvas,
        shape: &mut ColorField,
        room: Option<f32>,
    ) {
        let top = ui.cursor().min.y;
        let color = canvas.tools().color;
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

        let widest = ui.available_width().min(WHEEL_MOST);
        let side = match (room, self.rest) {
            (Some(room), Some(rest)) => (room - rest).clamp(widest.min(WHEEL_LEAST), widest),
            _ => widest,
        };
        let (rect, response) = ui.allocate_exact_size(Vec2::splat(side), Sense::click_and_drag());
        let center = rect.center();
        let outer = side * 0.5 - MARGIN;
        let thickness = (side * 0.11).max(10.0);
        let inner = outer - thickness - RING_GAP;
        let half = inner / std::f32::consts::SQRT_2;
        let square = Rect::from_center_size(center, Vec2::splat(half * 2.0));
        let corners = triangle(center, inner);
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
        if *shape == ColorField::Triangle {
            // Rows from the hue corner, each colour exact at its vertex.
            let mut first_of_row = Vec::new();
            for a in 0..=CELLS {
                first_of_row.push(field.vertices.len() as u32);
                for b in 0..=CELLS - a {
                    let (hue, white) = (a as f32 / CELLS as f32, b as f32 / CELLS as f32);
                    let value = hue + white;
                    let saturation = if value > 0.0 { hue / value } else { 0.0 };
                    field.vertices.push(Vertex {
                        pos: blend(corners, [hue, white, 1.0 - value]),
                        uv: WHITE_UV,
                        color: color32(hsv_to_rgb([hsv[0], saturation, value])),
                    });
                }
            }
            for a in 0..CELLS {
                let (row, next) = (first_of_row[a as usize], first_of_row[a as usize + 1]);
                for b in 0..CELLS - a {
                    field.add_triangle(row + b, row + b + 1, next + b);
                    if b + 1 < CELLS - a {
                        field.add_triangle(row + b + 1, next + b + 1, next + b);
                    }
                }
            }
        }
        for row in (0..=CELLS).filter(|_| *shape == ColorField::Square) {
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

        // The part pressed is where the press began, from its first frame:
        // egui tells a drag or a click only after the pointer has moved or
        // been let go, and the colour follows the pointer before that.
        let pressed = response.is_pointer_button_down_on();
        if pressed && self.dragged_from.is_none() {
            self.dragged_from = Some(color);
        }
        let origin = ui.input(|input| input.pointer.press_origin());
        if let (true, Some(pointer), Some(origin)) =
            (pressed, response.interact_pointer_pos(), origin)
        {
            if origin.distance(center) > inner + RING_GAP / 2.0 {
                let offset = pointer - center;
                hsv[0] = (f32::atan2(-offset.y, offset.x) / std::f32::consts::TAU).rem_euclid(1.0);
            } else if *shape == ColorField::Triangle {
                [hsv[1], hsv[2]] = saturation_value(pointer, corners);
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
        marker(match shape {
            ColorField::Square => egui::pos2(
                square.left() + square.width() * hsv[1],
                square.top() + square.height() * (1.0 - hsv[2]),
            ),
            ColorField::Triangle => blend(corners, weights(hsv[1], hsv[2])),
        });
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Other, true, tr("color-current"))
        });

        self.hsv = Some(hsv);
        let [r, g, b] =
            hsv_to_rgb(hsv).map(|channel| (channel.clamp(0.0, 1.0) * 255.0).round() as u8);
        let picked = Rgba8([r, g, b, (alpha * 255.0).round() as u8]);
        if picked != color {
            canvas.tools_mut().color = picked;
        }
        self.seen = Some(picked);
        if !pressed
            && let Some(from) = self.dragged_from.take()
            && from != picked
        {
            self.previous = Some(from);
        }

        ui.add_space(theme::space::S);
        widgets::segmented(
            ui,
            shape,
            &[
                (ColorField::Square, tr("color-field-square")),
                (ColorField::Triangle, tr("color-field-triangle")),
            ],
            2,
        );
        ui.add_space(theme::space::S);
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
                CornerRadius::same(theme::radius::CONTROL),
                rgba(previous),
                Stroke::new(1.0, theme::border()),
                egui::StrokeKind::Inside,
            );
            painter.rect(
                front,
                CornerRadius::same(theme::radius::CONTROL),
                rgba(picked),
                Stroke::new(3.0, theme::accent_ink()),
                egui::StrokeKind::Inside,
            );
            if back_response.has_focus() {
                crate::widgets::focus_ring(painter, back, theme::radius::CONTROL);
            }
            if back_response.clicked() {
                self.swap(canvas);
            }
            ui.vertical(|ui| {
                let label = ui.label(
                    egui::RichText::new(tr("color-current"))
                        .size(theme::SMALL)
                        .color(theme::muted()),
                );
                if let Some(typed) = self.hex_field(ui, picked, label.id)
                    && typed != picked
                {
                    self.previous = Some(picked);
                    self.seen = Some(typed);
                    canvas.tools_mut().color = typed;
                }
            });
        });
        let rest = ui.cursor().min.y - top - side;
        if self.rest.is_none_or(|before| (before - rest).abs() > 0.5) {
            self.rest = Some(rest);
            ui.ctx().request_repaint();
        }
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
    fn the_triangle_gives_saturation_and_value_as_2_2_13_s() {
        let corners = triangle(Pos2::new(100.0, 100.0), 60.0);
        let close = |[s, v]: [f32; 2], [ws, wv]: [f32; 2]| {
            assert!((s - ws).abs() < 1e-4 && (v - wv).abs() < 1e-4, "{s} {v}");
        };
        close(saturation_value(corners[0], corners), [1.0, 1.0]);
        close(saturation_value(corners[1], corners), [0.0, 1.0]);
        // Black has no saturation to speak of.
        assert!(saturation_value(corners[2], corners)[1].abs() < 1e-4);
        for [s, v] in [[0.3, 0.8], [0.9, 0.2], [0.5, 0.5]] {
            close(
                saturation_value(blend(corners, weights(s, v)), corners),
                [s, v],
            );
        }
        // Past the hue corner the drag stays on it.
        close(
            saturation_value(corners[0] + egui::vec2(40.0, 0.0), corners),
            [1.0, 1.0],
        );
    }

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

    /// The dock alone in a window, pressed and dragged as a mouse does.
    struct Wheel {
        ctx: egui::Context,
        canvas: Canvas,
        dock: ColorDock,
        shape: ColorField,
        area: Rect,
    }

    impl Wheel {
        fn new(color: Rgba8) -> Self {
            let mut canvas = Canvas::new(ugu_core::document::Document::new([8, 8]), |_| {});
            canvas.tools_mut().color = color;
            let mut wheel = Self {
                ctx: egui::Context::default(),
                canvas,
                dock: ColorDock::default(),
                shape: ColorField::Square,
                area: Rect::NOTHING,
            };
            wheel.frame(Vec::new());
            wheel
        }

        fn frame(&mut self, events: Vec<egui::Event>) {
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(400.0, 600.0))),
                events,
                ..egui::RawInput::default()
            };
            let Self {
                ctx,
                canvas,
                dock,
                shape,
                area,
            } = self;
            let mut output = ctx.run_ui(input, |ui| {
                egui::CentralPanel::default().show(ui, |ui| {
                    *area = Rect::from_min_size(
                        ui.cursor().min,
                        Vec2::splat(ui.available_width().min(WHEEL_MOST)),
                    );
                    dock.show(ui, canvas, shape, None);
                });
            });
            output.textures_delta.clear();
        }

        fn button(at: Pos2, pressed: bool) -> egui::Event {
            egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            }
        }

        /// Presses at `from`, moves to `to` in steps and lets go.
        fn drag(&mut self, from: Pos2, to: Pos2) {
            self.frame(vec![egui::Event::PointerMoved(from)]);
            self.frame(vec![Self::button(from, true)]);
            for step in 1..=4 {
                let at = from + (to - from) * (step as f32 / 4.0);
                self.frame(vec![egui::Event::PointerMoved(at)]);
            }
            self.frame(vec![Self::button(to, false)]);
            self.frame(Vec::new());
        }

        /// Presses and lets go at `at`, a frame apart, without moving.
        fn click(&mut self, at: Pos2) {
            self.frame(vec![egui::Event::PointerMoved(at)]);
            self.frame(vec![Self::button(at, true)]);
            self.frame(Vec::new());
            self.frame(vec![Self::button(at, false)]);
            self.frame(Vec::new());
        }

        fn hsv(&self) -> [f32; 3] {
            let [r, g, b, _] = self
                .canvas
                .tools()
                .color
                .0
                .map(|channel| f32::from(channel) / 255.0);
            rgb_to_hsv([r, g, b])
        }
    }

    #[test]
    fn pressing_the_square_after_the_ring_keeps_the_hue_and_the_other_way_round() {
        let mut wheel = Wheel::new(Rgba8([255, 200, 0, 255]));
        let center = wheel.area.center();
        let side = wheel.area.width();
        let outer = side * 0.5 - MARGIN;
        let thickness = (side * 0.11).max(10.0);
        let ring = outer - thickness * 0.5;
        // Cyan, on the left of the ring.
        wheel.drag(
            center + Vec2::new(-ring, 10.0),
            center + Vec2::new(-ring, 0.0),
        );
        let hue = wheel.hsv()[0];
        assert!((hue - 0.5).abs() < 0.02, "{hue}");
        // Near the square's top right, where the ring's angle is orange.
        let corner = (outer - thickness - RING_GAP) / std::f32::consts::SQRT_2;
        let cyan = wheel.canvas.tools().color;
        wheel.click(center + Vec2::new(corner - 6.0, -(corner - 6.0)));
        assert_eq!(wheel.dock.previous, Some(cyan));
        let [hue, saturation, value] = wheel.hsv();
        assert!(
            (hue - 0.5).abs() < 0.02,
            "the square took the ring's hue: {hue}"
        );
        assert!(saturation > 0.9 && value > 0.9, "{saturation} {value}");
        // Back on the ring, at the top: green, keeping saturation and value.
        wheel.click(center + Vec2::new(0.0, -ring));
        let [hue, saturation, value] = wheel.hsv();
        assert!((hue - 0.25).abs() < 0.02, "{hue}");
        assert!(
            saturation > 0.9 && value > 0.9,
            "the ring moved the square: {saturation} {value}"
        );
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

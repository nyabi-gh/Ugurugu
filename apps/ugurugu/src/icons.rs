// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! The interface glyphs of 2.2.13 (`Icons.cpp`): lines on a 24-unit grid,
//! each nudged by its own seeded wobble so they look drawn by hand. Only the
//! glyphs 3.0 uses so far are here; each keeps its 2.2.13 number, which
//! seeds its wobble.

use std::f64::consts::PI;

use egui::{Color32, Pos2, Rect, Shape, Stroke};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Glyph {
    Brush = 0,
    Eraser = 1,
    Undo = 2,
    Redo = 3,
    Play = 4,
    Pause = 5,
    Add = 6,
    Duplicate = 7,
    Remove = 9,
    MoveUp = 10,
    MoveDown = 11,
    EyeOpen = 12,
    EyeClosed = 13,
    FitView = 14,
    MirrorHorizontal = 15,
    Lasso = 16,
    Wand = 17,
    Bucket = 18,
    Wobble = 21,
    Panels = 22,
    Scale = 25,
    MirrorVertical = 27,
    Delete = 28,
    Deselect = 29,
    Confirm = 30,
    Cancel = 31,
}

type Points = Vec<[f64; 2]>;

#[derive(Default)]
struct Shapes {
    lines: Vec<Points>,
    fills: Vec<Points>,
}

fn quad(start: [f64; 2], control: [f64; 2], end: [f64; 2], steps: u32) -> Points {
    (0..=steps)
        .map(|step| {
            let t = f64::from(step) / f64::from(steps);
            let u = 1.0 - t;
            std::array::from_fn(|axis| {
                start[axis] * u * u + control[axis] * 2.0 * u * t + end[axis] * t * t
            })
        })
        .collect()
}

fn circle(center: [f64; 2], radius: f64, steps: u32) -> Points {
    (0..=steps)
        .map(|step| {
            let angle = 2.0 * PI * f64::from(step) / f64::from(steps);
            [
                center[0] + radius * angle.cos(),
                center[1] + radius * angle.sin(),
            ]
        })
        .collect()
}

fn direction(degrees: f64) -> [f64; 2] {
    let radians = degrees.to_radians();
    [radians.cos(), -radians.sin()]
}

fn arrow_head(shapes: &mut Shapes, tip: [f64; 2], travel: f64, length: f64) {
    let [left, right] = [travel + 180.0 - 32.0, travel + 180.0 + 32.0].map(direction);
    let at = |side: [f64; 2]| [tip[0] + side[0] * length, tip[1] + side[1] * length];
    shapes.lines.push(vec![at(left), tip]);
    shapes.lines.push(vec![tip, at(right)]);
}

fn shapes(glyph: Glyph) -> Shapes {
    let mut shapes = Shapes::default();
    let lines = &mut shapes.lines;
    match glyph {
        Glyph::Brush => {
            lines.push(vec![[18.6, 3.6], [11.4, 10.8]]);
            shapes.fills.push(vec![
                [11.4, 10.8],
                [13.0, 12.8],
                [10.6, 16.4],
                [6.6, 18.8],
                [4.8, 17.2],
                [7.6, 13.4],
            ]);
        }
        Glyph::Eraser => {
            lines.push(vec![
                [14.6, 4.4],
                [19.8, 9.6],
                [11.6, 17.6],
                [6.4, 12.4],
                [14.6, 4.4],
            ]);
            lines.push(vec![[9.2, 9.7], [14.4, 14.9]]);
            lines.push(vec![[5.2, 20.4], [15.4, 20.4]]);
        }
        Glyph::Undo => {
            lines.push(quad([19.0, 15.6], [17.4, 5.6], [6.4, 9.6], 22));
            arrow_head(&mut shapes, [6.4, 9.6], 199.0, 4.4);
        }
        Glyph::Redo => {
            lines.push(quad([5.0, 15.6], [6.6, 5.6], [17.6, 9.6], 22));
            arrow_head(&mut shapes, [17.6, 9.6], -19.0, 4.4);
        }
        Glyph::Play => shapes
            .fills
            .push(vec![[8.6, 5.4], [19.2, 12.0], [8.6, 18.6]]),
        Glyph::Pause => {
            shapes
                .fills
                .push(vec![[7.2, 5.6], [10.4, 5.6], [10.4, 18.4], [7.2, 18.4]]);
            shapes
                .fills
                .push(vec![[13.6, 5.6], [16.8, 5.6], [16.8, 18.4], [13.6, 18.4]]);
        }
        Glyph::Add => {
            lines.push(vec![[12.0, 5.4], [12.0, 18.6]]);
            lines.push(vec![[5.4, 12.0], [18.6, 12.0]]);
        }
        Glyph::Duplicate => {
            lines.push(vec![[8.6, 6.4], [19.2, 6.4], [19.2, 15.2]]);
            lines.push(vec![
                [4.8, 9.0],
                [15.2, 9.0],
                [15.2, 19.4],
                [4.8, 19.4],
                [4.8, 9.0],
            ]);
        }
        Glyph::Remove => lines.push(vec![[6.0, 12.0], [18.0, 12.0]]),
        Glyph::MoveUp => lines.push(vec![[6.4, 14.6], [12.0, 8.6], [17.6, 14.6]]),
        Glyph::MoveDown => lines.push(vec![[6.4, 9.4], [12.0, 15.4], [17.6, 9.4]]),
        Glyph::EyeOpen => {
            let mut outline = quad([3.8, 12.0], [12.0, 5.2], [20.2, 12.0], 14);
            outline.extend(quad([20.2, 12.0], [12.0, 18.8], [3.8, 12.0], 14));
            lines.push(outline);
            shapes.fills.push(circle([12.0, 12.0], 2.5, 18));
        }
        Glyph::EyeClosed => {
            lines.push(quad([3.8, 12.0], [12.0, 18.8], [20.2, 12.0], 16));
            lines.push(vec![[7.0, 15.7], [5.6, 18.4]]);
            lines.push(vec![[12.0, 17.4], [12.0, 20.2]]);
            lines.push(vec![[17.0, 15.7], [18.4, 18.4]]);
        }
        Glyph::FitView => {
            lines.push(vec![[4.6, 9.2], [4.6, 4.6], [9.2, 4.6]]);
            lines.push(vec![[14.8, 4.6], [19.4, 4.6], [19.4, 9.2]]);
            lines.push(vec![[19.4, 14.8], [19.4, 19.4], [14.8, 19.4]]);
            lines.push(vec![[9.2, 19.4], [4.6, 19.4], [4.6, 14.8]]);
        }
        Glyph::MirrorHorizontal => {
            lines.push(vec![[12.0, 3.8], [12.0, 20.2]]);
            lines.push(vec![[10.0, 6.0], [4.2, 12.0], [10.0, 18.0], [10.0, 6.0]]);
            lines.push(vec![[14.0, 6.0], [19.8, 12.0], [14.0, 18.0], [14.0, 6.0]]);
        }
        Glyph::Lasso => {
            lines.push(
                (0..=26)
                    .map(|step| {
                        let angle = std::f64::consts::TAU * f64::from(step) / 26.0;
                        [12.0 + 7.2 * angle.cos(), 9.8 + 5.4 * angle.sin()]
                    })
                    .collect(),
            );
            lines.push(quad([13.6, 15.0], [10.0, 17.4], [6.4, 19.6], 10));
            lines.push(vec![[6.4, 19.6], [9.2, 20.8]]);
        }
        Glyph::Wand => {
            lines.push(vec![[15.8, 8.2], [6.6, 17.4]]);
            lines.push(vec![[18.4, 2.8], [18.4, 5.4]]);
            lines.push(vec![[18.4, 7.4], [18.4, 10.0]]);
            lines.push(vec![[15.0, 6.4], [17.4, 6.4]]);
            lines.push(vec![[19.4, 6.4], [21.8, 6.4]]);
            shapes.fills.push(circle([20.6, 11.6], 1.0, 10));
        }
        Glyph::Bucket => {
            lines.push(quad([7.2, 9.6], [12.0, 3.6], [16.8, 9.6], 12));
            lines.push(vec![[5.8, 9.6], [18.2, 9.6]]);
            lines.push(vec![[6.8, 9.6], [8.4, 19.2], [15.6, 19.2], [17.2, 9.6]]);
            shapes.fills.push(circle([20.2, 14.4], 1.4, 12));
        }
        Glyph::Wobble => {
            // The shape the wobble preview animates, caught mid-wobble.
            const STEPS: u32 = 40;
            lines.push(
                (0..=STEPS)
                    .map(|step| {
                        let position = f64::from(step) / f64::from(STEPS);
                        let turn = 2.0 * PI * position;
                        let offset = 0.62 * (1.25 * turn).sin() + 0.38 * (2.75 * turn + 0.9).sin();
                        [3.6 + 16.8 * position, 12.0 - 5.9 * offset]
                    })
                    .collect(),
            );
        }
        Glyph::Panels => {
            lines.push(vec![
                [4.4, 4.8],
                [19.6, 4.8],
                [19.6, 19.2],
                [4.4, 19.2],
                [4.4, 4.8],
            ]);
            lines.push(vec![[13.2, 4.8], [13.2, 19.2]]);
            lines.push(vec![[13.2, 12.0], [19.6, 12.0]]);
        }
        Glyph::Scale => {
            lines.push(vec![[6.0, 18.0], [18.0, 6.0]]);
            arrow_head(&mut shapes, [18.0, 6.0], 45.0, 4.0);
            arrow_head(&mut shapes, [6.0, 18.0], 225.0, 4.0);
            shapes.lines.push(vec![[4.2, 9.0], [4.2, 4.2], [9.0, 4.2]]);
            shapes
                .lines
                .push(vec![[15.0, 19.8], [19.8, 19.8], [19.8, 15.0]]);
        }
        Glyph::MirrorVertical => {
            lines.push(vec![[3.8, 12.0], [20.2, 12.0]]);
            lines.push(vec![[6.0, 10.0], [12.0, 4.2], [18.0, 10.0], [6.0, 10.0]]);
            lines.push(vec![[6.0, 14.0], [12.0, 19.8], [18.0, 14.0], [6.0, 14.0]]);
        }
        Glyph::Delete => {
            lines.push(vec![[5.0, 7.2], [19.0, 7.2]]);
            lines.push(vec![[9.0, 4.6], [15.0, 4.6]]);
            lines.push(vec![[7.2, 8.8], [8.2, 19.6], [15.8, 19.6], [16.8, 8.8]]);
            lines.push(vec![[10.4, 10.2], [10.8, 17.4]]);
            lines.push(vec![[13.6, 10.2], [13.2, 17.4]]);
        }
        Glyph::Deselect => {
            lines.push(vec![[4.2, 9.0], [4.2, 4.2], [9.0, 4.2]]);
            lines.push(vec![[15.0, 4.2], [19.8, 4.2], [19.8, 9.0]]);
            lines.push(vec![[4.2, 15.0], [4.2, 19.8], [9.0, 19.8]]);
            lines.push(vec![[15.0, 19.8], [19.8, 19.8], [19.8, 15.0]]);
            lines.push(vec![[8.4, 8.4], [15.6, 15.6]]);
            lines.push(vec![[15.6, 8.4], [8.4, 15.6]]);
        }
        Glyph::Confirm => lines.push(vec![[4.8, 12.4], [9.6, 17.2], [19.4, 6.8]]),
        Glyph::Cancel => {
            lines.push(vec![[6.0, 6.0], [18.0, 18.0]]);
            lines.push(vec![[18.0, 6.0], [6.0, 18.0]]);
        }
    }
    shapes
}

fn mix_seed(value: u64) -> u64 {
    let mut value = value.wrapping_add(0x9e37_79b9_7f4a_7c15);
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^ (value >> 31)
}

fn seeded_angle(seed: u64, channel: u64) -> f64 {
    (mix_seed(seed.wrapping_add(channel)) % 6283) as f64 / 1000.0
}

fn densify(source: &[[f64; 2]], step: f64) -> Points {
    let Some(first) = source.first() else {
        return Vec::new();
    };
    let mut result = vec![*first];
    for pair in source.windows(2) {
        let [from, to] = [pair[0], pair[1]];
        let length = (to[0] - from[0]).hypot(to[1] - from[1]);
        let pieces = ((length / step) as usize).max(1);
        for piece in 1..=pieces {
            let t = piece as f64 / pieces as f64;
            result.push([
                from[0] + (to[0] - from[0]) * t,
                from[1] + (to[1] - from[1]) * t,
            ]);
        }
    }
    result
}

fn wobbled(source: &[[f64; 2]], seed: u64, amplitude: f64, phase: f64) -> Points {
    let dense = densify(source, 1.8);
    let a1 = seeded_angle(seed, 1) + phase;
    let a2 = seeded_angle(seed, 2) + phase * 1.4;
    let a3 = seeded_angle(seed, 3) + phase * 0.8;
    let a4 = seeded_angle(seed, 4) + phase * 1.2;
    let mut travelled = 0.0;
    dense
        .iter()
        .enumerate()
        .map(|(index, point)| {
            if index > 0 {
                let before = dense[index - 1];
                travelled += (point[0] - before[0]).hypot(point[1] - before[1]);
            }
            let dx = amplitude
                * (0.62 * (travelled * 0.55 + a1).sin() + 0.38 * (travelled * 1.35 + a2).sin());
            let dy = amplitude
                * (0.62 * (travelled * 0.62 + a3).sin() + 0.38 * (travelled * 1.21 + a4).sin());
            [point[0] + dx, point[1] + dy]
        })
        .collect()
}

/// Draws `glyph` filling the square `rect` in `color`; `phase` moves its
/// wobble, 0 at rest.
pub fn paint(painter: &egui::Painter, rect: Rect, glyph: Glyph, color: Color32, phase: f64) {
    let scale = rect.width() / 24.0;
    let to_screen = |points: Points| -> Vec<Pos2> {
        points
            .into_iter()
            .map(|[x, y]| rect.min + egui::vec2(x as f32 * scale, y as f32 * scale))
            .collect()
    };
    let seed = (glyph as u64).wrapping_mul(0x51ED_2701).wrapping_add(7);
    let shapes = shapes(glyph);
    for (index, line) in shapes.lines.iter().enumerate() {
        let points = to_screen(wobbled(line, seed + index as u64 * 131, 0.5, phase));
        // Round caps, as the 2.2.13 pen draws them.
        for end in [points.first(), points.last()].into_iter().flatten() {
            painter.circle_filled(*end, scale, color);
        }
        painter.add(Shape::line(points, Stroke::new(2.0 * scale, color)));
    }
    for (index, fill) in shapes.fills.iter().enumerate() {
        let points = to_screen(wobbled(fill, seed + 977 + index as u64 * 131, 0.5, phase));
        painter.add(Shape::convex_polygon(
            points,
            color,
            Stroke::new(1.4 * scale, color),
        ));
    }
}

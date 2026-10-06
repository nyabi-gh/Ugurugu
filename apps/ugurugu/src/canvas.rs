// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! M0 probe canvas: raw pointer strokes drawn with the document renderer, so
//! input handling and the display path can be checked end to end.

use ugu_render::raster::{CanvasRaster, StrokeStyle};
use ugu_win::pointer::{PointerKind, PointerSample};

use crate::input::CanvasInput;

/// The paper behind the strokes, opaque straight RGBA.
pub const PAPER: [u8; 4] = [245, 245, 245, 255];

struct Stroke {
    style: StrokeStyle,
    /// Physical pixels relative to the canvas origin.
    points: Vec<[f32; 2]>,
}

fn style(kind: PointerKind, pixels_per_point: f32) -> StrokeStyle {
    StrokeStyle {
        width: 2.0 * pixels_per_point,
        color: match kind {
            PointerKind::Pen => [20, 60, 160, 255],
            _ => [20, 20, 20, 255],
        },
    }
}

pub struct ProbeCanvas {
    strokes: Vec<Stroke>,
    live: Option<Stroke>,
    raster: CanvasRaster,
    /// Canvas area in client physical pixels: left, top, right, bottom.
    area: Option<[i32; 4]>,
    pixels_per_point: f32,
    sample_count: usize,
}

impl Default for ProbeCanvas {
    fn default() -> Self {
        Self {
            strokes: Vec::new(),
            live: None,
            raster: CanvasRaster::new([1, 1]),
            area: None,
            pixels_per_point: 1.0,
            sample_count: 0,
        }
    }
}

impl ProbeCanvas {
    pub fn contains(&self, position: [f64; 2]) -> bool {
        self.area.is_some_and(|[left, top, right, bottom]| {
            (f64::from(left)..f64::from(right)).contains(&position[0])
                && (f64::from(top)..f64::from(bottom)).contains(&position[1])
        })
    }

    pub fn apply(&mut self, input: CanvasInput) {
        let origin = self.area.map_or([0, 0], |area| [area[0], area[1]]);
        let local = |sample: &PointerSample| {
            [
                (sample.position[0] - f64::from(origin[0])) as f32,
                (sample.position[1] - f64::from(origin[1])) as f32,
            ]
        };
        match input {
            CanvasInput::Begin(kind, sample) => {
                self.sample_count += 1;
                let stroke = Stroke {
                    style: style(kind, self.pixels_per_point),
                    points: vec![local(&sample)],
                };
                self.raster.dot(stroke.points[0], stroke.style);
                self.live = Some(stroke);
            }
            CanvasInput::Extend(sample) | CanvasInput::End(sample) => {
                self.sample_count += 1;
                if let Some(live) = self.live.as_mut() {
                    let to = local(&sample);
                    let from = *live.points.last().expect("a stroke starts with a point");
                    self.raster.segment(from, to, live.style);
                    live.points.push(to);
                }
                if matches!(input, CanvasInput::End(_)) {
                    self.strokes.extend(self.live.take());
                }
            }
            CanvasInput::Cancel => {
                if self.live.take().is_some() {
                    self.redraw();
                }
            }
        }
    }

    pub fn clear(&mut self) {
        self.strokes.clear();
        self.live = None;
        self.raster.clear();
    }

    pub fn sample_count(&self) -> usize {
        self.sample_count
    }

    pub fn raster_mut(&mut self) -> &mut CanvasRaster {
        &mut self.raster
    }

    fn redraw(&mut self) {
        let strokes = self.strokes.iter().chain(self.live.iter());
        self.raster
            .redraw(strokes.map(|stroke| (stroke.points.as_slice(), stroke.style)));
    }

    /// Takes the rest of `ui` and returns the canvas area in client physical
    /// pixels, snapped to whole pixels so that canvas pixels map 1:1.
    pub fn layout(&mut self, ui: &mut egui::Ui) -> [i32; 4] {
        let rect = ui.max_rect();
        self.pixels_per_point = ui.ctx().pixels_per_point();
        let edge = |value: f32| (value * self.pixels_per_point).round() as i32;
        let area = [
            edge(rect.left()),
            edge(rect.top()),
            edge(rect.right()),
            edge(rect.bottom()),
        ];
        let size = [
            (area[2] - area[0]).max(1) as u32,
            (area[3] - area[1]).max(1) as u32,
        ];
        self.area = Some(area);
        if size != self.raster.size() {
            self.raster.resize(size);
            self.redraw();
        }
        area
    }
}

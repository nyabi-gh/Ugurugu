// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! M0 input probe canvas: shows raw strokes so pointer handling can be checked.
//! Rendering here is a stand-in until the renderer is chosen.

use ugu_win::pointer::{PointerKind, PointerSample};

use crate::input::CanvasInput;

struct Stroke {
    kind: PointerKind,
    /// Client physical pixels relative to the canvas origin.
    points: Vec<[f64; 2]>,
}

#[derive(Default)]
pub struct ProbeCanvas {
    strokes: Vec<Stroke>,
    live: Option<Stroke>,
    /// Canvas area of the last frame, in client physical pixels.
    area: Option<[f64; 4]>,
    sample_count: usize,
}

impl ProbeCanvas {
    pub fn contains(&self, position: [f64; 2]) -> bool {
        self.area.is_some_and(|[left, top, right, bottom]| {
            (left..right).contains(&position[0]) && (top..bottom).contains(&position[1])
        })
    }

    pub fn apply(&mut self, input: CanvasInput) {
        let origin = self.area.map_or([0.0, 0.0], |area| [area[0], area[1]]);
        let local = |sample: &PointerSample| {
            [
                sample.position[0] - origin[0],
                sample.position[1] - origin[1],
            ]
        };
        match input {
            CanvasInput::Begin(kind, sample) => {
                self.sample_count += 1;
                self.live = Some(Stroke {
                    kind,
                    points: vec![local(&sample)],
                });
            }
            CanvasInput::Extend(sample) => {
                self.sample_count += 1;
                if let Some(live) = self.live.as_mut() {
                    live.points.push(local(&sample));
                }
            }
            CanvasInput::End(sample) => {
                self.sample_count += 1;
                if let Some(mut live) = self.live.take() {
                    live.points.push(local(&sample));
                    self.strokes.push(live);
                }
            }
            CanvasInput::Cancel => self.live = None,
        }
    }

    pub fn clear(&mut self) {
        self.strokes.clear();
        self.live = None;
    }

    pub fn sample_count(&self) -> usize {
        self.sample_count
    }

    pub fn show(&mut self, ui: &mut egui::Ui) {
        let rect = ui.max_rect();
        let ppp = ui.ctx().pixels_per_point() as f64;
        self.area = Some([
            rect.left() as f64 * ppp,
            rect.top() as f64 * ppp,
            rect.right() as f64 * ppp,
            rect.bottom() as f64 * ppp,
        ]);
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, egui::Color32::from_gray(245));
        let to_screen = |point: &[f64; 2]| {
            rect.min + egui::vec2((point[0] / ppp) as f32, (point[1] / ppp) as f32)
        };
        for stroke in self.strokes.iter().chain(self.live.iter()) {
            let color = match stroke.kind {
                PointerKind::Pen => egui::Color32::from_rgb(20, 60, 160),
                _ => egui::Color32::from_gray(20),
            };
            let points: Vec<egui::Pos2> = stroke.points.iter().map(to_screen).collect();
            painter.add(egui::Shape::line(points, egui::Stroke::new(2.0, color)));
        }
    }
}

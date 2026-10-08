// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! The brush and eraser presets in the tool settings, after 2.2.13's
//! `BrushPopoverPanel` and `EraserPopoverPanel`: category tabs and a card for
//! each preset, showing a stroke drawn with it. A brush card's stroke wobbles
//! only while the pointer is over it, so the panel draws nothing while idle.

use std::collections::HashMap;
use std::sync::Arc;

use egui::{Align2, Color32, CornerRadius, FontId, Rect, Sense, Stroke, StrokeKind, Vec2, pos2};
use ugu_core::brush::{BRUSHES, Category, ERASERS, Preset};
use ugu_core::document::{Document, LayerKind};
use ugu_core::ops::{Op, Rgba8, StrokeId, Wobble};
use ugu_core::store::{self, Point};
use ugu_render::document::{DocumentRenderer, Purpose};
use ugu_session::Tool;
use vello_cpu::Pixmap;

use crate::canvas::Canvas;
use crate::i18n::tr;
use crate::theme;

const CATEGORIES: [Category; 4] = [
    Category::Pen,
    Category::Marker,
    Category::Airbrush,
    Category::Spray,
];
/// 2.2.13's previews in points, and the space above them in their cards.
const BRUSH_PREVIEW: [f32; 2] = [116.0, 28.0];
const ERASER_PREVIEW: [f32; 2] = [70.0, 36.0];
/// Brush previews' frames, how far apart, and wobble.
const FRAMES: u32 = 6;
const FRAME_SECONDS: f64 = 0.12;
const PREVIEW_WOBBLE: f32 = 2.2;

fn category_name(category: Category) -> &'static str {
    match category {
        Category::Pen => tr("brush-pen"),
        Category::Marker => tr("brush-marker"),
        Category::Airbrush => tr("brush-airbrush"),
        Category::Spray => tr("brush-spray"),
    }
}

#[derive(Default)]
pub struct Presets {
    /// The brush category shown; `None` follows the current brush.
    category: Option<Category>,
    /// Preview frames by preset and frame, for `scale` pixels per point.
    previews: HashMap<(&'static str, u32), egui::TextureHandle>,
    scale: f32,
    renderer: Option<DocumentRenderer>,
}

impl Presets {
    pub fn show(&mut self, ui: &mut egui::Ui, canvas: &mut Canvas, tool: Tool) {
        let scale = ui.ctx().pixels_per_point();
        if scale != self.scale {
            self.previews.clear();
            self.scale = scale;
        }
        let current = match tool {
            Tool::Pen | Tool::Select | Tool::Wand | Tool::Fill | Tool::Text | Tool::Eyedropper => {
                canvas.session().pen.preset
            }
            Tool::Eraser => canvas.session().eraser.preset,
        };
        let presets: Vec<&'static Preset> = match tool {
            Tool::Pen | Tool::Select | Tool::Wand | Tool::Fill | Tool::Text | Tool::Eyedropper => {
                let shown = self.category.unwrap_or(current.category);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    let width = (ui.available_width() - 12.0) / 4.0;
                    for category in CATEGORIES {
                        let tab =
                            egui::Button::selectable(shown == category, category_name(category));
                        if ui.add_sized([width, 22.0], tab).clicked() {
                            self.category = Some(category);
                        }
                    }
                });
                BRUSHES
                    .iter()
                    .filter(|preset| preset.category == shown)
                    .collect()
            }
            Tool::Eraser => ERASERS.iter().collect(),
        };
        let columns = if tool == Tool::Pen { 2 } else { 3 };
        let gap = 4.0;
        let width = (ui.available_width() - gap * (columns - 1) as f32) / columns as f32;
        let mut chosen = None;
        egui::Grid::new(("presets", tool == Tool::Pen))
            .spacing([gap, gap])
            .show(ui, |ui| {
                for (index, preset) in presets.iter().enumerate() {
                    if self
                        .card(ui, preset, preset.id == current.id, width)
                        .clicked()
                    {
                        chosen = Some(*preset);
                    }
                    if (index + 1) % columns == 0 {
                        ui.end_row();
                    }
                }
            });
        if let Some(preset) = chosen {
            canvas.edit(|session| session.choose_preset(tool, preset));
        }
    }

    fn card(
        &mut self,
        ui: &mut egui::Ui,
        preset: &'static Preset,
        selected: bool,
        width: f32,
    ) -> egui::Response {
        let eraser = is_eraser(preset);
        let ([preview_width, preview_height], top) = if eraser {
            (ERASER_PREVIEW, 5.0)
        } else {
            (BRUSH_PREVIEW, 3.0)
        };
        let height = top + preview_height + 3.0 + 16.0 + 3.0;
        let (rect, response) = ui.allocate_exact_size(Vec2::new(width, height), Sense::click());
        let name = tr(preset.id);
        response.widget_info(|| {
            egui::WidgetInfo::selected(egui::WidgetType::RadioButton, true, selected, name)
        });
        let hovered = response.hovered();
        let frame = if hovered && !eraser {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_secs_f64(FRAME_SECONDS));
            let time = ui.ctx().input(|input| input.time);
            (time / FRAME_SECONDS) as u32 % FRAMES
        } else {
            0
        };
        let painter = ui.painter();
        let fill = if selected || hovered {
            theme::HOVER
        } else {
            theme::BASE
        };
        let edge = if selected || response.has_focus() {
            Stroke::new(1.5, theme::ACCENT)
        } else {
            Stroke::new(1.0, theme::BORDER)
        };
        painter.rect(rect, CornerRadius::same(8), fill, edge, StrokeKind::Inside);
        let texture = self.preview(ui.ctx(), preset, frame);
        let shown = Vec2::new(preview_width.min(width - 8.0), preview_height);
        let image = Rect::from_center_size(
            pos2(rect.center().x, rect.top() + top + shown.y * 0.5),
            shown,
        );
        let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
        painter.image(texture.id(), image, uv, Color32::WHITE);
        let ink = if selected { theme::TEXT } else { theme::MUTED };
        let font = FontId::proportional(theme::SMALL);
        let mut job = egui::text::LayoutJob::simple_singleline(name.to_owned(), font, ink);
        job.wrap.max_width = width - 12.0;
        job.wrap.max_rows = 1;
        job.wrap.break_anywhere = true;
        let galley = painter.layout_job(job);
        let at = pos2(rect.center().x, rect.bottom() - 11.0);
        painter.galley(
            Align2::CENTER_CENTER.anchor_size(at, galley.size()).min,
            galley,
            ink,
        );
        response.on_hover_text(name)
    }

    /// `preset`'s stroke on `frame`, drawn once at the screen's scale.
    fn preview(
        &mut self,
        ctx: &egui::Context,
        preset: &'static Preset,
        frame: u32,
    ) -> egui::TextureHandle {
        if let Some(texture) = self.previews.get(&(preset.id, frame)) {
            return texture.clone();
        }
        let scale = self.scale;
        let document = if is_eraser(preset) {
            eraser_preview(preset, scale)
        } else {
            brush_preview(preset, scale)
        };
        let size = document.canvas;
        let mut pixmap = Pixmap::new(size[0] as u16, size[1] as u16);
        self.renderer
            .get_or_insert_with(|| DocumentRenderer::new(1))
            .render(&document, i64::from(frame), Purpose::Display, &mut pixmap);
        let image = egui::ColorImage::from_rgba_premultiplied(
            [size[0] as usize, size[1] as usize],
            pixmap.data_as_u8_slice(),
        );
        let texture = ctx.load_texture(
            format!("preset {} {frame}", preset.id),
            image,
            egui::TextureOptions::LINEAR,
        );
        self.previews.insert((preset.id, frame), texture.clone());
        texture
    }
}

fn is_eraser(preset: &Preset) -> bool {
    ERASERS.iter().any(|eraser| eraser.id == preset.id)
}

/// A preview document of `size` points at `scale` pixels per point.
fn preview_document(size: [f32; 2], scale: f32) -> Document {
    let mut document = Document::new(size.map(|edge| (edge * scale).round().max(1.0) as u32));
    document.background = Rgba8([0, 0, 0, 0]);
    document.frames = FRAMES;
    document.wobble = Wobble::classic(PREVIEW_WOBBLE);
    document
}

fn text_color(alpha: u8) -> Rgba8 {
    let [r, g, b, _] = theme::TEXT.to_array();
    Rgba8([r, g, b, alpha])
}

fn seed(preset: &Preset) -> u64 {
    preset.id.bytes().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x100_0000_01b3)
    })
}

fn points(points: impl Iterator<Item = ([f32; 2], f32)>, scale: f32) -> Arc<[Point]> {
    points
        .map(|([x, y], pressure)| Point {
            x: x * scale,
            y: y * scale,
            pressure,
        })
        .collect()
}

fn with_ops(mut document: Document, ops: Vec<Op>) -> Document {
    if let LayerKind::Paint(paint) = &mut document.layers[0].kind {
        paint.ops = ops;
    }
    document
}

/// 2.2.13's brush preview: a gentle curve across the card, pressing
/// hardest in the middle, at most 14 points wide.
fn brush_preview(preset: &Preset, scale: f32) -> Document {
    let mut document = preview_document(BRUSH_PREVIEW, scale);
    let samples = 26;
    let curve = (0..samples).map(|index| {
        let t = index as f32 / (samples - 1) as f32;
        let wave = (t * std::f32::consts::PI * 1.5).sin() * 4.0;
        let at = [
            10.0 + t * (BRUSH_PREVIEW[0] - 20.0),
            BRUSH_PREVIEW[1] * 0.5 - wave,
        ];
        (at, 0.2 + 0.8 * (t * std::f32::consts::PI).sin())
    });
    let stroke = store::Stroke {
        points: points(curve, scale),
        color: text_color(255),
        width: preset.size.min(14.0) * scale,
        brush: preset.brush,
        seed: seed(preset),
    };
    document.store.strokes.insert(StrokeId(0), stroke);
    with_ops(
        document,
        vec![Op::Paint {
            stroke: StrokeId(0),
            clip: None,
        }],
    )
}

/// 2.2.13's eraser preview: a band rubbed out along a short diagonal.
fn eraser_preview(preset: &Preset, scale: f32) -> Document {
    let mut document = preview_document(ERASER_PREVIEW, scale);
    let band = store::Stroke {
        points: points([([4.0, 18.0], 1.0), ([66.0, 18.0], 1.0)].into_iter(), scale),
        color: text_color(255),
        width: 20.0 * scale,
        brush: store::Brush {
            size_dynamics: 0.0,
            antialias: true,
            ..store::Brush::DEFAULT
        },
        seed: 0,
    };
    let samples = 18;
    let diagonal = (0..samples).map(|index| {
        let t = index as f32 / (samples - 1) as f32;
        ([26.0 + t * 18.0, 4.0 + t * 28.0], 1.0)
    });
    let rub = store::Stroke {
        points: points(diagonal, scale),
        color: Rgba8([0, 0, 0, 255]),
        width: 16.0 * scale,
        brush: preset.brush,
        seed: seed(preset),
    };
    document.store.strokes.insert(StrokeId(0), band);
    document.store.strokes.insert(StrokeId(1), rub);
    with_ops(
        document,
        vec![
            Op::Paint {
                stroke: StrokeId(0),
                clip: None,
            },
            Op::Erase {
                stroke: StrokeId(1),
                clip: None,
            },
        ],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drawn(document: &Document) -> u64 {
        let [width, height] = document.canvas.map(|edge| edge as u16);
        let mut pixmap = Pixmap::new(width, height);
        DocumentRenderer::new(1).render(document, 0, Purpose::Display, &mut pixmap);
        pixmap
            .data_as_u8_slice()
            .chunks(4)
            .map(|pixel| u64::from(pixel[3]))
            .sum()
    }

    #[test]
    fn every_preview_is_a_valid_document_that_shows_its_preset() {
        for scale in [1.0, 1.5] {
            for preset in &BRUSHES {
                let document = brush_preview(preset, scale);
                document.validate().unwrap();
                assert!(drawn(&document) > 0, "{}", preset.id);
            }
            for preset in &ERASERS {
                let document = eraser_preview(preset, scale);
                document.validate().unwrap();
                let mut band = document.clone();
                if let LayerKind::Paint(paint) = &mut band.layers[0].kind {
                    paint.ops.truncate(1);
                }
                let rubbed = drawn(&document);
                assert!(rubbed > 0 && rubbed < drawn(&band), "{}", preset.id);
            }
        }
    }
}

// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! The canvas split around the layer being drawn on, so a stroke changes one
//! surface and the shown image is put back together only where it changed.
//!
//! `below` holds the background and every shown layer under the current one,
//! `surface` the current layer's own pixels before its opacity, and `above`
//! the shown layers over it on transparent. Pixel arithmetic follows Vello's
//! u8 pipeline (`(a·b + 255) >> 8` for a normalised product), so a stroke
//! added to `surface` is within one level of what a full render of that layer
//! gives; the caches are made again from full renders whenever the frame or
//! the layers change.

use ugu_core::document::{Document, LayerId, LayerKind};
use ugu_core::store::Stroke;
use vello_cpu::color::AlphaColor;
use vello_cpu::kurbo::Affine;
use vello_cpu::{Pixmap, RasterizerSettings, RenderContext, RenderSettings, Resources};

use crate::document::DocumentRenderer;
use crate::live::LiveStroke;
use crate::raster::{PixelRect, document_level};
use crate::stroke::{self, Pen, Resampler};

/// `a·b/255` as Vello's u8 pipeline rounds it.
fn mul(a: u8, b: u8) -> u8 {
    ((u16::from(a) * u16::from(b) + 255) >> 8) as u8
}

/// Premultiplied `source` over `target`.
pub fn src_over(target: &mut [u8], source: &[u8]) {
    let keep = 255 - source[3];
    for channel in 0..4 {
        target[channel] = source[channel] + mul(keep, target[channel]);
    }
}

/// Removes `alpha` of `target`.
pub fn dest_out(target: &mut [u8], alpha: u8) {
    let keep = 255 - alpha;
    for value in target.iter_mut().take(4) {
        *value = mul(keep, *value);
    }
}

/// Paints premultiplied `color` over `target` where a shape covers `cover`
/// of the pixel, with Vello's formula for antialiased edges. Vello fills
/// spans it knows are fully covered with plain `src_over`, which can round
/// one level differently; which pixels those are depends on its tiling.
pub fn paint(target: &mut [u8], color: [u8; 4], cover: u8) {
    if cover == 255 {
        src_over(target, &color);
        return;
    }
    let keep = u16::from(255 - mul(color[3], cover));
    for channel in 0..4 {
        let mixed =
            u16::from(target[channel]) * keep + u16::from(color[channel]) * u16::from(cover);
        target[channel] = ((mixed + 255) >> 8) as u8;
    }
}

/// Erases with premultiplied `color`'s alpha where a shape covers `cover`.
pub fn erase(target: &mut [u8], color: [u8; 4], cover: u8) {
    dest_out(target, mul(cover, color[3]));
}

/// Vello's premultiplied bytes for a straight colour.
pub fn premultiplied([r, g, b, a]: [u8; 4]) -> [u8; 4] {
    let color = AlphaColor::<vello_cpu::color::Srgb>::from_rgba8(r, g, b, a)
        .premultiply()
        .to_rgba8();
    [color.r, color.g, color.b, color.a]
}

pub struct Split {
    pub layer: LayerId,
    /// The frame within the cycle.
    pub frame: u32,
    pub opacity: f32,
    pub visible: bool,
    pub below: Pixmap,
    pub surface: Pixmap,
    pub above: Pixmap,
}

impl DocumentRenderer {
    /// Splits `frame` around `layer`, a top-level paint layer. `None` when
    /// there is no such layer.
    pub fn split(&mut self, document: &Document, layer: LayerId, frame: i64) -> Option<Split> {
        let index = document.layers.iter().position(|each| each.id == layer)?;
        let current = &document.layers[index];
        let LayerKind::Paint(paint) = &current.kind else {
            return None;
        };
        let [width, height] = document.canvas.map(|edge| edge as u16);
        let mut below = Pixmap::new(width, height);
        let mut surface = Pixmap::new(width, height);
        let mut above = Pixmap::new(width, height);
        let frame = ugu_core::motion::frame_in_cycle(frame, document.frames);
        let shown = |each: &&ugu_core::document::Layer| each.visible;
        self.render_layers(
            document,
            frame,
            true,
            document.layers[..index].iter().filter(shown),
            &mut below,
        );
        self.render_surface(document, paint, frame, &mut surface);
        self.render_layers(
            document,
            frame,
            false,
            document.layers[index + 1..].iter().filter(shown),
            &mut above,
        );
        Some(Split {
            layer,
            frame,
            opacity: paint.opacity,
            visible: current.visible,
            below,
            surface,
            above,
        })
    }
}

/// Puts the shown image back together within `rect`, drawing `live` (a
/// stroke being drawn, `None` for none) on the surface first.
pub fn composite(split: &Split, live: Option<&LiveStroke>, rect: PixelRect, out: &mut Pixmap) {
    let width = usize::from(out.width());
    let opacity = (split.opacity.clamp(0.0, 1.0) * 255.0).round() as u8;
    let [left, top, right, bottom] = rect.map(|value| value as usize);
    let below = split.below.data_as_u8_slice();
    let surface = split.surface.data_as_u8_slice();
    let above = split.above.data_as_u8_slice();
    let out = out.data_as_u8_slice_mut();
    for y in top..bottom {
        for x in left..right {
            let at = (y * width + x) * 4;
            let mut pixel: [u8; 4] = below[at..at + 4].try_into().expect("4 bytes");
            if split.visible {
                let mut layer: [u8; 4] = surface[at..at + 4].try_into().expect("4 bytes");
                if let Some(live) = live {
                    live.apply(x, y, &mut layer);
                }
                let faded = layer.map(|value| mul(value, opacity));
                src_over(&mut pixel, &faded);
            }
            src_over(&mut pixel, &above[at..at + 4]);
            out[at..at + 4].copy_from_slice(&pixel);
        }
    }
}

/// Draws one stroke into a pixel rectangle on the calling thread.
pub struct Stamp {
    context: RenderContext,
    resources: Resources,
}

impl Default for Stamp {
    fn default() -> Self {
        Self {
            context: RenderContext::new_with(
                1,
                1,
                RenderSettings {
                    level: document_level(),
                    num_threads: 0,
                },
            ),
            resources: Resources::new(),
        }
    }
}

impl Stamp {
    /// Adds a committed stroke to `surface` as a full render would: painted
    /// over, or erased from, what is there. Returns the pixels it changed.
    pub fn apply_stroke(
        &mut self,
        surface: &mut Pixmap,
        stroke: &Stroke,
        erase: bool,
        wobble: f32,
        frame: u32,
    ) -> Option<PixelRect> {
        let pen = Pen {
            width: stroke.width,
            brush: stroke.brush,
            seed: stroke.seed,
            wobble: f64::from(wobble) * f64::from(stroke.brush.wobble_scale),
        };
        let rect = clamp(stroke::bounds(&stroke.points, &pen), surface)?;
        let samples = Resampler::whole(&stroke.points, stroke::spacing(stroke.width));
        let path = stroke::outline(&samples, &pen, frame)?;
        let coverage = self.draw(rect, |context| {
            context.set_aliasing_threshold((!stroke.brush.antialias).then_some(128));
            context.set_paint(AlphaColor::<vello_cpu::color::Srgb>::WHITE);
            context.fill_path(&path);
        });
        let color = premultiplied(stroke_color(stroke, erase));
        blend_coverage(surface, &coverage, rect, color, erase);
        Some(rect)
    }

    /// Fills `draw` in a fresh transparent pixmap covering `rect`.
    pub fn draw(&mut self, rect: PixelRect, draw: impl FnOnce(&mut RenderContext)) -> Pixmap {
        let [left, top, right, bottom] = rect.map(|value| value as u16);
        let (width, height) = (right - left, bottom - top);
        self.context.reset_and_resize(width, height);
        self.context
            .set_transform(Affine::translate((-f64::from(left), -f64::from(top))));
        draw(&mut self.context);
        self.context.flush();
        let mut piece = Pixmap::new(width, height);
        self.context.render_with(
            &mut piece,
            &mut self.resources,
            RasterizerSettings::default(),
        );
        piece
    }
}

/// The whole pixels `bounds` (left, top, right, bottom) touches on `pixmap`,
/// with one more each way for antialiasing.
pub fn clamp([left, top, right, bottom]: [f64; 4], pixmap: &Pixmap) -> Option<PixelRect> {
    let size = [f64::from(pixmap.width()), f64::from(pixmap.height())];
    let rect = [
        (left.floor() - 1.0).clamp(0.0, size[0]) as u32,
        (top.floor() - 1.0).clamp(0.0, size[1]) as u32,
        (right.ceil() + 1.0).clamp(0.0, size[0]) as u32,
        (bottom.ceil() + 1.0).clamp(0.0, size[1]) as u32,
    ];
    (rect[0] < rect[2] && rect[1] < rect[3]).then_some(rect)
}

/// The straight colour a stroke is drawn in: an eraser's is black, and the
/// brush opacity scales the alpha.
pub fn stroke_color(stroke: &Stroke, erase: bool) -> [u8; 4] {
    let [r, g, b, a] = if erase {
        [0, 0, 0, stroke.color.0[3]]
    } else {
        stroke.color.0
    };
    let alpha = (f32::from(a) * stroke.brush.opacity.clamp(0.0, 1.0)).round() as u8;
    [r, g, b, alpha]
}

/// Paints premultiplied `color`, or erases, through `coverage` (alpha of a
/// pixmap covering `rect`) onto `surface`.
fn blend_coverage(
    surface: &mut Pixmap,
    coverage: &Pixmap,
    rect: PixelRect,
    color: [u8; 4],
    erase_it: bool,
) {
    let width = usize::from(surface.width());
    let coverage_width = usize::from(coverage.width());
    let [left, top, right, bottom] = rect.map(|value| value as usize);
    let target = surface.data_as_u8_slice_mut();
    let source = coverage.data_as_u8_slice();
    for y in top..bottom {
        for x in left..right {
            let cover = source[((y - top) * coverage_width + x - left) * 4 + 3];
            if cover == 0 {
                continue;
            }
            let to = (y * width + x) * 4;
            if erase_it {
                erase(&mut target[to..to + 4], color, cover);
            } else {
                paint(&mut target[to..to + 4], color, cover);
            }
        }
    }
}

impl DocumentRenderer {
    /// The current layer's own pixels, without its opacity.
    fn render_surface(
        &mut self,
        document: &Document,
        paint: &ugu_core::ops::PaintLayer,
        frame: u32,
        pixmap: &mut Pixmap,
    ) {
        self.render_ops(document, frame, None, &[(paint, None)], pixmap);
    }

    fn render_layers<'a>(
        &mut self,
        document: &'a Document,
        frame: u32,
        background: bool,
        layers: impl Iterator<Item = &'a ugu_core::document::Layer>,
        pixmap: &mut Pixmap,
    ) {
        let layers: Vec<_> = layers
            .filter_map(|layer| match &layer.kind {
                LayerKind::Paint(paint) => Some((paint, Some(paint.opacity))),
                LayerKind::Group(_) => None,
            })
            .collect();
        self.render_ops(
            document,
            frame,
            background.then_some(document.background),
            &layers,
            pixmap,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::Purpose;
    use std::sync::Arc;
    use ugu_core::command;
    use ugu_core::ops::{Rgba8, Wobble};
    use ugu_core::store::{Brush, BrushEngine, Point};

    fn stroke(from: [f32; 2], to: [f32; 2], color: [u8; 4], seed: u64) -> Stroke {
        Stroke {
            points: Arc::from(
                (0..=30)
                    .map(|step| {
                        let t = step as f32 / 30.0;
                        Point {
                            x: from[0] + (to[0] - from[0]) * t,
                            y: from[1] + (to[1] - from[1]) * t + (t * 9.0).sin() * 6.0,
                            pressure: 0.3 + 0.7 * t,
                        }
                    })
                    .collect::<Vec<_>>(),
            ),
            color: Rgba8(color),
            width: 9.0,
            brush: Brush {
                engine: BrushEngine::Line,
                opacity: 0.8,
                hardness: 1.0,
                antialias: true,
                size_dynamics: 0.8,
                wobble_scale: 1.0,
            },
            seed,
        }
    }

    /// Three layers with strokes and an eraser, the middle one translucent.
    fn document() -> Document {
        let mut document = Document::new([120, 80]);
        document.background = Rgba8([250, 245, 235, 255]);
        document.wobble = Wobble::classic(2.0);
        let first = document.layers[0].id;
        let changes = command::draw(
            &document,
            first,
            stroke([5.0, 10.0], [110.0, 60.0], [200, 40, 40, 255], 1),
            false,
            None,
        );
        ugu_core::edit::commit(&mut document, changes).unwrap();
        for (index, color) in [[40, 160, 60, 200], [30, 60, 200, 255]]
            .into_iter()
            .enumerate()
        {
            let (id, changes) =
                command::add_paint_layer(&document, None, index + 1, format!("{index}"));
            ugu_core::edit::commit(&mut document, changes).unwrap();
            let changes = command::draw(
                &document,
                id,
                stroke(
                    [10.0, 70.0 - index as f32 * 20.0],
                    [115.0, 15.0],
                    color,
                    10 + index as u64,
                ),
                false,
                None,
            );
            ugu_core::edit::commit(&mut document, changes).unwrap();
        }
        let middle = document.layers[1].id;
        let changes = command::draw(
            &document,
            middle,
            stroke([60.0, 0.0], [60.0, 80.0], [0, 0, 0, 255], 99),
            true,
            None,
        );
        ugu_core::edit::commit(&mut document, changes).unwrap();
        let changes = command::update_layer(&document, middle, |layer| {
            if let LayerKind::Paint(paint) = &mut layer.kind {
                paint.opacity = 0.6;
            }
        })
        .unwrap();
        ugu_core::edit::commit(&mut document, changes).unwrap();
        document
    }

    fn render(document: &Document, frame: i64) -> Pixmap {
        let mut pixmap = Pixmap::new(120, 80);
        DocumentRenderer::new(0).render(document, frame, Purpose::Display, &mut pixmap);
        pixmap
    }

    fn surface(document: &Document, layer: LayerId, frame: i64) -> Pixmap {
        DocumentRenderer::new(0)
            .split(document, layer, frame)
            .unwrap()
            .surface
    }

    #[test]
    fn the_split_puts_back_the_full_frame() {
        let document = document();
        for layer in document.layers.iter().map(|layer| layer.id) {
            let split = DocumentRenderer::new(0).split(&document, layer, 3).unwrap();
            let mut shown = Pixmap::new(120, 80);
            composite(&split, None, [0, 0, 120, 80], &mut shown);
            let full = render(&document, 3);
            let most = shown
                .data_as_u8_slice()
                .iter()
                .zip(full.data_as_u8_slice())
                .map(|(a, b)| a.abs_diff(*b))
                .max()
                .unwrap();
            // Layers above are put together before going over, which rounds
            // differently from one at a time.
            assert!(most <= 2, "layer {layer:?} differs by {most}");
        }
    }

    #[test]
    fn a_committed_stroke_added_to_the_surface_is_within_a_level_of_a_full_render() {
        let mut document = document();
        let layer = document.layers[1].id;
        for (erase, seed) in [(false, 500), (true, 501), (false, 502)] {
            let mut surface = surface(&document, layer, 4);
            let new = stroke([0.0, 40.0], [120.0, 35.0], [90, 20, 150, 180], seed);
            let wobble = document.wobble.amount;
            Stamp::default()
                .apply_stroke(&mut surface, &new, erase, wobble, 4)
                .unwrap();
            let changes = command::draw(&document, layer, new, erase, None);
            ugu_core::edit::commit(&mut document, changes).unwrap();
            let expected = surface_of(&document, layer, 4);
            let most = surface
                .data_as_u8_slice()
                .iter()
                .zip(expected.data_as_u8_slice())
                .map(|(a, b)| a.abs_diff(*b))
                .max()
                .unwrap();
            assert!(most <= 1, "erase {erase}: differs by {most}");
        }
    }

    fn surface_of(document: &Document, layer: LayerId, frame: i64) -> Pixmap {
        surface(document, layer, frame)
    }
}

// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! The canvas while it is edited: the frame's render plan and every layer's
//! own surface, kept so that a stroke changes one surface and the shown
//! image is put together again only where it changed, by the same
//! compositor as a full render.
//!
//! Pixel arithmetic follows Vello's u8 pipeline (`(a·b + 255) >> 8` for a
//! normalised product), so a stroke added to a surface is within one level
//! of what drawing that layer again gives.

use std::sync::Arc;

use ugu_core::document::{Document, LayerId};
use ugu_core::history::LayerRevisions;
use ugu_core::ops::Wobble;
use ugu_core::store::{BrushEngine, Mask, Stroke};
use vello_cpu::color::AlphaColor;
use vello_cpu::kurbo::Affine;
use vello_cpu::{Pixmap, RasterizerSettings, RenderContext, RenderSettings, Resources};

use crate::composite::{self, Overlay, Put, Source};
use crate::dab::{self, Dab};
use crate::document::{DocumentRenderer, Purpose};
use crate::live::LiveStroke;
use crate::mask::Runs;
use crate::plan::RenderPlan;
use crate::raster::{PixelRect, document_level};
use crate::stream::Held;
use crate::stroke::{self, Pen, Resampler};
use crate::tile::TiledSurface;

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

/// Premultiplied `source` behind `target`.
pub fn dest_over(target: &mut [u8], source: [u8; 4]) {
    let keep = 255 - target[3];
    for channel in 0..4 {
        target[channel] = target[channel].saturating_add(mul(keep, source[channel]));
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

/// Whether a stroke cut to `clip` reaches the pixel at `x`, `y`.
pub fn inside(clip: Option<&Mask>, x: usize, y: usize) -> bool {
    clip.is_none_or(|clip| clip.contains(x as i32, y as i32))
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
    /// How the shown image is put together.
    pub puts: Vec<Put>,
    /// Premultiplied; `None` when the first step lays the background.
    pub background: Option<[u8; 4]>,
    /// One per step that takes a source, in order.
    pub sources: Vec<Held>,
    /// The edited layer among the sources; `None` when it is not shown.
    pub edited: Option<usize>,
    /// The paint layer of each source, in order; `None` when sources were
    /// put together ahead of time.
    pub layers: Option<Vec<LayerId>>,
}

impl Split {
    /// Adds a committed stroke to the edited layer as drawing it again would:
    /// painted over, or erased from, what is there. Returns the pixels it
    /// changed.
    pub fn stamp(
        &mut self,
        stamp: &mut Stamp,
        stroke: &Stroke,
        erase: bool,
        wobble: Wobble,
        frames: u32,
        clip: Option<&Mask>,
    ) -> Option<PixelRect> {
        let Held::Tiles(surface) = &mut self.sources[self.edited?] else {
            unreachable!("a paint layer's pixels are tiles");
        };
        let pen = Pen::new(stroke, wobble, frames);
        let surface = Arc::make_mut(surface);
        stamp.apply_stroke(surface, stroke, erase, &pen, self.frame, clip)
    }

    /// Puts `plan`'s layers (`RenderPlan::reference`) together into `out`,
    /// which must have the canvas size, from the surfaces held: what a full
    /// render of the plan gives. `false` when the plan has no layers or this
    /// split does not hold one of them apart (a hidden layer, or sources put
    /// together ahead of time); the caller then renders the plan.
    pub fn reference(&self, plan: &RenderPlan, out: &mut Pixmap, threads: usize) -> bool {
        let Some(layers) = &self.layers else {
            return false;
        };
        let sources = self.sources();
        let Some(read) = plan
            .layers
            .iter()
            .map(|(id, _)| {
                layers
                    .iter()
                    .position(|layer| layer == id)
                    .map(|at| sources[at])
            })
            .collect::<Option<Vec<_>>>()
        else {
            return false;
        };
        let rect = [0, 0, u32::from(out.width()), u32::from(out.height())];
        !read.is_empty()
            && composite::evaluate(
                &composite::program(plan),
                None,
                &read,
                rect,
                out,
                threads,
                None,
            )
    }

    /// Adds a committed fill to the edited layer as drawing it again at
    /// full size does: the covered pixels become `color` (straight), the
    /// fringe goes behind what is there. Returns the pixels it changed.
    pub fn stamp_fill(
        &mut self,
        coverage: &Mask,
        clip: Option<&Mask>,
        antialias: bool,
        color: [u8; 4],
    ) -> Option<PixelRect> {
        let Held::Tiles(surface) = &mut self.sources[self.edited?] else {
            unreachable!("a paint layer's pixels are tiles");
        };
        let surface = Arc::make_mut(surface);
        let (area, fringe) = Runs::fill(coverage, clip, antialias);
        let size = surface.size().map(|edge| edge as i32);
        let [left, top, right, bottom] = [area.bounds(), fringe.as_ref().and_then(Runs::bounds)]
            .into_iter()
            .flatten()
            .reduce(|[l, t, r, b], [l2, t2, r2, b2]| {
                [l.min(l2), t.min(t2), r.max(r2), b.max(b2)]
            })?;
        let rect = [
            left.max(0),
            top.max(0),
            right.min(size[0]),
            bottom.min(size[1]),
        ];
        if rect[0] >= rect[2] || rect[1] >= rect[3] {
            return None;
        }
        let rect = rect.map(|edge| edge as u32);
        let color = premultiplied(color);
        surface.ensure(rect);
        for (y, line) in surface.rows_mut(rect) {
            let y = y as i32;
            let [left, right] = [rect[0] as i32, rect[2] as i32];
            let span = |&[from, to]: &[i32; 2]| {
                (from.clamp(left, right) - left) as usize..(to.clamp(left, right) - left) as usize
            };
            for run in area.row(y) {
                line[span(run)].fill(color);
            }
            for run in fringe.iter().flat_map(|fringe| fringe.row(y)) {
                for target in &mut line[span(run)] {
                    dest_over(target, color);
                }
            }
        }
        Some(rect)
    }

    fn sources(&self) -> Vec<Source<'_>> {
        self.sources
            .iter()
            .map(|held| match held {
                Held::Tiles(surface) => Source::Tiles(surface),
                Held::Whole(pixmap) => Source::Whole(pixmap),
            })
            .collect()
    }
}

impl DocumentRenderer {
    /// Draws `frame` into `display`, which must have the canvas size, and
    /// keeps what it was made from for editing `layer`. With `revisions`,
    /// layers drawn before are reused. When the layers' own surfaces would
    /// take more than the budget, what does not depend on `layer` is put
    /// together ahead of time instead; the display then differs from a full
    /// render by the rounding of that, at most 2 levels. `None` when the stop
    /// flag ended it.
    pub fn split(
        &mut self,
        document: &Document,
        layer: LayerId,
        frame: i64,
        revisions: Option<&LayerRevisions>,
        display: &mut Pixmap,
    ) -> Option<Split> {
        let plan = RenderPlan::new(document, Purpose::Display);
        let cycle = ugu_core::motion::frame_in_cycle(frame, document.frames);
        if self.over_budget(document, &plan, 1) {
            let program = self.reduced(document, &plan, cycle, layer)?;
            let split = Split {
                layer,
                frame: cycle,
                puts: program.puts,
                background: None,
                sources: program.sources,
                edited: program.edited,
                layers: None,
            };
            let rect = [
                0,
                0,
                u32::from(display.width()),
                u32::from(display.height()),
            ];
            composite::evaluate(
                &split.puts,
                None,
                &split.sources(),
                rect,
                display,
                self.thread_count(),
                None,
            );
            return Some(split);
        }
        if !self.render_plan(document, &plan, frame, revisions, display) {
            return None;
        }
        let sources = plan
            .layers
            .iter()
            .map(|(id, _)| Held::Tiles(self.surface(*id).expect("just drawn")))
            .collect();
        // The edited layer's pixels change in place from now on, so this
        // renderer does not keep a share of them.
        self.forget(layer);
        Some(Split {
            layer,
            frame: cycle,
            edited: plan.layers.iter().position(|(id, _)| *id == layer),
            puts: composite::program(&plan),
            background: Some(premultiplied(document.background.0)),
            sources,
            layers: Some(plan.layers.iter().map(|(id, _)| *id).collect()),
        })
    }
}

/// Puts the shown image together within `rect`, drawing `live` (a stroke
/// being drawn, `None` for none) on the edited layer first.
pub fn composite(split: &Split, live: Option<&LiveStroke>, rect: PixelRect, out: &mut Pixmap) {
    let sources = split.sources();
    match (live, split.edited) {
        (Some(live), Some(source)) => {
            let apply = |x: usize, y: usize, pixel: &mut [u8; 4]| live.apply(x, y, pixel);
            let overlay = Overlay {
                source,
                apply: &apply,
            };
            composite::evaluate_with(&split.puts, split.background, &sources, rect, out, &overlay);
        }
        _ => {
            let [left, top, right, bottom] = rect;
            // Thread start-up outweighs the work below about this many pixels.
            let threads = if (right - left) * (bottom - top) > 65_536 {
                std::thread::available_parallelism().map_or(1, |count| count.get().min(8))
            } else {
                1
            };
            composite::evaluate(
                &split.puts,
                split.background,
                &sources,
                rect,
                out,
                threads,
                None,
            );
        }
    }
}

/// Draws one stroke into a pixel rectangle on the calling thread.
pub struct Stamp {
    context: RenderContext,
    resources: Resources,
    painter: dab::Painter,
    dabs: Vec<Dab>,
    /// A dab stroke's pixels, premultiplied.
    buffer: Vec<u8>,
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
            painter: dab::Painter::default(),
            dabs: Vec::new(),
            buffer: Vec::new(),
        }
    }
}

impl Stamp {
    /// Adds a committed stroke to `surface` as drawing the layer again would:
    /// painted over, or erased from, what is there, only inside `clip` when
    /// there is one. Returns the pixels it may have changed.
    pub fn apply_stroke(
        &mut self,
        surface: &mut TiledSurface,
        stroke: &Stroke,
        erase: bool,
        pen: &Pen,
        frame: u32,
        clip: Option<&Mask>,
    ) -> Option<PixelRect> {
        if stroke.brush.engine != BrushEngine::Line {
            return self.apply_dabs(surface, stroke, erase, pen, frame, clip);
        }
        let rect = clamp(stroke::bounds(&stroke.points, pen), surface.size())?;
        let samples = Resampler::whole(&stroke.points, stroke::spacing(stroke.width));
        let path = stroke::outline(&samples, pen, frame)?;
        let coverage = self.draw(rect, |context| {
            context.set_aliasing_threshold((!stroke.brush.antialias).then_some(128));
            context.set_paint(AlphaColor::<vello_cpu::color::Srgb>::WHITE);
            context.fill_path(&path);
        });
        let color = premultiplied(stroke_color(stroke, erase));
        surface.ensure(rect);
        let coverage_width = usize::from(coverage.width());
        let source = coverage.data_as_u8_slice();
        for (y, line) in surface.rows_mut(rect) {
            let from = (y - rect[1]) as usize * coverage_width;
            for (x, target) in line.iter_mut().enumerate() {
                let cover = source[(from + x) * 4 + 3];
                if cover == 0 || !inside(clip, rect[0] as usize + x, y as usize) {
                    continue;
                }
                if erase {
                    self::erase(target, color, cover);
                } else {
                    paint(target, color, cover);
                }
            }
        }
        Some(rect)
    }

    /// `apply_stroke` for an airbrush or spray: its dabs painted over each
    /// other, then over, or erased from, what is there, as the renderer lays
    /// them.
    fn apply_dabs(
        &mut self,
        surface: &mut TiledSurface,
        stroke: &Stroke,
        erase: bool,
        pen: &Pen,
        frame: u32,
        clip: Option<&Mask>,
    ) -> Option<PixelRect> {
        dab::stroke_dabs(
            &stroke.points,
            pen,
            frame,
            stroke.color.0[3],
            &mut self.dabs,
        );
        let rect = clamp(dab::bounds(&self.dabs), surface.size())?;
        let [left, top, right, _] = rect;
        let width = (right - left) as usize;
        for dab in &mut self.dabs {
            dab.center = [
                dab.center[0] - f64::from(left),
                dab.center[1] - f64::from(top),
            ];
        }
        self.buffer.clear();
        self.buffer.resize(width * (rect[3] - top) as usize * 4, 0);
        let [r, g, b, _] = if erase { [0; 4] } else { stroke.color.0 };
        let look = dab::look(&stroke.brush);
        self.painter
            .paint(&mut self.buffer, width, &self.dabs, look, [r, g, b]);
        surface.ensure(rect);
        for (y, line) in surface.rows_mut(rect) {
            let from = (y - top) as usize * width;
            for (x, target) in line.iter_mut().enumerate() {
                let source = &self.buffer[(from + x) * 4..(from + x) * 4 + 4];
                if source[3] == 0 || !inside(clip, left as usize + x, y as usize) {
                    continue;
                }
                if erase {
                    dest_out(target, source[3]);
                } else {
                    src_over(target, source);
                }
            }
        }
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

/// The whole pixels `bounds` (left, top, right, bottom) touches on a canvas
/// of `size`, with one more each way for antialiasing.
pub fn clamp([left, top, right, bottom]: [f64; 4], size: [u32; 2]) -> Option<PixelRect> {
    let size = size.map(f64::from);
    let rect = [
        (left.floor() - 1.0).clamp(0.0, size[0]) as u32,
        (top.floor() - 1.0).clamp(0.0, size[1]) as u32,
        (right.ceil() + 1.0).clamp(0.0, size[0]) as u32,
        (bottom.ceil() + 1.0).clamp(0.0, size[1]) as u32,
    ];
    (rect[0] < rect[2] && rect[1] < rect[3]).then_some(rect)
}

/// The straight colour a pen or marker stroke is drawn in: an eraser's is
/// black, and the alpha is `stroke::line_alpha`.
pub fn stroke_color(stroke: &Stroke, erase: bool) -> [u8; 4] {
    let [r, g, b, _] = if erase { [0; 4] } else { stroke.color.0 };
    [r, g, b, stroke::line_alpha(stroke)]
}

#[cfg(test)]
mod tests {
    use super::*;
    use ugu_core::command;
    use ugu_core::document::LayerKind;
    use ugu_core::edit::Change;
    use ugu_core::history::History;
    use ugu_core::ops::{Blend, Motion, MotionStyle, Rgba8, Wobble};
    use ugu_core::selection::{Combine, Selection, Shape};
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
                ..Brush::default()
            },
            seed,
        }
    }

    /// Layers with strokes and an eraser: a translucent Multiply layer, a
    /// layer clipped to it, and a Screen group holding the top layer.
    fn history() -> History {
        let mut document = Document::new([120, 80]);
        document.background = Rgba8([250, 245, 235, 255]);
        document.wobble = Wobble::classic(2.0);
        let mut history = History::new(document, false);
        let first = LayerId(1);
        let draw = |history: &mut History, layer, stroke, erase| {
            history
                .edit("Draw", |document| {
                    command::draw(document, layer, stroke, erase, None)
                })
                .unwrap();
        };
        draw(
            &mut history,
            first,
            stroke([5.0, 10.0], [110.0, 60.0], [200, 40, 40, 255], 1),
            false,
        );
        for (index, color) in [[40, 160, 60, 200], [30, 60, 200, 255], [200, 200, 30, 255]]
            .into_iter()
            .enumerate()
        {
            let mut added = LayerId(0);
            history
                .edit("Add", |document| {
                    let (id, changes) =
                        command::add_paint_layer(document, None, index + 1, format!("{index}"));
                    added = id;
                    changes
                })
                .unwrap();
            draw(
                &mut history,
                added,
                stroke(
                    [10.0, 70.0 - index as f32 * 20.0],
                    [115.0, 15.0],
                    color,
                    10 + index as u64,
                ),
                false,
            );
        }
        let [_, middle, clipped, top] =
            [0, 1, 2, 3].map(|index| history.document().layers[index].id);
        draw(
            &mut history,
            middle,
            stroke([60.0, 0.0], [60.0, 80.0], [0, 0, 0, 255], 99),
            true,
        );
        // Two layers move their own ways, as broken lines.
        for (layer, style) in [(clipped, MotionStyle::Stepped), (top, MotionStyle::Smooth)] {
            let motion = Motion {
                style,
                broken: true,
                break_amount: 0.4,
                break_range: 10.0,
                ..Motion::DEFAULT
            };
            history
                .edit("Wobble", |document| {
                    command::update_layer(document, layer, |layer| match &mut layer.kind {
                        LayerKind::Paint(paint) => {
                            paint.wobble = Some(Wobble {
                                amount: 3.0,
                                motion,
                            });
                        }
                        LayerKind::Group(_) => unreachable!("a paint layer"),
                    })
                    .unwrap()
                })
                .unwrap();
        }
        // A selection: a fill in it under the first layer's stroke's end,
        // and a clear in it on the Multiply layer.
        let [width, height] = [70, 50];
        let row_bytes = ugu_core::store::Mask::row_bytes(width);
        let mut bits = vec![0u8; row_bytes * height as usize];
        for y in 0..height {
            for x in 0..width {
                if (x - 35) * (x - 35) + (y - 25) * (y - 25) < 500 && (x + y) % 11 != 0 {
                    bits[y as usize * row_bytes + x as usize / 8] |= 0x80 >> (x % 8);
                }
            }
        }
        let mask = ugu_core::ops::MaskId(0);
        history
            .edit("Fill", |document| {
                let index = match document.layer(first).map(|layer| &layer.kind) {
                    Some(LayerKind::Paint(paint)) => paint.ops.len(),
                    _ => unreachable!(),
                };
                let selection = ugu_core::store::Mask {
                    bounds: [30, 20, width, height],
                    bits: bits.into(),
                };
                let fill = ugu_core::ops::Op::Fill {
                    coverage: mask,
                    color: Rgba8([20, 120, 220, 170]),
                    antialias: true,
                    clip: Some(mask),
                };
                vec![
                    Change::InsertMask(mask, selection),
                    Change::InsertOp {
                        layer: first,
                        index,
                        op: fill,
                    },
                ]
            })
            .unwrap();
        history
            .edit("Clear", |document| {
                let index = match document.layer(middle).map(|layer| &layer.kind) {
                    Some(LayerKind::Paint(paint)) => paint.ops.len(),
                    _ => unreachable!(),
                };
                vec![Change::InsertOp {
                    layer: middle,
                    index,
                    op: ugu_core::ops::Op::ClearSelection { mask },
                }]
            })
            .unwrap();
        // The selection moved half a pixel on the first layer, and an image
        // placed turned over it.
        let mut png = Vec::new();
        let mut encoder = png::Encoder::new(&mut png, 6, 4);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().unwrap();
        let pixels: Vec<u8> = (0..24u8)
            .flat_map(|index| [index * 10, 255 - index * 10, 90, 150 + index * 4])
            .collect();
        writer.write_image_data(&pixels).unwrap();
        writer.finish().unwrap();
        let asset = ugu_core::ops::AssetId([7; 32]);
        history
            .edit("Move and place", |document| {
                let index = match document.layer(first).map(|layer| &layer.kind) {
                    Some(LayerKind::Paint(paint)) => paint.ops.len(),
                    _ => unreachable!(),
                };
                let moved = ugu_core::ops::Op::TransformSelection {
                    mask,
                    transform: ugu_core::ops::Affine([1.0, 0.0, 12.5, 0.0, 1.0, -6.0]),
                    sampling: ugu_core::ops::Sampling::Smooth,
                    keep_source: false,
                };
                let placed = ugu_core::ops::Op::PlaceImage {
                    asset,
                    transform: ugu_core::ops::Affine([0.0, -3.0, 70.0, 3.0, 0.0, 30.0]),
                    sampling: ugu_core::ops::Sampling::Smooth,
                };
                let image = ugu_core::store::Asset {
                    size: [6, 4],
                    png: png.as_slice().into(),
                };
                vec![
                    Change::InsertAsset(asset, image),
                    Change::InsertOp {
                        layer: first,
                        index,
                        op: moved,
                    },
                    Change::InsertOp {
                        layer: first,
                        index: index + 1,
                        op: placed,
                    },
                ]
            })
            .unwrap();
        let set = |history: &mut History, id, update: &dyn Fn(&mut ugu_core::ops::PaintLayer)| {
            history
                .edit("Set", |document| {
                    command::update_layer(document, id, |layer| {
                        if let LayerKind::Paint(paint) = &mut layer.kind {
                            update(paint);
                        }
                    })
                    .unwrap()
                })
                .unwrap();
        };
        set(&mut history, middle, &|paint| {
            paint.opacity = 0.6;
            paint.blend = Blend::Multiply;
        });
        set(&mut history, clipped, &|paint| paint.clip_to_below = true);
        let mut group = history.document().layer(top).unwrap().clone();
        group.id = LayerId(50);
        group.kind = LayerKind::Group(ugu_core::document::Group {
            opacity: 0.9,
            blend: Blend::Screen,
            clip_to_below: false,
            children: vec![history.document().layer(top).unwrap().clone()],
        });
        history
            .edit("Group", |_| {
                vec![
                    ugu_core::edit::Change::RemoveLayer(top),
                    ugu_core::edit::Change::InsertLayer {
                        parent: None,
                        index: 3,
                        layer: group,
                    },
                ]
            })
            .unwrap();
        history
    }

    /// `history`, and the same cropped narrower and taller, drawn on, and
    /// resampled larger.
    fn histories() -> [History; 2] {
        let mut reframed = history();
        reframed
            .edit("Crop", |document| {
                command::crop_canvas(document, [-10, 6], [104, 90])
            })
            .unwrap();
        let first = LayerId(1);
        reframed
            .edit("Draw", |document| {
                let new = stroke([0.0, 85.0], [100.0, 5.0], [10, 140, 140, 230], 70);
                command::draw(document, first, new, false, None)
            })
            .unwrap();
        reframed
            .edit("Resample", |document| {
                command::resample_image(document, [130, 100], ugu_core::ops::Sampling::Smooth)
            })
            .unwrap();
        [history(), reframed]
    }

    fn layer_wobble(document: &Document, layer: LayerId) -> Wobble {
        match document.layer(layer).map(|layer| &layer.kind) {
            Some(LayerKind::Paint(paint)) => paint.wobble.unwrap_or(document.wobble),
            _ => document.wobble,
        }
    }

    fn canvas(document: &Document) -> Pixmap {
        let [width, height] = document.canvas.map(|edge| edge as u16);
        Pixmap::new(width, height)
    }

    fn whole(document: &Document) -> [u32; 4] {
        [0, 0, document.canvas[0], document.canvas[1]]
    }

    fn render(document: &Document, frame: i64) -> Pixmap {
        let mut pixmap = canvas(document);
        DocumentRenderer::new(0).render(document, frame, Purpose::Display, &mut pixmap);
        pixmap
    }

    fn split(history: &History, layer: LayerId, frame: i64) -> (Split, Pixmap) {
        let mut display = canvas(history.document());
        let split = DocumentRenderer::new(0)
            .split(
                history.document(),
                layer,
                frame,
                Some(history.layer_revisions()),
                &mut display,
            )
            .expect("nothing stops it");
        (split, display)
    }

    fn max_difference(a: &Pixmap, b: &Pixmap) -> u8 {
        a.data_as_u8_slice()
            .iter()
            .zip(b.data_as_u8_slice())
            .map(|(a, b)| a.abs_diff(*b))
            .max()
            .unwrap()
    }

    fn paint_layers(history: &History) -> Vec<LayerId> {
        RenderPlan::new(history.document(), Purpose::Display)
            .layers
            .iter()
            .map(|(id, _)| *id)
            .collect()
    }

    #[test]
    fn a_reference_put_together_from_the_split_is_the_reference_drawn() {
        use crate::plan::Reference;
        let mut history = history();
        let ids = paint_layers(&history);
        let (marked, hidden) = (ids[1], ids[2]);
        let change = |history: &mut History, id, change: fn(&mut ugu_core::document::Layer)| {
            history
                .edit("Change", |document| {
                    command::update_layer(document, id, change).unwrap()
                })
                .unwrap();
        };
        change(&mut history, marked, |layer| layer.reference = true);
        change(&mut history, hidden, |layer| layer.visible = false);
        let document = history.document();
        for frame in [0, 3] {
            let (split, _) = split(&history, ids[0], frame);
            let references = ids.iter().map(|id| Reference::Layer(*id));
            for reference in references.chain([Reference::Marked, Reference::Visible]) {
                let plan = RenderPlan::reference(document, reference);
                let mut drawn = canvas(document);
                DocumentRenderer::new(0).render_plan(document, &plan, frame, None, &mut drawn);
                let mut read = canvas(document);
                if split.reference(&plan, &mut read, 2) {
                    assert_eq!(
                        read.data_as_u8_slice(),
                        drawn.data_as_u8_slice(),
                        "{reference:?}, frame {frame}"
                    );
                } else {
                    // A hidden layer is drawn for its own reference only.
                    assert_eq!(reference, Reference::Layer(hidden));
                    assert!(drawn.data_as_u8_slice().chunks(4).any(|pixel| pixel[3] > 0));
                }
            }
        }
        // The background is left out, and nothing marked reads as nothing.
        let mut marked_only = canvas(document);
        let plan = RenderPlan::reference(document, Reference::Marked);
        DocumentRenderer::new(0).render_plan(document, &plan, 0, None, &mut marked_only);
        assert_eq!(&marked_only.data_as_u8_slice()[..4], &[0; 4]);
        change(&mut history, marked, |layer| layer.reference = false);
        assert!(
            RenderPlan::reference(history.document(), Reference::Marked)
                .layers
                .is_empty()
        );
    }

    #[test]
    fn the_split_puts_back_the_full_frame() {
        for history in histories() {
            let full = render(history.document(), 3);
            let layers = paint_layers(&history);
            assert_eq!(layers.len(), 4);
            for layer in layers {
                let (split, display) = split(&history, layer, 3);
                assert!(display.data_as_u8_slice() == full.data_as_u8_slice());
                let mut shown = canvas(history.document());
                composite(&split, None, whole(history.document()), &mut shown);
                assert!(
                    shown.data_as_u8_slice() == full.data_as_u8_slice(),
                    "layer {layer:?}"
                );
            }
        }
    }

    #[test]
    fn a_committed_stroke_stamped_on_any_layer_is_within_a_level_of_a_full_render() {
        let pen = stroke([0.0; 2], [1.0; 2], [0; 4], 0).brush;
        let fading = Brush {
            opacity_dynamics: 0.7,
            ..pen
        };
        let find = |id| ugu_core::brush::find(id).unwrap().brush;
        let brushes = [pen, fading, find("soft-airbrush"), find("rough-spray")];
        for (mut history, brush) in brushes
            .into_iter()
            .flat_map(|brush| histories().map(|history| (history, brush)))
        {
            for layer in paint_layers(&history) {
                // Drawn plainly and cut to the selection `history` keeps.
                let selection = ugu_core::ops::MaskId(0);
                for (erase, seed, clip) in [
                    (false, 500, None),
                    (true, 501, None),
                    (false, 502, Some(selection)),
                    (true, 503, Some(selection)),
                ] {
                    let (mut split, _) = split(&history, layer, 4);
                    let mut new = stroke([0.0, 40.0], [120.0, 35.0], [90, 20, 150, 180], seed);
                    new.brush = brush;
                    let document = history.document();
                    let rect = split
                        .stamp(
                            &mut Stamp::default(),
                            &new,
                            erase,
                            layer_wobble(document, layer),
                            document.frames,
                            clip.and_then(|id| document.store.masks.get(&id)),
                        )
                        .unwrap();
                    let mut shown = canvas(history.document());
                    composite(&split, None, whole(history.document()), &mut shown);
                    history
                        .edit("Draw", |document| {
                            command::draw(document, layer, new, erase, clip)
                        })
                        .unwrap();
                    let mut renderer = DocumentRenderer::new(0);
                    let mut full = canvas(history.document());
                    let plan = RenderPlan::new(history.document(), Purpose::Display);
                    renderer.render_plan(history.document(), &plan, 4, None, &mut full);
                    let Held::Tiles(stamped) = &split.sources[split.edited.unwrap()] else {
                        unreachable!("a paint layer's pixels are tiles");
                    };
                    let drawn = renderer.surface(layer).unwrap().to_pixmap();
                    let most = max_difference(&stamped.to_pixmap(), &drawn);
                    assert!(
                        most <= 1,
                        "{layer:?} {:?} erase {erase}: differs by {most}",
                        brush.engine
                    );
                    // A level in a layer can round to two through the layers
                    // over it.
                    let most = max_difference(&shown, &full);
                    assert!(
                        most <= 2,
                        "{layer:?} erase {erase}: shown differs by {most}"
                    );
                    assert!(rect[2] > rect[0]);
                }
            }
        }
    }

    #[test]
    fn a_fill_stamped_on_any_layer_is_within_a_level_of_a_full_render() {
        // A ring of a fill, so the fringe falls inside and outside it.
        let ring = Selection::combine(
            Selection::of_shape(&Shape::Ellipse([20.0, 10.0], [100.0, 70.0]), [120, 80]).as_ref(),
            Selection::of_shape(&Shape::Rectangle([45.0, 30.0], [75.0, 50.0]), [120, 80]),
            Combine::Subtract,
        )
        .unwrap();
        let coverage = ugu_core::ops::MaskId(40);
        let selection = ugu_core::ops::MaskId(0);
        for mut history in histories() {
            if history.document().canvas != [120, 80] {
                continue;
            }
            for layer in paint_layers(&history) {
                for (antialias, clip) in [(true, None), (false, None), (true, Some(selection))] {
                    let (mut split, _) = split(&history, layer, 4);
                    let document = history.document();
                    let color = [30, 140, 90, 200];
                    let rect = split
                        .stamp_fill(
                            ring.mask(),
                            clip.and_then(|id| document.store.masks.get(&id)),
                            antialias,
                            color,
                        )
                        .unwrap();
                    assert!(rect[2] > rect[0]);
                    let mut shown = canvas(document);
                    composite(&split, None, whole(document), &mut shown);
                    history
                        .group("Fill", |group| {
                            if !group.document().store.masks.contains_key(&coverage) {
                                group.apply(|_| {
                                    vec![Change::InsertMask(coverage, ring.mask().clone())]
                                })?;
                            }
                            group.apply(|document| {
                                command::fill(
                                    document,
                                    layer,
                                    coverage,
                                    Rgba8(color),
                                    antialias,
                                    clip,
                                )
                            })
                        })
                        .unwrap();
                    let mut renderer = DocumentRenderer::new(0);
                    let mut full = canvas(history.document());
                    let plan = RenderPlan::new(history.document(), Purpose::Display);
                    renderer.render_plan(history.document(), &plan, 4, None, &mut full);
                    let Held::Tiles(stamped) = &split.sources[split.edited.unwrap()] else {
                        unreachable!("a paint layer's pixels are tiles");
                    };
                    let drawn = renderer.surface(layer).unwrap().to_pixmap();
                    let most = max_difference(&stamped.to_pixmap(), &drawn);
                    assert!(
                        most <= 1,
                        "{layer:?} antialias {antialias} clip {clip:?}: differs by {most}"
                    );
                    let most = max_difference(&shown, &full);
                    assert!(most <= 2, "{layer:?}: shown differs by {most}");
                }
            }
        }
    }

    #[test]
    fn over_the_budget_the_split_keeps_every_layer_editable() {
        for history in histories() {
            over_the_budget(history);
        }
    }

    fn over_the_budget(mut history: History) {
        for layer in paint_layers(&history) {
            let mut display = canvas(history.document());
            let mut renderer = DocumentRenderer::new(4);
            renderer.set_surface_budget(0);
            let mut split = renderer
                .split(history.document(), layer, 4, None, &mut display)
                .expect("nothing stops it");
            // What does not depend on the layer is put together ahead of
            // time, which rounds a little differently from one at a time.
            let full = render(history.document(), 4);
            let most = max_difference(&display, &full);
            assert!(most <= 2, "{layer:?}: differs by {most}");
            assert!(split.edited.is_some());
            assert!(split.sources.len() < 6);

            let new = stroke([0.0, 40.0], [120.0, 35.0], [90, 20, 150, 180], 600);
            let document = history.document();
            split
                .stamp(
                    &mut Stamp::default(),
                    &new,
                    false,
                    layer_wobble(document, layer),
                    document.frames,
                    None,
                )
                .unwrap();
            let mut shown = canvas(history.document());
            composite(&split, None, whole(history.document()), &mut shown);
            history
                .edit("Draw", |document| {
                    command::draw(document, layer, new, false, None)
                })
                .unwrap();
            let most = max_difference(&shown, &render(history.document(), 4));
            assert!(most <= 2, "{layer:?} after a stroke: differs by {most}");
        }
    }

    #[test]
    fn a_pending_transform_shown_on_any_layer_is_within_a_level_of_a_full_render() {
        use ugu_core::ops::{Affine as Transform, Sampling};
        let centre = [65.0, 45.0];
        let transforms = [
            (Transform::translation(7.5, -3.25), Sampling::Smooth),
            (Transform::translation(-12.0, 5.0), Sampling::Smooth),
            (Transform::rotation_about(0.52, centre), Sampling::Smooth),
            (
                Transform::scaling_about([0.3, 0.4], centre),
                Sampling::Smooth,
            ),
            (
                Transform::scaling_about([-1.7, 1.3], centre),
                Sampling::Nearest,
            ),
            (Transform::rotation_about(-0.3, centre), Sampling::Nearest),
        ];
        let mask = ugu_core::ops::MaskId(0);
        for history in histories() {
            if history.document().canvas != [120, 80] {
                continue;
            }
            for layer in paint_layers(&history) {
                for keep_source in [false, true] {
                    let (mut split, _) = split(&history, layer, 4);
                    let document = history.document();
                    let selection = document.store.masks.get(&mask).unwrap();
                    let mut moving = split.begin_move(selection, 2).unwrap();
                    // One after another, as a drag shows them.
                    for (transform, sampling) in transforms {
                        split.show_move(&mut moving, transform, sampling, keep_source);
                        let mut moved = History::new(document.clone(), false);
                        moved
                            .edit("Transform", |document| {
                                command::transform_selection(
                                    document,
                                    layer,
                                    mask,
                                    transform,
                                    sampling,
                                    keep_source,
                                )
                            })
                            .unwrap();
                        let mut renderer = DocumentRenderer::new(0);
                        let mut full = canvas(moved.document());
                        let plan = RenderPlan::new(moved.document(), Purpose::Display);
                        renderer.render_plan(moved.document(), &plan, 4, None, &mut full);
                        let Held::Tiles(shown_layer) = &split.sources[split.edited.unwrap()] else {
                            unreachable!("a paint layer's pixels are tiles");
                        };
                        let drawn = renderer.surface(layer).unwrap().to_pixmap();
                        let most = max_difference(&shown_layer.to_pixmap(), &drawn);
                        assert!(
                            most <= 1,
                            "{layer:?} {transform:?} keep {keep_source}: differs by {most}"
                        );
                        let mut shown = canvas(document);
                        composite(&split, None, whole(document), &mut shown);
                        let most = max_difference(&shown, &full);
                        assert!(
                            most <= 2,
                            "{layer:?} {transform:?}: shown differs by {most}"
                        );
                    }
                    split.show_move(
                        &mut moving,
                        Transform::IDENTITY,
                        Sampling::Smooth,
                        keep_source,
                    );
                    let mut shown = canvas(document);
                    composite(&split, None, whole(document), &mut shown);
                    assert!(shown.data_as_u8_slice() == render(document, 4).data_as_u8_slice());
                    split.show_move(&mut moving, transforms[2].0, Sampling::Smooth, keep_source);
                    split.end_move(moving, false);
                    composite(&split, None, whole(document), &mut shown);
                    assert!(shown.data_as_u8_slice() == render(document, 4).data_as_u8_slice());
                }
            }
        }
    }

    #[test]
    fn a_dragged_transform_glanced_at_matches_the_shown_one() {
        use ugu_core::ops::{Affine as Transform, Sampling};
        let centre = [65.0, 45.0];
        let transforms = [
            Transform::translation(8.0, -4.0),
            Transform::rotation_about(0.52, centre),
            Transform::scaling_about([-1.7, 1.3], centre),
            Transform::translation(-12.0, 5.0),
        ];
        let mask = ugu_core::ops::MaskId(0);
        for history in histories() {
            if history.document().canvas != [120, 80] {
                continue;
            }
            let document = history.document();
            let selection = document.store.masks.get(&mask).unwrap();
            for layer in paint_layers(&history) {
                for keep_source in [false, true] {
                    let (mut split, _) = split(&history, layer, 4);
                    let mut moving = split.begin_move(selection, 2).unwrap();
                    let mut same = split.glance(&moving, whole(document), 1, 2).unwrap();
                    let mut corner = split.glance(&moving, [37, 21, 120, 80], 1, 2).unwrap();
                    let mut small = split.glance(&moving, whole(document), 2, 2).unwrap();
                    assert_eq!(corner.origin(), [37, 21]);
                    assert_eq!(small.pixels().width(), 60);
                    small.show(Transform::IDENTITY, Sampling::Smooth, keep_source);
                    let (_, mean) = differences(small.pixels(), &halved(&render(document, 4)));
                    assert!(mean < 0.1, "{layer:?} unmoved: {mean}");
                    for transform in transforms {
                        for glance in [&mut same, &mut corner, &mut small] {
                            glance.show(transform, Sampling::Smooth, keep_source);
                        }
                        split.show_move(&mut moving, transform, Sampling::Smooth, keep_source);
                        let mut shown = canvas(document);
                        composite(&split, None, whole(document), &mut shown);
                        let most = max_difference(same.pixels(), &shown);
                        assert!(most <= 2, "{layer:?} {transform:?}: differs by {most}");
                        let crop = cropped(&shown, [37, 21, 120, 80]);
                        assert!(max_difference(corner.pixels(), &crop) <= 2);
                        // Averaging before blending, and sampling the smaller
                        // pixels, blur edges, but the picture is in place.
                        let (most, mean) = differences(small.pixels(), &halved(&shown));
                        assert!(
                            mean < 1.5,
                            "{layer:?} {transform:?}: mean {mean}, most {most}"
                        );
                    }
                    for glance in [&mut same, &mut small] {
                        glance.show(Transform::IDENTITY, Sampling::Smooth, keep_source);
                    }
                    assert!(
                        same.pixels().data_as_u8_slice() == render(document, 4).data_as_u8_slice()
                    );
                }
            }
        }
    }

    fn cropped(pixmap: &Pixmap, [left, top, right, bottom]: PixelRect) -> Pixmap {
        let mut out = Pixmap::new((right - left) as u16, (bottom - top) as u16);
        let width = usize::from(pixmap.width());
        let from = pixmap.data_as_u8_slice().as_chunks::<4>().0;
        for (row, line) in out
            .data_as_u8_slice_mut()
            .as_chunks_mut::<4>()
            .0
            .chunks_exact_mut((right - left) as usize)
            .enumerate()
        {
            let at = (top as usize + row) * width + left as usize;
            line.copy_from_slice(&from[at..at + line.len()]);
        }
        out
    }

    fn halved(pixmap: &Pixmap) -> Pixmap {
        let [width, height] = [pixmap.width(), pixmap.height()].map(usize::from);
        let from = pixmap.data_as_u8_slice().as_chunks::<4>().0;
        let mut out = Pixmap::new(width.div_ceil(2) as u16, height.div_ceil(2) as u16);
        let out_width = usize::from(out.width());
        for (index, pixel) in out
            .data_as_u8_slice_mut()
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .enumerate()
        {
            let [x, y] = [index % out_width * 2, index / out_width * 2];
            let mut sum = [0u32; 4];
            let mut count = 0;
            for [dx, dy] in [[0, 0], [1, 0], [0, 1], [1, 1]] {
                if x + dx < width && y + dy < height {
                    count += 1;
                    for channel in 0..4 {
                        sum[channel] += u32::from(from[(y + dy) * width + x + dx][channel]);
                    }
                }
            }
            *pixel = sum.map(|value| ((value + count / 2) / count) as u8);
        }
        out
    }

    /// The largest and the mean difference of any channel.
    fn differences(a: &Pixmap, b: &Pixmap) -> (u8, f64) {
        let (a, b) = (a.data_as_u8_slice(), b.data_as_u8_slice());
        assert_eq!(a.len(), b.len());
        let most = a
            .iter()
            .zip(b)
            .map(|(a, b)| a.abs_diff(*b))
            .max()
            .unwrap_or(0);
        let sum: u64 = a
            .iter()
            .zip(b)
            .map(|(a, b)| u64::from(a.abs_diff(*b)))
            .sum();
        (most, sum as f64 / a.len() as f64)
    }
}

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
use ugu_core::store::Stroke;
use vello_cpu::color::AlphaColor;
use vello_cpu::kurbo::Affine;
use vello_cpu::{Pixmap, RasterizerSettings, RenderContext, RenderSettings, Resources};

use crate::composite::{self, Overlay};
use crate::document::{DocumentRenderer, Purpose};
use crate::live::LiveStroke;
use crate::plan::RenderPlan;
use crate::raster::{PixelRect, document_level};
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
    pub plan: RenderPlan,
    /// Premultiplied.
    pub background: [u8; 4],
    /// One per layer of the plan, in its order.
    pub surfaces: Vec<Arc<TiledSurface>>,
}

impl Split {
    /// Where the edited layer is among the plan's layers; `None` when it is
    /// not shown.
    pub fn index(&self) -> Option<usize> {
        self.plan
            .layers
            .iter()
            .position(|(id, _)| *id == self.layer)
    }

    /// Adds a committed stroke to the edited layer as drawing it again would:
    /// painted over, or erased from, what is there. Returns the pixels it
    /// changed.
    pub fn stamp(
        &mut self,
        stamp: &mut Stamp,
        stroke: &Stroke,
        erase: bool,
        wobble: f32,
    ) -> Option<PixelRect> {
        let index = self.index()?;
        let surface = Arc::make_mut(&mut self.surfaces[index]);
        stamp.apply_stroke(surface, stroke, erase, wobble, self.frame)
    }
}

impl DocumentRenderer {
    /// Draws `frame` into `display`, which must have the canvas size, and
    /// keeps what it was made from for editing `layer`. With `revisions`,
    /// layers drawn before are reused. `None` when the stop flag ended it.
    pub fn split(
        &mut self,
        document: &Document,
        layer: LayerId,
        frame: i64,
        revisions: Option<&LayerRevisions>,
        display: &mut Pixmap,
    ) -> Option<Split> {
        let plan = RenderPlan::new(document, Purpose::Display);
        if !self.render_plan(document, &plan, frame, revisions, display) {
            return None;
        }
        let surfaces = plan
            .layers
            .iter()
            .map(|(id, _)| self.surface(*id).expect("just drawn"))
            .collect();
        // The edited layer's pixels change in place from now on, so this
        // renderer does not keep a share of them.
        self.forget(layer);
        Some(Split {
            layer,
            frame: ugu_core::motion::frame_in_cycle(frame, document.frames),
            plan,
            background: premultiplied(document.background.0),
            surfaces,
        })
    }
}

/// Puts the shown image together within `rect`, drawing `live` (a stroke
/// being drawn, `None` for none) on the edited layer first.
pub fn composite(split: &Split, live: Option<&LiveStroke>, rect: PixelRect, out: &mut Pixmap) {
    let surfaces: Vec<Option<&TiledSurface>> = split
        .surfaces
        .iter()
        .map(|surface| Some(&**surface))
        .collect();
    let background = Some(split.background);
    match (live, split.index()) {
        (Some(live), Some(layer)) => {
            let apply = |x: usize, y: usize, pixel: &mut [u8; 4]| live.apply(x, y, pixel);
            let overlay = Overlay {
                layer,
                apply: &apply,
            };
            composite::evaluate_with(&split.plan, background, &surfaces, rect, out, &overlay);
        }
        _ => {
            let [left, top, right, bottom] = rect;
            // Thread start-up outweighs the work below about this many pixels.
            let threads = if (right - left) * (bottom - top) > 65_536 {
                std::thread::available_parallelism().map_or(1, |count| count.get().min(8))
            } else {
                1
            };
            composite::evaluate(&split.plan, background, &surfaces, rect, out, threads, None);
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
    /// Adds a committed stroke to `surface` as drawing the layer again would:
    /// painted over, or erased from, what is there. Returns the pixels it
    /// changed.
    pub fn apply_stroke(
        &mut self,
        surface: &mut TiledSurface,
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
        let rect = clamp(stroke::bounds(&stroke.points, &pen), surface.size())?;
        let samples = Resampler::whole(&stroke.points, stroke::spacing(stroke.width));
        let path = stroke::outline(&samples, &pen, frame)?;
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
                if cover == 0 {
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

#[cfg(test)]
mod tests {
    use super::*;
    use ugu_core::command;
    use ugu_core::document::LayerKind;
    use ugu_core::history::History;
    use ugu_core::ops::{Blend, Rgba8, Wobble};
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

    fn render(document: &Document, frame: i64) -> Pixmap {
        let mut pixmap = Pixmap::new(120, 80);
        DocumentRenderer::new(0).render(document, frame, Purpose::Display, &mut pixmap);
        pixmap
    }

    fn split(history: &History, layer: LayerId, frame: i64) -> (Split, Pixmap) {
        let mut display = Pixmap::new(120, 80);
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
    fn the_split_puts_back_the_full_frame() {
        let history = history();
        let full = render(history.document(), 3);
        let layers = paint_layers(&history);
        assert_eq!(layers.len(), 4);
        for layer in layers {
            let (split, display) = split(&history, layer, 3);
            assert!(display.data_as_u8_slice() == full.data_as_u8_slice());
            let mut shown = Pixmap::new(120, 80);
            composite(&split, None, [0, 0, 120, 80], &mut shown);
            assert!(
                shown.data_as_u8_slice() == full.data_as_u8_slice(),
                "layer {layer:?}"
            );
        }
    }

    #[test]
    fn a_committed_stroke_stamped_on_any_layer_is_within_a_level_of_a_full_render() {
        let mut history = history();
        for layer in paint_layers(&history) {
            for (erase, seed) in [(false, 500), (true, 501)] {
                let (mut split, _) = split(&history, layer, 4);
                let new = stroke([0.0, 40.0], [120.0, 35.0], [90, 20, 150, 180], seed);
                let wobble = history.document().wobble.amount;
                let rect = split
                    .stamp(&mut Stamp::default(), &new, erase, wobble)
                    .unwrap();
                let mut shown = Pixmap::new(120, 80);
                composite(&split, None, [0, 0, 120, 80], &mut shown);
                history
                    .edit("Draw", |document| {
                        command::draw(document, layer, new, erase, None)
                    })
                    .unwrap();
                let most = max_difference(&shown, &render(history.document(), 4));
                assert!(most <= 1, "{layer:?} erase {erase}: differs by {most}");
                assert!(rect[2] > rect[0]);
            }
        }
    }
}

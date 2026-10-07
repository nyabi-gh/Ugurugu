// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! Draws one frame of a document with Vello CPU.
//!
//! Each paint layer and isolated section is a Vello layer: its operations
//! draw on a transparent surface in order, so an eraser removes only what is
//! below it in the same surface, and the surface is then drawn over what is
//! beneath with its opacity. This is the meaning `ugu_core::semantics` pins.
//! Only what M2 can draw is accepted; `check` names the rest.

use ugu_core::document::{Document, LayerKind};
use ugu_core::motion::frame_in_cycle;
use ugu_core::ops::{Blend, Op, Wobble};
use ugu_core::store::{BrushEngine, Store};
use vello_cpu::color::AlphaColor;
use vello_cpu::kurbo::Rect;
use vello_cpu::peniko::{BlendMode, Compose, Mix};
use vello_cpu::{Pixmap, RasterizerSettings, RenderContext, RenderSettings, Resources};

use crate::raster::document_level;
use crate::stroke::{self, Pen, Resampler};

/// Content this build cannot draw yet, and the milestone that adds it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unsupported {
    /// Groups and clipping (M3).
    Group,
    /// Blend modes other than Normal (M3).
    Blend,
    Clipping,
    /// Fills, images, selections and their clips (M4).
    Fill,
    Image,
    Selection,
    /// Crops and resizes (M4).
    CanvasChange,
    /// Airbrush and spray (M4).
    Brush,
}

impl std::fmt::Display for Unsupported {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Group => "layer groups",
            Self::Blend => "blend modes other than Normal",
            Self::Clipping => "clipping layers",
            Self::Fill => "fills",
            Self::Image => "images",
            Self::Selection => "selections",
            Self::CanvasChange => "canvas crops or resizes",
            Self::Brush => "airbrush or spray strokes",
        })
    }
}

/// Whether this build can draw `document`.
pub fn check(document: &Document) -> Result<(), Unsupported> {
    for layer in &document.layers {
        let LayerKind::Paint(paint) = &layer.kind else {
            return Err(Unsupported::Group);
        };
        if paint.blend != Blend::Normal {
            return Err(Unsupported::Blend);
        }
        if paint.clip_to_below {
            return Err(Unsupported::Clipping);
        }
        check_ops(&paint.ops, &document.store)?;
    }
    Ok(())
}

fn check_ops(ops: &[Op], store: &Store) -> Result<(), Unsupported> {
    for op in ops {
        match op {
            Op::Paint { stroke, clip } | Op::Erase { stroke, clip } => {
                if clip.is_some() {
                    return Err(Unsupported::Selection);
                }
                if store
                    .strokes
                    .get(stroke)
                    .is_some_and(|stroke| stroke.brush.engine != BrushEngine::Line)
                {
                    return Err(Unsupported::Brush);
                }
            }
            Op::Fill { .. } => return Err(Unsupported::Fill),
            Op::PlaceImage { .. } => return Err(Unsupported::Image),
            Op::TransformSelection { .. } | Op::ClearSelection { .. } => {
                return Err(Unsupported::Selection);
            }
            Op::Crop { .. } | Op::Resample { .. } => return Err(Unsupported::CanvasChange),
            Op::Isolated(section) => check_ops(&section.ops, store)?,
        }
    }
    Ok(())
}

/// Which layers a frame shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Purpose {
    /// Everything visible, reference layers included.
    Display,
    /// Visible layers except reference layers.
    Export,
}

pub struct DocumentRenderer {
    context: RenderContext,
    resources: Resources,
}

impl DocumentRenderer {
    /// `threads` 0 draws on the calling thread; the result is the same.
    pub fn new(threads: u16) -> Self {
        Self {
            context: RenderContext::new_with(
                1,
                1,
                RenderSettings {
                    level: document_level(),
                    num_threads: threads,
                },
            ),
            resources: Resources::new(),
        }
    }

    /// Draws `frame` of a document that passed `check` into `pixmap`, which
    /// must have the canvas size, as premultiplied RGBA8.
    pub fn render(
        &mut self,
        document: &Document,
        frame: i64,
        purpose: Purpose,
        pixmap: &mut Pixmap,
    ) {
        let [width, height] = document.canvas.map(|edge| edge as u16);
        assert_eq!([pixmap.width(), pixmap.height()], [width, height]);
        let frame = frame_in_cycle(frame, document.frames);
        self.context.reset_and_resize(width, height);
        let [r, g, b, a] = document.background.0;
        self.context.set_paint(AlphaColor::from_rgba8(r, g, b, a));
        self.context
            .fill_rect(&Rect::new(0.0, 0.0, f64::from(width), f64::from(height)));
        for layer in &document.layers {
            if !layer.visible || (purpose == Purpose::Export && layer.reference) {
                continue;
            }
            let LayerKind::Paint(paint) = &layer.kind else {
                continue;
            };
            let wobble = paint.wobble.unwrap_or(document.wobble);
            self.context
                .push_layer(None, None, Some(paint.opacity), None, None);
            self.ops(&paint.ops, &document.store, document.wobble, wobble, frame);
            self.context.pop_layer();
        }
        self.context.flush();
        // Clear first: the layers above draw over it.
        pixmap.data_as_u8_slice_mut().fill(0);
        self.context
            .render_with(pixmap, &mut self.resources, RasterizerSettings::default());
    }

    fn ops(
        &mut self,
        ops: &[Op],
        store: &Store,
        document_wobble: Wobble,
        wobble: Wobble,
        frame: u32,
    ) {
        for op in ops {
            match op {
                Op::Paint { stroke, .. } | Op::Erase { stroke, .. } => {
                    let Some(stroke) = store.strokes.get(stroke) else {
                        continue;
                    };
                    let pen = Pen {
                        width: stroke.width,
                        brush: stroke.brush,
                        seed: stroke.seed,
                        wobble: f64::from(wobble.amount) * f64::from(stroke.brush.wobble_scale),
                    };
                    let samples = Resampler::whole(&stroke.points, stroke::spacing(stroke.width));
                    let Some(path) = stroke::outline(&samples, &pen, frame) else {
                        continue;
                    };
                    let [r, g, b, a] = stroke.color.0;
                    let alpha = (f32::from(a) * stroke.brush.opacity.clamp(0.0, 1.0)).round() as u8;
                    self.context
                        .set_aliasing_threshold((!stroke.brush.antialias).then_some(128));
                    let erase = matches!(op, Op::Erase { .. });
                    if erase {
                        let mode = BlendMode::new(Mix::Normal, Compose::DestOut);
                        self.context.push_layer(None, Some(mode), None, None, None);
                        self.context
                            .set_paint(AlphaColor::from_rgba8(0, 0, 0, alpha));
                    } else {
                        self.context
                            .set_paint(AlphaColor::from_rgba8(r, g, b, alpha));
                    }
                    self.context.fill_path(&path);
                    if erase {
                        self.context.pop_layer();
                    }
                    self.context.set_aliasing_threshold(None);
                }
                Op::Isolated(section) => {
                    let wobble = section.wobble.unwrap_or(document_wobble);
                    self.context
                        .push_layer(None, None, Some(section.opacity), None, None);
                    self.ops(&section.ops, store, document_wobble, wobble, frame);
                    self.context.pop_layer();
                }
                // `check` refuses documents with anything else.
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use ugu_core::document::LayerId;
    use ugu_core::ops::{PaintLayer, Rgba8, Section, StrokeId, merge_down};
    use ugu_core::store::{Brush, Point, Stroke};

    const RED: [u8; 4] = [220, 30, 30, 255];
    const BLUE: [u8; 4] = [30, 30, 220, 255];

    fn document() -> Document {
        let mut document = Document::new([96, 48]);
        document.background = Rgba8([0, 0, 0, 0]);
        document
    }

    /// A horizontal line from `from` to `to` at height `y`.
    fn line(
        document: &mut Document,
        from: f32,
        to: f32,
        y: f32,
        color: [u8; 4],
        antialias: bool,
    ) -> StrokeId {
        let id = StrokeId(document.store.strokes.len() as u32);
        let points: Vec<Point> = (0..=20)
            .map(|step| Point {
                x: from + (to - from) * step as f32 / 20.0,
                y,
                pressure: 1.0,
            })
            .collect();
        document.store.strokes.insert(
            id,
            Stroke {
                points: Arc::from(points),
                color: Rgba8(color),
                width: 8.0,
                brush: Brush {
                    engine: BrushEngine::Line,
                    opacity: 1.0,
                    hardness: 1.0,
                    antialias,
                    size_dynamics: 0.8,
                    wobble_scale: 1.0,
                },
                seed: 0x1234 + u64::from(id.0),
            },
        );
        id
    }

    fn paint_layer(document: &mut Document) -> &mut PaintLayer {
        match &mut document.layers[0].kind {
            LayerKind::Paint(paint) => paint,
            LayerKind::Group(_) => unreachable!(),
        }
    }

    fn render(document: &Document, frame: i64, threads: u16) -> Pixmap {
        let [width, height] = document.canvas.map(|edge| edge as u16);
        let mut pixmap = Pixmap::new(width, height);
        DocumentRenderer::new(threads).render(document, frame, Purpose::Display, &mut pixmap);
        pixmap
    }

    fn at(pixmap: &Pixmap, x: u16, y: u16) -> [u8; 4] {
        let index = (usize::from(y) * usize::from(pixmap.width()) + usize::from(x)) * 4;
        pixmap.data_as_u8_slice()[index..index + 4]
            .try_into()
            .unwrap()
    }

    fn max_difference(a: &Pixmap, b: &Pixmap) -> u8 {
        a.data_as_u8_slice()
            .iter()
            .zip(b.data_as_u8_slice())
            .map(|(a, b)| a.abs_diff(*b))
            .max()
            .unwrap()
    }

    #[test]
    fn an_eraser_reaches_only_what_came_before_it() {
        let mut document = document();
        document.wobble = Wobble::classic(0.0);
        let before = line(&mut document, 8.0, 88.0, 24.0, RED, true);
        let eraser = line(&mut document, 8.0, 88.0, 24.0, [0, 0, 0, 255], true);
        let after = line(&mut document, 56.0, 88.0, 24.0, BLUE, true);
        paint_layer(&mut document).ops = vec![
            Op::Paint {
                stroke: before,
                clip: None,
            },
            Op::Erase {
                stroke: eraser,
                clip: None,
            },
            Op::Paint {
                stroke: after,
                clip: None,
            },
        ];
        let result = render(&document, 0, 0);
        assert_eq!(at(&result, 24, 24), [0, 0, 0, 0]);
        assert_eq!(at(&result, 72, 24), BLUE);
    }

    /// Two layers with their own opacity and motion, each with an eraser that
    /// overlaps the other layer's line.
    fn two_layers() -> (Document, PaintLayer, PaintLayer) {
        let mut document = document();
        let lower_line = line(&mut document, 8.0, 88.0, 20.0, RED, true);
        let lower_eraser = line(&mut document, 8.0, 30.0, 26.0, [0, 0, 0, 255], true);
        let upper_line = line(&mut document, 20.0, 80.0, 28.0, BLUE, true);
        let upper_eraser = line(&mut document, 50.0, 70.0, 22.0, [0, 0, 0, 255], true);
        let mut below = paint_layer(&mut document).clone();
        below.ops = vec![
            Op::Paint {
                stroke: lower_line,
                clip: None,
            },
            Op::Erase {
                stroke: lower_eraser,
                clip: None,
            },
        ];
        below.opacity = 0.6;
        let mut above = below.clone();
        above.ops = vec![
            Op::Paint {
                stroke: upper_line,
                clip: None,
            },
            Op::Erase {
                stroke: upper_eraser,
                clip: None,
            },
        ];
        above.opacity = 0.75;
        above.wobble = Some(Wobble::classic(4.0));
        (document, below, above)
    }

    fn with_layers(document: &Document, layers: &[PaintLayer]) -> Document {
        let mut document = document.clone();
        let template = document.layers[0].clone();
        document.layers = layers
            .iter()
            .enumerate()
            .map(|(index, paint)| {
                let mut layer = template.clone();
                layer.id = LayerId(index as u32 + 1);
                layer.kind = LayerKind::Paint(paint.clone());
                layer
            })
            .collect();
        document
    }

    #[test]
    fn merging_keeps_every_frame_and_each_eraser_in_its_own_layer() {
        let (document, below, above) = two_layers();
        let merged = merge_down(&below, &above).unwrap();
        let separate = with_layers(&document, &[below, above]);
        let joined = with_layers(&document, &[merged]);
        for frame in 0..4 {
            let difference =
                max_difference(&render(&separate, frame, 0), &render(&joined, frame, 0));
            // Opacity is applied once per surface either way; only the u8
            // rounding of nested surfaces may differ.
            assert!(difference <= 2, "frame {frame} differs by {difference}");
        }
        // The upper eraser crosses the lower red line without removing it.
        let red = at(&render(&joined, 0, 0), 60, 20);
        assert!(red[0] > 100 && red[3] > 100, "{red:?}");
    }

    #[test]
    fn an_eraser_added_after_a_merge_reaches_both_layers() {
        let (mut document, below, mut above) = two_layers();
        document.wobble = Wobble::classic(0.0);
        above.wobble = None;
        // Covers y 20..28: the lower line's lower half and the upper line's
        // upper half.
        let eraser = line(&mut document, 36.0, 44.0, 24.0, [0, 0, 0, 255], true);
        let mut merged = merge_down(&below, &above).unwrap();
        merged.ops.push(Op::Erase {
            stroke: eraser,
            clip: None,
        });
        let before = render(&with_layers(&document, &[below, above]), 0, 0);
        assert_ne!(at(&before, 40, 21), [0, 0, 0, 0]);
        assert_ne!(at(&before, 40, 26), [0, 0, 0, 0]);
        let result = render(&with_layers(&document, &[merged]), 0, 0);
        assert_eq!(at(&result, 40, 21), [0, 0, 0, 0]);
        assert_eq!(at(&result, 40, 26), [0, 0, 0, 0]);
        assert_ne!(at(&result, 40, 30), [0, 0, 0, 0]);
    }

    #[test]
    fn every_thread_count_draws_the_same_pixels() {
        let (document, below, above) = two_layers();
        let document = with_layers(&document, &[below, above]);
        let single = render(&document, 5, 0);
        for threads in [1, 4, 8] {
            assert!(
                render(&document, 5, threads).data_as_u8_slice() == single.data_as_u8_slice(),
                "{threads} threads differ"
            );
        }
    }

    #[test]
    fn strokes_move_from_frame_to_frame_and_frames_wrap() {
        let (document, below, above) = two_layers();
        let document = with_layers(&document, &[below, above]);
        let first = render(&document, 0, 0);
        assert!(max_difference(&first, &render(&document, 1, 0)) > 0);
        let wrapped = render(&document, i64::from(document.frames), 0);
        assert!(first.data_as_u8_slice() == wrapped.data_as_u8_slice());
    }

    #[test]
    fn a_pen_without_antialiasing_draws_whole_pixels() {
        let mut document = document();
        let stroke = line(&mut document, 8.0, 88.0, 24.3, RED, false);
        paint_layer(&mut document).ops = vec![Op::Paint { stroke, clip: None }];
        let result = render(&document, 2, 0);
        assert!(
            result
                .data_as_u8_slice()
                .chunks(4)
                .all(|pixel| pixel[3] == 0 || pixel[3] == 255)
        );
    }

    #[test]
    fn hidden_and_reference_layers() {
        let (document, below, above) = two_layers();
        let mut document = with_layers(&document, &[below, above]);
        document.layers[1].visible = false;
        let without_upper = render(&document, 0, 0);
        document.layers[1].visible = true;
        document.layers[1].reference = true;
        let [width, height] = document.canvas.map(|edge| edge as u16);
        let mut exported = Pixmap::new(width, height);
        DocumentRenderer::new(0).render(&document, 0, Purpose::Export, &mut exported);
        assert!(exported.data_as_u8_slice() == without_upper.data_as_u8_slice());
        assert!(max_difference(&exported, &render(&document, 0, 0)) > 0);
    }

    #[test]
    fn what_this_build_cannot_draw_is_named() {
        let mut document = document();
        assert_eq!(check(&document), Ok(()));
        paint_layer(&mut document).ops = vec![Op::Crop {
            offset: [0, 0],
            size: [96, 48],
        }];
        assert_eq!(check(&document), Err(Unsupported::CanvasChange));
        paint_layer(&mut document).ops = vec![Op::Isolated(Box::new(Section {
            ops: vec![Op::ClearSelection {
                mask: ugu_core::ops::MaskId(0),
            }],
            opacity: 1.0,
            wobble: None,
        }))];
        assert_eq!(check(&document), Err(Unsupported::Selection));
        paint_layer(&mut document).ops.clear();
        paint_layer(&mut document).blend = Blend::Multiply;
        assert_eq!(check(&document), Err(Unsupported::Blend));
    }
}

// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! A pending transform shown on the edited layer before it is in the
//! document. The moved part is drawn with the steps the renderer draws
//! `TransformSelection` with, over the layer as it was when the transform
//! began, and only where the shown result changes, so a drag redraws the
//! selection rather than the layer.

use std::sync::Arc;

use ugu_core::ops::{self, Sampling};
use ugu_core::store::Mask;
use vello_cpu::color::{AlphaColor, Srgb};
use vello_cpu::kurbo::{Affine, BezPath, Rect};
use vello_cpu::peniko::{ImageQuality, ImageSampler};
use vello_cpu::{
    Image, ImageSource, Pixmap, RasterizerSettings, RenderContext, RenderSettings, Resources,
};

use crate::compose::{Split, clamp, paint};
use crate::document::{affine, moved_bounds, quality};
use crate::mask::Runs;
use crate::raster::{PixelRect, document_level};
use crate::stream::Held;
use crate::tile::TiledSurface;

pub struct Moving {
    original: Arc<TiledSurface>,
    runs: Runs,
    area: BezPath,
    /// Left, top, right, bottom of the selected pixels.
    bounds: [f64; 4],
    /// Pixels of `original` the moved part is sampled from, and where.
    source: Option<(Arc<Pixmap>, PixelRect)>,
    /// The moved pixels, premultiplied, and how much of each the moved
    /// selection covers, in its alpha.
    moved: Pixmap,
    cover: Pixmap,
    /// What differs from `original` now.
    shown: Option<PixelRect>,
    threads: u16,
    context: RenderContext,
    resources: Resources,
}

fn union(a: Option<PixelRect>, b: Option<PixelRect>) -> Option<PixelRect> {
    match (a, b) {
        (Some(a), Some(b)) => Some([
            a[0].min(b[0]),
            a[1].min(b[1]),
            a[2].max(b[2]),
            a[3].max(b[3]),
        ]),
        (a, None) => a,
        (None, b) => b,
    }
}

fn contains(outer: PixelRect, inner: PixelRect) -> bool {
    outer[0] <= inner[0] && outer[1] <= inner[1] && outer[2] >= inner[2] && outer[3] >= inner[3]
}

fn to_f64(rect: PixelRect) -> [f64; 4] {
    rect.map(f64::from)
}

/// `rect` of `surface` as a pixmap; tiles never drawn are transparent.
fn copy(surface: &TiledSurface, rect: PixelRect) -> Pixmap {
    let [left, top, right, bottom] = rect;
    let width = (right - left) as usize;
    let mut pixmap = Pixmap::new(width as u16, (bottom - top) as u16);
    let out = pixmap.data_as_u8_slice_mut().as_chunks_mut::<4>().0;
    for y in top..bottom {
        let line = (y - top) as usize * width;
        for (x, part) in surface.row(y, left, right) {
            let from = line + (x - left) as usize;
            out[from..from + part.len()].copy_from_slice(part);
        }
    }
    pixmap
}

impl Moving {
    /// Draws the selected part of `original` moved by `transform` into
    /// `self.moved` and how much of each pixel the moved selection covers
    /// into `self.cover`, both then covering `rect`; `false` when no pixel of
    /// the layer lands there. They stay apart because Vello mixes a clip's
    /// edge with what is below in one step.
    fn draw(&mut self, transform: Affine, quality: ImageQuality, rect: PixelRect) -> bool {
        let size = self.original.size();
        let Some(need) = clamp(moved_bounds(to_f64(rect), transform.inverse()), size) else {
            return false;
        };
        if !self
            .source
            .as_ref()
            .is_some_and(|(_, held)| contains(*held, need))
        {
            // Any turn of the selection reads within half its size around it.
            let [left, top, right, bottom] = self.bounds;
            let reach = (right - left).max(bottom - top) / 2.0;
            let around = clamp(
                [left - reach, top - reach, right + reach, bottom + reach],
                size,
            );
            let rect = union(Some(need), around).expect("need is a rectangle");
            self.source = Some((Arc::new(copy(&self.original, rect)), rect));
        }
        let (source, at) = self.source.as_ref().expect("made above");
        let [width, height] = [rect[2] - rect[0], rect[3] - rect[1]].map(|edge| edge as u16);
        let moved = Affine::translate((-f64::from(rect[0]), -f64::from(rect[1]))) * transform;
        let image = Image {
            image: ImageSource::Pixmap(source.clone()),
            sampler: ImageSampler {
                quality,
                ..ImageSampler::default()
            },
        };
        let image_rect = Rect::new(
            0.0,
            0.0,
            f64::from(source.width()),
            f64::from(source.height()),
        );
        let placed = moved * Affine::translate((f64::from(at[0]), f64::from(at[1])));
        self.moved.resize(width, height);
        self.cover.resize(width, height);
        self.context.reset_and_resize(width, height);
        self.context.set_transform(placed);
        self.context.set_paint(image);
        self.context.fill_rect(&image_rect);
        self.context.flush();
        self.context.render_with(
            &mut self.moved,
            &mut self.resources,
            RasterizerSettings::default(),
        );
        self.context.reset_and_resize(width, height);
        self.context.set_transform(moved);
        self.context.set_paint(AlphaColor::<Srgb>::WHITE);
        self.context.fill_path(&self.area);
        self.context.flush();
        self.context.render_with(
            &mut self.cover,
            &mut self.resources,
            RasterizerSettings::default(),
        );
        true
    }
}

impl Split {
    /// Starts showing a transform of `mask`'s part of the edited layer;
    /// `None` when the layer is not shown or the mask is empty.
    pub fn begin_move(&self, mask: &Mask, threads: u16) -> Option<Moving> {
        let Held::Tiles(surface) = &self.sources[self.edited?] else {
            unreachable!("a paint layer's pixels are tiles");
        };
        let runs = Runs::from_mask(mask);
        let bounds = runs.bounds()?.map(f64::from);
        Some(Moving {
            original: surface.clone(),
            area: runs.path(),
            runs,
            bounds,
            source: None,
            moved: Pixmap::new(1, 1),
            cover: Pixmap::new(1, 1),
            shown: None,
            context: RenderContext::new_with(
                1,
                1,
                RenderSettings {
                    level: document_level(),
                    num_threads: threads,
                },
            ),
            threads,
            resources: Resources::new(),
        })
    }

    /// Shows the edited layer as `TransformSelection` with these values
    /// would draw it. Returns the pixels it changed.
    pub fn show_move(
        &mut self,
        moving: &mut Moving,
        transform: ops::Affine,
        sampling: Sampling,
        keep_source: bool,
    ) -> Option<PixelRect> {
        let size = moving.original.size();
        let moved = (transform != ops::Affine::IDENTITY)
            .then(|| {
                let kurbo = affine(transform);
                let rect = clamp(moved_bounds(moving.bounds, kurbo), size)?;
                moving
                    .draw(kurbo, quality(sampling, transform), rect)
                    .then_some(rect)
            })
            .flatten();
        let now = moved.map(|rect| {
            let cleared = (!keep_source).then(|| clamp(moving.bounds, size)).flatten();
            union(Some(rect), cleared).expect("moved is a rectangle")
        });
        let rect = union(moving.shown, now)?;
        moving.shown = now;
        let Held::Tiles(surface) = &mut self.sources[self.edited?] else {
            unreachable!("a paint layer's pixels are tiles");
        };
        let surface = Arc::make_mut(surface);
        surface.ensure(rect);
        let [left, top, right, bottom] = rect;
        let moved_width = usize::from(moving.moved.width());
        let moved_pixels = moving.moved.data_as_u8_slice().as_chunks::<4>().0;
        let cover = moving.cover.data_as_u8_slice().as_chunks::<4>().0;
        let (original, runs) = (&moving.original, &moving.runs);
        let put = |y: u32, line: &mut [[u8; 4]]| {
            line.fill([0; 4]);
            for (x, part) in original.row(y, left, right) {
                let from = (x - left) as usize;
                line[from..from + part.len()].copy_from_slice(part);
            }
            let Some(moved) = moved else {
                return;
            };
            if !keep_source {
                for &[from, to] in runs.row(y as i32) {
                    let span =
                        |edge: i32| (edge.clamp(left as i32, right as i32) - left as i32) as usize;
                    line[span(from)..span(to)].fill([0; 4]);
                }
            }
            if (moved[1]..moved[3]).contains(&y) {
                let row = (y - moved[1]) as usize * moved_width;
                let span = row..row + (moved[2] - moved[0]) as usize;
                let target = &mut line[(moved[0] - left) as usize..(moved[2] - left) as usize];
                for (target, (source, cover)) in target
                    .iter_mut()
                    .zip(moved_pixels[span.clone()].iter().zip(&cover[span]))
                {
                    if cover[3] != 0 {
                        paint(target, *source, cover[3]);
                    }
                }
            }
        };
        let mut rows: Vec<_> = surface.rows_mut(rect).collect();
        // Thread start-up outweighs the work below about this many pixels.
        let threads = if (right - left) * (bottom - top) > 65_536 {
            usize::from(moving.threads.max(1))
        } else {
            1
        };
        let chunk = rows.len().div_ceil(threads);
        std::thread::scope(|scope| {
            for part in rows.chunks_mut(chunk) {
                scope.spawn(|| {
                    for (y, line) in part {
                        put(*y, line);
                    }
                });
            }
        });
        Some(rect)
    }

    /// Ends showing a transform: when it is applied the shown pixels stay,
    /// otherwise the layer as it was comes back. Returns the pixels changed.
    pub fn end_move(&mut self, moving: Moving, applied: bool) -> Option<PixelRect> {
        if applied {
            return None;
        }
        let edited = self.edited?;
        self.sources[edited] = Held::Tiles(moving.original);
        moving.shown
    }
}

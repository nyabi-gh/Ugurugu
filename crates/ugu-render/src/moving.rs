// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! A pending transform shown on the edited layer before it is in the
//! document. The moved part is drawn with the steps the renderer draws
//! `TransformSelection` with, over the layer as it was when the transform
//! began, and only where the shown result changes, so a drag redraws the
//! selection rather than the layer. While it is dragged, a `Glance` shows it
//! only where it is seen and no larger than it is seen.

use std::sync::Arc;

use ugu_core::ops::{self, Sampling};
use ugu_core::store::Mask;
use vello_cpu::color::{AlphaColor, Srgb};
use vello_cpu::kurbo::{Affine, BezPath, Rect};
use vello_cpu::peniko::{ImageQuality, ImageSampler};
use vello_cpu::{Image, ImageSource, Pixmap, RasterizerSettings, RenderContext, Resources};

use crate::compose::{Split, clamp, paint};
use crate::composite::{self, Put, Source};
use crate::document::{affine, moved_bounds, quality};
use crate::mask::Runs;
use crate::raster::{PixelRect, document_level};
use crate::stream::Held;
use crate::tile::{Pixel, TiledSurface};

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
            context: crate::raster::context(document_level(), threads),
            threads,
            resources: Resources::new(),
        })
    }

    /// Starts showing `moving` within `rect` of the canvas at 1/`shrink` of
    /// its size; `None` when the edited layer is not shown or `rect` is
    /// empty.
    pub fn glance(
        &self,
        moving: &Moving,
        rect: PixelRect,
        shrink: u32,
        threads: u16,
    ) -> Option<Glance> {
        let edited = self.edited?;
        let size = moving.original.size();
        let rect = aligned(rect, shrink, size)?;
        let selected = aligned(clamp(moving.bounds, size)?, shrink, size)?;
        let count = usize::from(threads.max(1));
        let [left, right] = [rect[0], rect[2]];
        let (original, runs) = (&moving.original, &moving.runs);
        let sources = self
            .sources
            .iter()
            .enumerate()
            .map(|(index, held)| {
                if index == edited {
                    return Pixmap::new(1, 1);
                }
                reduce(rect, shrink, count, |y, line| match held {
                    Held::Tiles(surface) => tiles_row(surface, y, left, line),
                    Held::Whole(pixmap) => {
                        let width = usize::from(pixmap.width());
                        let from = y as usize * width + left as usize;
                        let pixels = pixmap.data_as_u8_slice().as_chunks::<4>().0;
                        line.copy_from_slice(&pixels[from..from + (right - left) as usize]);
                    }
                })
            })
            .collect();
        let kept = reduce(rect, shrink, count, |y, line| {
            tiles_row(original, y, left, line);
        });
        let cleared = reduce(rect, shrink, count, |y, line| {
            tiles_row(original, y, left, line);
            for &[from, to] in runs.row(y as i32) {
                let span =
                    |edge: i32| (edge.clamp(left as i32, right as i32) - left as i32) as usize;
                line[span(from)..span(to)].fill([0; 4]);
            }
        });
        let source = reduce(selected, shrink, count, |y, line| {
            tiles_row(original, y, selected[0], line);
        });
        let out = Pixmap::new(kept.width(), kept.height());
        Some(Glance {
            rect,
            shrink,
            puts: self.puts.clone(),
            background: self.background,
            sources,
            edited,
            kept,
            cleared,
            source: Arc::new(source),
            source_at: [selected[0], selected[1]],
            area: moving.area.clone(),
            bounds: moving.bounds,
            shown: None,
            moved: Pixmap::new(1, 1),
            cover: Pixmap::new(1, 1),
            context: crate::raster::context(document_level(), threads),
            resources: Resources::new(),
            threads: count,
            out,
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

/// A pending transform while it is dragged: the split within `rect` of the
/// canvas, each `shrink`² pixels averaged into one, put together the same
/// way with the moved part drawn as small. The edited layer itself is left
/// for `show_move` when the drag ends; averaging before blending makes this
/// differ from the full image made smaller by a level or so.
pub struct Glance {
    rect: PixelRect,
    shrink: u32,
    puts: Vec<Put>,
    background: Option<Pixel>,
    sources: Vec<Pixmap>,
    edited: usize,
    /// The edited layer as the transform began, and without the selected
    /// part.
    kept: Pixmap,
    cleared: Pixmap,
    /// The edited layer around the selection, and where it starts on the
    /// canvas.
    source: Arc<Pixmap>,
    source_at: [u32; 2],
    area: BezPath,
    bounds: [f64; 4],
    /// Where the moved part was last drawn, and whether the selected part
    /// also stayed.
    shown: Option<(Option<PixelRect>, bool)>,
    moved: Pixmap,
    cover: Pixmap,
    context: RenderContext,
    resources: Resources,
    threads: usize,
    out: Pixmap,
}

impl Glance {
    /// The pixels shown.
    pub fn pixels(&self) -> &Pixmap {
        &self.out
    }

    /// Where the pixels start on the canvas.
    pub fn origin(&self) -> [u32; 2] {
        [self.rect[0], self.rect[1]]
    }

    /// How many canvas pixels each pixel shown spans each way.
    pub fn shrink(&self) -> u32 {
        self.shrink
    }

    /// The canvas rectangle and shrink it was made for.
    pub fn view(&self) -> (PixelRect, u32) {
        (self.rect, self.shrink)
    }

    /// Shows the transform as `show_move` would, smaller. Returns the
    /// pixels shown that changed.
    pub fn show(
        &mut self,
        transform: ops::Affine,
        sampling: Sampling,
        keep_source: bool,
    ) -> Option<PixelRect> {
        let size = [self.out.width(), self.out.height()].map(u32::from);
        let shrink = f64::from(self.shrink);
        let view = Affine::scale(1.0 / shrink)
            * Affine::translate((-f64::from(self.rect[0]), -f64::from(self.rect[1])));
        let moving = transform != ops::Affine::IDENTITY;
        let keep = keep_source || !moving;
        let kurbo = view * affine(transform);
        let moved = moving
            .then(|| clamp(moved_bounds(self.bounds, kurbo), size))
            .flatten();
        let moved = moved.filter(|&rect| self.draw(kurbo, quality(sampling, transform), rect));
        let changed = match self.shown {
            Some((before, kept)) if kept == keep => union(before, moved),
            _ => Some([0, 0, size[0], size[1]]),
        };
        self.shown = Some((moved, keep));
        let rect = changed?;
        let [left, top, right, bottom] = rect.map(|edge| edge as usize);
        let width = size[0] as usize;
        let edited = &mut self.sources[self.edited];
        if [edited.width(), edited.height()] != [self.out.width(), self.out.height()] {
            *edited = Pixmap::new(self.out.width(), self.out.height());
        }
        let base = if keep { &self.kept } else { &self.cleared };
        let base = base.data_as_u8_slice().as_chunks::<4>().0;
        let moved_width = usize::from(self.moved.width());
        let moved_pixels = self.moved.data_as_u8_slice().as_chunks::<4>().0;
        let cover = self.cover.data_as_u8_slice().as_chunks::<4>().0;
        let rows = edited.data_as_u8_slice_mut().as_chunks_mut::<4>().0
            [top * width..bottom * width]
            .chunks_exact_mut(width);
        let put = |y: usize, line: &mut [Pixel]| {
            line[left..right].copy_from_slice(&base[y * width + left..y * width + right]);
            let Some([from, high, to, low]) = moved.map(|rect| rect.map(|edge| edge as usize))
            else {
                return;
            };
            if (high..low).contains(&y) {
                let row = (y - high) * moved_width;
                for (target, (source, cover)) in line[from..to].iter_mut().zip(
                    moved_pixels[row..row + to - from]
                        .iter()
                        .zip(&cover[row..row + to - from]),
                ) {
                    if cover[3] != 0 {
                        paint(target, *source, cover[3]);
                    }
                }
            }
        };
        let mut rows: Vec<_> = rows.enumerate().collect();
        let chunk = rows.len().div_ceil(self.threads).max(1);
        std::thread::scope(|scope| {
            for part in rows.chunks_mut(chunk) {
                scope.spawn(|| {
                    for (row, line) in part {
                        put(top + *row, line);
                    }
                });
            }
        });
        let sources: Vec<Source<'_>> = self.sources.iter().map(Source::Whole).collect();
        composite::evaluate(
            &self.puts,
            self.background,
            &sources,
            rect,
            &mut self.out,
            self.threads,
            None,
        );
        Some(rect)
    }

    /// Draws the selected part moved by `transform` (canvas to shown
    /// pixels) into `self.moved` and its cover into `self.cover`, both then
    /// covering `rect`, as `Moving::draw` does; `false` when nothing lands
    /// there.
    fn draw(&mut self, transform: Affine, quality: ImageQuality, rect: PixelRect) -> bool {
        let [width, height] = [rect[2] - rect[0], rect[3] - rect[1]].map(|edge| edge as u16);
        if width == 0 || height == 0 {
            return false;
        }
        let shift = Affine::translate((-f64::from(rect[0]), -f64::from(rect[1])));
        let placed = shift
            * transform
            * Affine::translate((f64::from(self.source_at[0]), f64::from(self.source_at[1])))
            * Affine::scale(f64::from(self.shrink));
        let image = Image {
            image: ImageSource::Pixmap(self.source.clone()),
            sampler: ImageSampler {
                quality,
                ..ImageSampler::default()
            },
        };
        let image_rect = Rect::new(
            0.0,
            0.0,
            f64::from(self.source.width()),
            f64::from(self.source.height()),
        );
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
        self.context.set_transform(shift * transform);
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

/// `rect` with its left and top moved out to a multiple of `shrink`, within
/// a canvas of `size`; `None` when empty.
fn aligned(rect: PixelRect, shrink: u32, size: [u32; 2]) -> Option<PixelRect> {
    let rect = [
        rect[0] / shrink * shrink,
        rect[1] / shrink * shrink,
        rect[2].min(size[0]),
        rect[3].min(size[1]),
    ];
    (rect[0] < rect[2] && rect[1] < rect[3]).then_some(rect)
}

/// Row `y` of `surface` from `left` into `line`, transparent where nothing
/// is drawn.
fn tiles_row(surface: &TiledSurface, y: u32, left: u32, line: &mut [Pixel]) {
    line.fill([0; 4]);
    let right = left + line.len() as u32;
    for (x, part) in surface.row(y, left, right) {
        let from = (x - left) as usize;
        line[from..from + part.len()].copy_from_slice(part);
    }
}

/// `rect` of a canvas with each `shrink`² block of premultiplied pixels
/// averaged into one; a block cut by the canvas edge averages the pixels it
/// has. `row` fills a canvas row of `rect` given its y.
fn reduce(
    rect: PixelRect,
    shrink: u32,
    threads: usize,
    row: impl Fn(u32, &mut [Pixel]) + Sync,
) -> Pixmap {
    let [left, top, right, bottom] = rect;
    let span = (right - left) as usize;
    let block = shrink as usize;
    let width = span.div_ceil(block);
    let height = (bottom - top).div_ceil(shrink) as usize;
    let mut out = Pixmap::new(width as u16, height as u16);
    let lines = out.data_as_u8_slice_mut().as_chunks_mut::<4>().0;
    let per = height.div_ceil(threads.max(1)).max(1);
    let row = &row;
    std::thread::scope(|scope| {
        for (part, chunk) in lines.chunks_mut(per * width).enumerate() {
            scope.spawn(move || {
                let mut line = vec![[0; 4]; span];
                let mut sums = vec![[0u32; 4]; width];
                for (index, target) in chunk.chunks_exact_mut(width).enumerate() {
                    let first = top + ((part * per + index) * block) as u32;
                    let last = (first + shrink).min(bottom);
                    sums.fill([0; 4]);
                    for y in first..last {
                        row(y, &mut line);
                        if block == 1 {
                            target.copy_from_slice(&line);
                            continue;
                        }
                        for (sum, pixels) in sums.iter_mut().zip(line.chunks(block)) {
                            for pixel in pixels {
                                for channel in 0..4 {
                                    sum[channel] += u32::from(pixel[channel]);
                                }
                            }
                        }
                    }
                    if block == 1 {
                        continue;
                    }
                    let rows = last - first;
                    for (x, (target, sum)) in target.iter_mut().zip(&sums).enumerate() {
                        let count = rows * (span - x * block).min(block) as u32;
                        *target = sum.map(|value| ((value + count / 2) / count) as u8);
                    }
                }
            });
        }
    });
    out
}

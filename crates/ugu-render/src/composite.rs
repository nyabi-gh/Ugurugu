// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! Puts layer surfaces together as a `RenderPlan` says, one row at a time,
//! in premultiplied RGBA8.
//!
//! Normal rounds as Vello's u8 pipeline does, so a layer drawn on its own
//! surface and put over the one below gives the same bytes as Vello drawing
//! both in one scene. Multiply, Screen and Overlay are the W3C separable
//! formulas on premultiplied values, rounded to the nearest level.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use ugu_core::ops::Blend;
use vello_cpu::Pixmap;

use crate::compose::src_over;
use crate::plan::{Composite, RenderPlan, Step};
use crate::raster::PixelRect;
use crate::tile::TiledSurface;

use crate::tile::Pixel;

/// `a·b/255` as Vello's u8 pipeline rounds it.
fn mul(a: u8, b: u8) -> u8 {
    ((u16::from(a) * u16::from(b) + 255) >> 8) as u8
}

fn div255(value: u32) -> u8 {
    ((value + 127) / 255) as u8
}

fn opacity_level(opacity: f32) -> u8 {
    (opacity.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// Premultiplied `source` over `target` with `mode`.
pub fn blend(mode: Blend, target: &mut Pixel, source: Pixel) {
    if source[3] == 0 {
        return;
    }
    if mode == Blend::Normal {
        src_over(target, &source);
        return;
    }
    let (sa, ba) = (u32::from(source[3]), u32::from(target[3]));
    let alpha = div255(255 * sa + 255 * ba - sa * ba);
    for channel in 0..3 {
        let (s, b) = (u32::from(source[channel]), u32::from(target[channel]));
        // Each term stays within 255², and the sum is at most 255·alpha.
        let value = match mode {
            Blend::Normal => unreachable!(),
            Blend::Multiply => s * (255 - ba) + b * (255 - sa) + s * b,
            Blend::Screen => 255 * s + 255 * b - s * b,
            Blend::Overlay => {
                let rest = s * (255 - ba) + b * (255 - sa);
                if 2 * b <= ba {
                    rest + 2 * s * b
                } else {
                    rest + sa * ba - 2 * (ba - b) * (sa - s)
                }
            }
        };
        target[channel] = div255(value).min(alpha);
    }
    target[3] = alpha;
}

/// One step of putting sources together. A program is made from a render
/// plan by `program`, or by the editing split, which puts together ahead of
/// time what does not depend on the layer being edited.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Put {
    /// Starts a group's transparent surface.
    Begin,
    /// Puts the group begun last over the surface below it.
    End(Composite),
    /// Puts the next source over the current surface.
    Over(Composite),
    /// Fills the current surface with the next source as it is, keeping its
    /// base.
    Backdrop,
    /// Makes the next source, at this opacity, the base that clipped steps
    /// after it are cut to.
    Base(f32),
}

/// The program that puts `plan`'s layers together, one source per layer.
pub fn program(plan: &RenderPlan) -> Vec<Put> {
    plan.steps
        .iter()
        .map(|step| match step {
            Step::Begin(_) => Put::Begin,
            Step::End(_, composite) => Put::End(*composite),
            Step::Paint(_, composite) => Put::Over(*composite),
        })
        .collect()
}

/// Pixels a step takes: a layer's tiles, a whole surface of the program's
/// size, or nothing (transparent).
#[derive(Clone, Copy)]
pub enum Source<'a> {
    Tiles(&'a TiledSurface),
    Whole(&'a Pixmap),
    Empty,
}

impl<'a> Source<'a> {
    /// The parts of row `y` within `left..right` with pixels, each as where
    /// it starts in the span and its pixels.
    fn parts(self, y: usize, left: usize, right: usize, mut each: impl FnMut(usize, &'a [Pixel])) {
        match self {
            Self::Tiles(surface) => {
                for (x, part) in surface.row(y as u32, left as u32, right as u32) {
                    each(x as usize - left, part);
                }
            }
            Self::Whole(pixmap) => {
                let width = usize::from(pixmap.width());
                let pixels = pixmap.data_as_u8_slice().as_chunks::<4>().0;
                each(0, &pixels[y * width + left..y * width + right]);
            }
            Self::Empty => {}
        }
    }

    /// Row `y` within `left..right`, transparent where there is nothing.
    fn row(self, y: usize, left: usize, right: usize) -> Vec<Pixel> {
        let mut row = vec![[0; 4]; right - left];
        self.parts(y, left, right, |start, part| {
            row[start..start + part.len()].copy_from_slice(part);
        });
        row
    }
}

/// One surface being put together: the document, or a group.
struct Level {
    row: Vec<Pixel>,
    /// The latest unclipped surface and its opacity, while a clipped
    /// sibling still follows.
    base: Option<(Vec<Pixel>, u8)>,
}

/// For each step, whether a clipped sibling follows it before the next
/// unclipped one, so its pixels must be kept as the base.
fn bases_needed(puts: &[Put]) -> Vec<bool> {
    let mut needed = vec![false; puts.len()];
    let mut last_unclipped: Vec<Option<usize>> = vec![None];
    for (index, put) in puts.iter().enumerate() {
        let composite = match put {
            Put::Begin => {
                last_unclipped.push(None);
                continue;
            }
            Put::End(composite) => {
                last_unclipped.pop();
                composite
            }
            Put::Over(composite) => composite,
            Put::Backdrop => continue,
            Put::Base(_) => {
                *last_unclipped.last_mut().expect("the document level") = None;
                continue;
            }
        };
        let last = last_unclipped.last_mut().expect("the document level");
        if composite.clipped {
            if let Some(base) = *last {
                needed[base] = true;
            }
        } else {
            *last = Some(index);
        }
    }
    needed
}

/// Rows a thread puts together at a time.
const STRIPE: usize = 16;

/// Runs `puts` with `sources` (one per `Over`, `Backdrop` and `Base`, in
/// order) over `background` within `rect` of `out`, on up to `threads`
/// threads. Returns `false` when `stop` was set before every row was done.
pub fn evaluate(
    puts: &[Put],
    background: Option<Pixel>,
    sources: &[Source<'_>],
    rect: PixelRect,
    out: &mut Pixmap,
    threads: usize,
    stop: Option<&AtomicBool>,
) -> bool {
    let width = usize::from(out.width());
    let [left, top, right, bottom] = rect.map(|value| value as usize);
    if left >= right || top >= bottom {
        return true;
    }
    let needed = bases_needed(puts);
    let row_bytes = width * 4;
    let data = &mut out.data_as_u8_slice_mut()[top * row_bytes..bottom * row_bytes];
    let stripe = |first: usize, target: &mut [u8], levels: &mut Vec<Level>| {
        for (offset, line) in target.chunks_mut(row_bytes).enumerate() {
            let y = first + offset;
            let line = &mut line.as_chunks_mut::<4>().0[left..right];
            let row = Row {
                puts,
                needed: &needed,
                background,
                sources,
                span: [left, right],
                y,
            };
            row.evaluate(levels, None);
            line.copy_from_slice(&levels[0].row);
        }
    };
    let stripes: Vec<std::sync::Mutex<&mut [u8]>> = data
        .chunks_mut(STRIPE * row_bytes)
        .map(std::sync::Mutex::new)
        .collect();
    let next = AtomicUsize::new(0);
    let stopped = AtomicBool::new(false);
    let work = || {
        let mut levels = Vec::new();
        loop {
            let index = next.fetch_add(1, Ordering::Relaxed);
            let Some(target) = stripes.get(index) else {
                return;
            };
            if stop.is_some_and(|stop| stop.load(Ordering::Relaxed)) {
                stopped.store(true, Ordering::Relaxed);
                return;
            }
            let mut target = target.lock().expect("one thread per stripe");
            stripe(top + index * STRIPE, &mut target, &mut levels);
        }
    };
    if threads <= 1 || stripes.len() < 2 {
        work();
    } else {
        std::thread::scope(|scope| {
            for _ in 0..threads.min(stripes.len()) {
                scope.spawn(work);
            }
        });
    }
    !stopped.load(Ordering::Relaxed)
}

/// Something drawn on one source's pixels as they are put together, such as
/// a stroke being drawn.
pub struct Overlay<'a> {
    /// The source's place in the sources.
    pub source: usize,
    /// Changes the source's pixel at `x`, `y`.
    pub apply: &'a dyn Fn(usize, usize, &mut Pixel),
}

/// `evaluate` on the calling thread, with `overlay` on its source.
pub fn evaluate_with(
    puts: &[Put],
    background: Option<Pixel>,
    sources: &[Source<'_>],
    rect: PixelRect,
    out: &mut Pixmap,
    overlay: &Overlay<'_>,
) {
    let width = usize::from(out.width());
    let [left, top, right, bottom] = rect.map(|value| value as usize);
    if left >= right || top >= bottom {
        return;
    }
    let needed = bases_needed(puts);
    let mut levels = Vec::new();
    let pixels = out.data_as_u8_slice_mut().as_chunks_mut::<4>().0;
    for y in top..bottom {
        let row = Row {
            puts,
            needed: &needed,
            background,
            sources,
            span: [left, right],
            y,
        };
        row.evaluate(&mut levels, Some(overlay));
        pixels[y * width + left..y * width + right].copy_from_slice(&levels[0].row);
    }
}

/// One row of a program run.
struct Row<'a, 'b> {
    puts: &'a [Put],
    needed: &'a [bool],
    background: Option<Pixel>,
    sources: &'a [Source<'b>],
    span: [usize; 2],
    y: usize,
}

impl Row<'_, '_> {
    fn evaluate(&self, levels: &mut Vec<Level>, overlay: Option<&Overlay<'_>>) {
        let [left, right] = self.span;
        let span = right - left;
        let start = |depth: usize, levels: &mut Vec<Level>, fill: Pixel| {
            if levels.len() == depth {
                levels.push(Level {
                    row: Vec::new(),
                    base: None,
                });
            }
            let level = &mut levels[depth];
            level.row.clear();
            level.row.resize(span, fill);
            level.base = None;
        };
        start(0, levels, self.background.unwrap_or([0; 4]));
        let mut depth = 0;
        let mut next = 0;
        for (index, put) in self.puts.iter().enumerate() {
            match put {
                Put::Begin => {
                    depth += 1;
                    start(depth, levels, [0; 4]);
                }
                Put::End(composite) => {
                    depth -= 1;
                    let (lower, upper) = levels.split_at_mut(depth + 1);
                    let parts = std::iter::once((0, upper[0].row.as_slice()));
                    over(
                        &mut lower[depth],
                        parts,
                        *composite,
                        self.needed[index],
                        span,
                    );
                }
                Put::Over(composite) => {
                    let source = self.sources[next];
                    let level = &mut levels[depth];
                    match overlay.filter(|overlay| overlay.source == next) {
                        Some(overlay) => {
                            let mut row = source.row(self.y, left, right);
                            for (offset, pixel) in row.iter_mut().enumerate() {
                                (overlay.apply)(left + offset, self.y, pixel);
                            }
                            let whole = std::iter::once((0, row.as_slice()));
                            over(level, whole, *composite, self.needed[index], span);
                        }
                        None => {
                            let mut parts = Vec::new();
                            source.parts(self.y, left, right, |start, part| {
                                parts.push((start, part));
                            });
                            over(
                                level,
                                parts.into_iter(),
                                *composite,
                                self.needed[index],
                                span,
                            );
                        }
                    }
                    next += 1;
                }
                Put::Backdrop => {
                    let level = &mut levels[depth];
                    level.row.fill([0; 4]);
                    self.sources[next].parts(self.y, left, right, |start, part| {
                        level.row[start..start + part.len()].copy_from_slice(part);
                    });
                    next += 1;
                }
                Put::Base(opacity) => {
                    let row = self.sources[next].row(self.y, left, right);
                    levels[depth].base = Some((row, opacity_level(*opacity)));
                    next += 1;
                }
            }
        }
    }
}

/// Puts `parts` of a surface (where each starts in the span, and its
/// pixels; transparent elsewhere) over `level`.
fn over<'a>(
    level: &mut Level,
    parts: impl Iterator<Item = (usize, &'a [Pixel])>,
    composite: Composite,
    keep: bool,
    span: usize,
) {
    let opacity = opacity_level(composite.opacity);
    let base = if composite.clipped {
        level.base.take()
    } else {
        None
    };
    let mut kept = (keep && !composite.clipped).then(|| vec![[0; 4]; span]);
    for (start, part) in parts {
        if let Some(kept) = kept.as_mut() {
            kept[start..start + part.len()].copy_from_slice(part);
        }
        let targets = &mut level.row[start..start + part.len()];
        for (offset, (target, &pixel)) in targets.iter_mut().zip(part).enumerate() {
            if pixel[3] == 0 {
                continue;
            }
            let mut pixel = pixel;
            if let Some((base, base_opacity)) = &base {
                let cover = mul(base[start + offset][3], *base_opacity);
                pixel = pixel.map(|value| mul(value, cover));
            }
            blend(
                composite.blend,
                target,
                pixel.map(|value| mul(value, opacity)),
            );
        }
    }
    level.base = if composite.clipped {
        base
    } else {
        kept.map(|kept| (kept, opacity))
    };
}

/// A base kept by `Stream`.
pub enum Kept {
    Tiles(TiledSurface),
    Whole(Pixmap),
}

impl Kept {
    fn source(&self) -> Source<'_> {
        match self {
            Self::Tiles(surface) => Source::Tiles(surface),
            Self::Whole(pixmap) => Source::Whole(pixmap),
        }
    }
}

/// Runs a program one step at a time over whole surfaces, so a step's
/// source can be made just before it and let go after: only a surface per
/// open group and the current base stay. Each pixel goes through the same
/// arithmetic in the same order as `evaluate`, so the result is the same.
pub struct Stream {
    size: [u32; 2],
    threads: usize,
    needed: Vec<bool>,
    /// The step to run next.
    at: usize,
    levels: Vec<(Pixmap, Option<(Kept, u8)>)>,
}

impl Stream {
    pub fn new(puts: &[Put], size: [u32; 2], background: Option<Pixel>, threads: usize) -> Self {
        let mut root = Pixmap::new(size[0] as u16, size[1] as u16);
        if let Some(background) = background {
            root.data_as_u8_slice_mut()
                .as_chunks_mut::<4>()
                .0
                .fill(background);
        }
        Self {
            size,
            threads,
            needed: bases_needed(puts),
            at: 0,
            levels: vec![(root, None)],
        }
    }

    /// Runs `put`, the next step of the program, with `source` for steps
    /// that take one.
    pub fn step(&mut self, put: Put, source: Option<Kept>) {
        let keep = self.needed[self.at];
        self.at += 1;
        match put {
            Put::Begin => {
                let surface = Pixmap::new(self.size[0] as u16, self.size[1] as u16);
                self.levels.push((surface, None));
            }
            Put::End(composite) => {
                let (group, _) = self.levels.pop().expect("a group was begun");
                self.over(Kept::Whole(group), composite, keep);
            }
            Put::Over(composite) => {
                self.over(source.expect("a source for the step"), composite, keep);
            }
            Put::Backdrop => {
                let source = source.expect("a source for the step");
                let (target, _) = self.levels.last_mut().expect("the document level");
                let [width, height] = self.size.map(|edge| edge as usize);
                let out = target.data_as_u8_slice_mut().as_chunks_mut::<4>().0;
                for y in 0..height {
                    let row = source.source().row(y, 0, width);
                    out[y * width..(y + 1) * width].copy_from_slice(&row);
                }
            }
            Put::Base(opacity) => {
                let source = source.expect("a source for the step");
                self.levels.last_mut().expect("the document level").1 =
                    Some((source, opacity_level(opacity)));
            }
        }
    }

    /// The finished document.
    pub fn finish(mut self) -> Pixmap {
        assert_eq!(self.levels.len(), 1, "every group was ended");
        self.levels.pop().expect("the document level").0
    }

    fn over(&mut self, source: Kept, composite: Composite, keep: bool) {
        let width = self.size[0] as usize;
        let threads = self.threads;
        let (target, base) = self.levels.last_mut().expect("the document level");
        let base_ref = if composite.clipped {
            base.as_ref()
        } else {
            None
        };
        let stripes: Vec<std::sync::Mutex<(usize, &mut [Pixel])>> = target
            .data_as_u8_slice_mut()
            .as_chunks_mut::<4>()
            .0
            .chunks_mut(STRIPE * width)
            .enumerate()
            .map(|(index, rows)| std::sync::Mutex::new((index * STRIPE, rows)))
            .collect();
        let next = AtomicUsize::new(0);
        let work = || {
            loop {
                let index = next.fetch_add(1, Ordering::Relaxed);
                let Some(stripe) = stripes.get(index) else {
                    return;
                };
                let mut stripe = stripe.lock().expect("one thread per stripe");
                let (first, rows) = &mut *stripe;
                for (offset, line) in rows.chunks_mut(width).enumerate() {
                    let y = *first + offset;
                    let mut level = Level {
                        row: line.to_vec(),
                        base: base_ref
                            .map(|(kept, opacity)| (kept.source().row(y, 0, width), *opacity)),
                    };
                    let mut parts = Vec::new();
                    source
                        .source()
                        .parts(y, 0, width, |start, part| parts.push((start, part)));
                    over(&mut level, parts.into_iter(), composite, false, width);
                    line.copy_from_slice(&level.row);
                }
            }
        };
        if threads <= 1 || stripes.len() < 2 {
            work();
        } else {
            std::thread::scope(|scope| {
                for _ in 0..threads.min(stripes.len()) {
                    scope.spawn(work);
                }
            });
        }
        drop(stripes);
        if !composite.clipped {
            *base = keep.then(|| (source, opacity_level(composite.opacity)));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The W3C formula in f32 on premultiplied values.
    fn reference(mode: Blend, source: Pixel, backdrop: Pixel) -> [f32; 4] {
        let s = source.map(|value| f32::from(value) / 255.0);
        let b = backdrop.map(|value| f32::from(value) / 255.0);
        let (sa, ba) = (s[3], b[3]);
        let mut out = [0.0; 4];
        for c in 0..3 {
            let mixed = match mode {
                Blend::Normal => s[c] * ba,
                Blend::Multiply => s[c] * b[c],
                Blend::Screen => s[c] * ba + b[c] * sa - s[c] * b[c],
                Blend::Overlay if 2.0 * b[c] <= ba => 2.0 * s[c] * b[c],
                Blend::Overlay => sa * ba - 2.0 * (ba - b[c]) * (sa - s[c]),
            };
            out[c] = s[c] * (1.0 - ba) + b[c] * (1.0 - sa) + mixed;
        }
        out[3] = sa + ba - sa * ba;
        out.map(|value| value * 255.0)
    }

    /// A premultiplied pixel from a counter.
    fn pixel(seed: u32) -> Pixel {
        let mut state = seed.wrapping_mul(2_654_435_761) ^ 0x9e37_79b9;
        let mut next = || {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            (state >> 8) as u8
        };
        let alpha = next();
        [
            mul(next(), alpha),
            mul(next(), alpha),
            mul(next(), alpha),
            alpha,
        ]
    }

    #[test]
    fn blend_modes_are_within_one_level_of_the_formulas() {
        for mode in [
            Blend::Normal,
            Blend::Multiply,
            Blend::Screen,
            Blend::Overlay,
        ] {
            for seed in 0..20_000 {
                let (source, backdrop) = (pixel(seed), pixel(seed + 1_000_000));
                let mut target = backdrop;
                blend(mode, &mut target, source);
                let expected = reference(mode, source, backdrop);
                for channel in 0..4 {
                    let off = (f32::from(target[channel]) - expected[channel]).abs();
                    assert!(
                        off <= 1.0,
                        "{mode:?} {source:?} over {backdrop:?}: {target:?} vs {expected:?}"
                    );
                }
                assert!(target[..3].iter().all(|&value| value <= target[3]));
            }
        }
    }

    #[test]
    fn a_base_is_kept_only_while_clipped_siblings_follow() {
        let over = |clipped| {
            Put::Over(Composite {
                blend: Blend::Normal,
                opacity: 1.0,
                clipped,
            })
        };
        let puts = [
            over(false),
            over(true),
            Put::Begin,
            over(false),
            over(false),
            Put::End(Composite {
                blend: Blend::Normal,
                opacity: 1.0,
                clipped: false,
            }),
            over(true),
            over(false),
            Put::Backdrop,
            Put::Base(0.5),
            over(true),
        ];
        assert_eq!(
            bases_needed(&puts),
            [
                true, false, false, false, false, true, false, false, false, false, false
            ]
        );
    }
}

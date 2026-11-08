// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! The line around a paint layer's pixels (`ugu_core::ops::Outline`): an
//! exact distance to the nearest covered pixel, in the layer's own pixels,
//! before it is put together with the others (decisions.md section 2).

use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use ugu_core::ops::Outline;
use vello_cpu::Pixmap;

use crate::raster::PixelRect;
use crate::tile::{Pixel, TiledSurface};

/// Columns swept together, so the sweep reads whole cache lines of a row.
const STRIP: usize = 64;

/// How far, in document pixels, a pixel looks for the strongest alpha around
/// it to tell how much it covers (see [`under`]).
const NEARBY: usize = 6;

/// Thread start-up outweighs the work below about this many pixels.
const WORTH_THREADS: usize = 65_536;

/// An outline at the size a layer is drawn.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Look {
    width: f32,
    reach: usize,
    nearby: usize,
    color: [u8; 3],
    opacity: u32,
}

impl Look {
    pub fn new(outline: &Outline, shrink: u32) -> Self {
        let width = outline.width / shrink as f32;
        let [r, g, b, a] = outline.color.0;
        Self {
            width,
            reach: (width + 0.5).ceil() as usize,
            nearby: (NEARBY / shrink as usize).max(1),
            color: [r, g, b],
            opacity: u32::from(a),
        }
    }

    /// How far a changed pixel can change the outline.
    fn margin(&self) -> usize {
        self.reach.max(self.nearby)
    }

    /// The bytes a surface of `size` can take for this outline, at most,
    /// around a layer of `extent` pixels.
    pub fn bytes_around(&self, extent: [u16; 2], size: [u32; 2]) -> usize {
        let grown =
            |axis: usize| (usize::from(extent[axis]) + 2 * self.reach).min(size[axis] as usize);
        grown(0) * grown(1) * 4
    }
}

/// Buffers kept from one outline to the next: a document's worth of them
/// for every layer of every frame costs milliseconds.
#[derive(Default)]
pub struct Scratch {
    pixels: Vec<Pixel>,
    heights: Vec<u16>,
    across: Vec<u8>,
    prefix: Vec<u8>,
    suffix: Vec<u8>,
    most: Vec<u8>,
    spare: Vec<Pixmap>,
}

impl Scratch {
    /// Keeps `surface`'s pixels for the next outline of its size.
    pub fn give_back(&mut self, mut surface: TiledSurface) {
        self.spare.extend(surface.clear());
    }

    /// Lets go of the pixels `give_back` kept that nothing has used.
    pub fn drop_spares(&mut self) {
        self.spare.clear();
    }

    fn pixmap(&mut self, [width, height]: [u16; 2]) -> Pixmap {
        let found = self
            .spare
            .iter()
            .position(|pixmap| (pixmap.width(), pixmap.height()) == (width, height));
        found
            .map(|at| self.spare.swap_remove(at))
            .unwrap_or_else(|| Pixmap::new(width, height))
    }

    fn sized(&mut self, length: usize) {
        self.pixels.resize(length, [0; 4]);
        self.heights.resize(length, 0);
    }
}

/// `work` on each `chunk` long piece of `data`, by its index, on `threads`
/// threads. Each thread makes its own state with `state`.
fn each_chunk<T: Send, S>(
    data: &mut [T],
    chunk: usize,
    threads: usize,
    state: impl Fn() -> S + Sync,
    work: impl Fn(&mut S, usize, &mut [T]) + Sync,
) {
    if threads <= 1 {
        let mut state = state();
        for (index, part) in data.chunks_mut(chunk).enumerate() {
            work(&mut state, index, part);
        }
        return;
    }
    let chunks: Vec<Mutex<&mut [T]>> = data.chunks_mut(chunk).map(Mutex::new).collect();
    let next = AtomicUsize::new(0);
    let run = || {
        let mut state = state();
        loop {
            let index = next.fetch_add(1, Ordering::Relaxed);
            let Some(part) = chunks.get(index) else {
                return;
            };
            work(
                &mut state,
                index,
                &mut part.lock().expect("one thread per chunk"),
            );
        }
    };
    std::thread::scope(|scope| {
        for _ in 1..threads {
            scope.spawn(run);
        }
        run();
    });
}

/// `strip` holds the columns from `from`, one row after another: how far each
/// is along its column from the nearest covered pixel, `limit` or more when
/// that is out of reach.
fn sweep(strip: &mut [u16], from: usize, down: usize, across: usize, pixels: &[Pixel], limit: u16) {
    let columns = strip.len() / down;
    let mut last = [limit; STRIP];
    let last = &mut last[..columns];
    for (y, row) in strip.chunks_exact_mut(columns).enumerate() {
        let line = &pixels[y * across + from..y * across + from + columns];
        for ((out, at), pixel) in row.iter_mut().zip(last.iter_mut()).zip(line) {
            *at = if pixel[3] > 0 {
                0
            } else {
                at.saturating_add(1)
            };
            *out = *at;
        }
    }
    last.fill(limit);
    for (y, row) in strip.chunks_exact_mut(columns).enumerate().rev() {
        let line = &pixels[y * across + from..y * across + from + columns];
        for ((out, at), pixel) in row.iter_mut().zip(last.iter_mut()).zip(line) {
            *at = if pixel[3] > 0 {
                0
            } else {
                at.saturating_add(1)
            };
            *out = (*out).min(*at);
        }
    }
}

/// Squared distance to the nearest covered pixel over one row, from how far
/// each column's nearest is (Felzenszwalb and Huttenlocher), for the columns
/// from `first` as long as `out`. The inside of a covered run cannot be the
/// nearest to anything outside it, so only its ends go in; the caller knows
/// those inside are at 0. False when nothing is in reach.
fn envelope(
    out: &mut [f32],
    heights: &[u16],
    limit: u16,
    first: usize,
    v: &mut Vec<usize>,
    z: &mut Vec<f64>,
) -> bool {
    v.clear();
    z.clear();
    let at = |q: usize| f64::from(heights[q]) * f64::from(heights[q]);
    let intersect = |q: usize, p: usize| {
        ((at(q) + (q * q) as f64) - (at(p) + (p * p) as f64)) / (2.0 * (q as f64 - p as f64))
    };
    let last = heights.len() - 1;
    for q in 0..heights.len() {
        let height = heights[q];
        if height >= limit
            || (height == 0 && q > 0 && q < last && heights[q - 1] == 0 && heights[q + 1] == 0)
        {
            continue;
        }
        while let (Some(&p), Some(&from)) = (v.last(), z.last()) {
            if intersect(q, p) <= from {
                v.pop();
                z.pop();
            } else {
                break;
            }
        }
        let from = v.last().map_or(f64::NEG_INFINITY, |&p| intersect(q, p));
        v.push(q);
        z.push(from);
    }
    if v.is_empty() {
        return false;
    }
    let mut k = 0;
    for (x, slot) in (first..).zip(out.iter_mut()) {
        while k + 1 < v.len() && z[k + 1] < x as f64 {
            k += 1;
        }
        let p = v[k];
        let dx = x as f64 - p as f64;
        *slot = (dx * dx + at(p)) as f32;
    }
    true
}

/// The strongest alpha within `radius` along a row (van Herk, Gil and
/// Werman), `row` being padded with `radius` zeros at both ends so every
/// window is whole.
fn widest_in_row(
    out: &mut [u8],
    row: &[Pixel],
    radius: usize,
    prefix: &mut [u8],
    suffix: &mut [u8],
) {
    let window = 2 * radius + 1;
    let padded = |at: usize| {
        at.checked_sub(radius)
            .and_then(|at| row.get(at))
            .map_or(0, |pixel| pixel[3])
    };
    let length = row.len() + 2 * radius;
    for at in 0..length {
        let here = padded(at);
        prefix[at] = if at % window == 0 {
            here
        } else {
            prefix[at - 1].max(here)
        };
    }
    for at in (0..length).rev() {
        let here = padded(at);
        suffix[at] = if at + 1 == length || (at + 1) % window == 0 {
            here
        } else {
            suffix[at + 1].max(here)
        };
    }
    for (at, slot) in out.iter_mut().enumerate() {
        *slot = suffix[at].max(prefix[at + 2 * radius]);
    }
}

/// Fills `scratch.most` with the strongest alpha within `radius` of each
/// pixel of `scratch.pixels`, `across` wide, in a square. The vertical pass
/// works on whole rows so that it stays in cache order.
fn strongest_nearby(
    scratch: &mut Scratch,
    across: usize,
    down: usize,
    radius: usize,
    threads: usize,
) {
    let window = 2 * radius + 1;
    let rows = down + 2 * radius;
    scratch.across.resize(across * down, 0);
    scratch.prefix.resize(across * rows, 0);
    scratch.suffix.resize(across * rows, 0);
    scratch.most.resize(across * down, 0);
    let Scratch {
        pixels,
        across: wide,
        prefix,
        suffix,
        most,
        ..
    } = scratch;
    let pixels = &*pixels;
    each_chunk(
        wide,
        across,
        threads,
        || (vec![0; across + 2 * radius], vec![0; across + 2 * radius]),
        |(before, after), y, row| {
            let source = &pixels[y * across..(y + 1) * across];
            widest_in_row(row, source, radius, before, after);
        },
    );
    let wide = &*wide;
    // Row `y` of the input padded with `radius` empty rows at both ends.
    let lay = |into: &mut [u8], y: usize, over: bool| {
        let from = y
            .checked_sub(radius)
            .filter(|y| *y < down)
            .map(|y| &wide[y * across..(y + 1) * across]);
        match from {
            Some(from) if over => {
                for (slot, value) in into.iter_mut().zip(from) {
                    *slot = (*slot).max(*value);
                }
            }
            Some(from) => into.copy_from_slice(from),
            None if over => {}
            None => into.fill(0),
        }
    };
    each_chunk(
        prefix,
        across * window,
        threads,
        || (),
        |(), block, part| {
            for index in 0..part.len() / across {
                let (before, rest) = part.split_at_mut(index * across);
                let row = &mut rest[..across];
                let y = block * window + index;
                if index == 0 {
                    lay(row, y, false);
                } else {
                    row.copy_from_slice(&before[(index - 1) * across..]);
                    lay(row, y, true);
                }
            }
        },
    );
    each_chunk(
        suffix,
        across * window,
        threads,
        || (),
        |(), block, part| {
            let count = part.len() / across;
            for index in (0..count).rev() {
                let (rest, after) = part.split_at_mut((index + 1) * across);
                let row = &mut rest[index * across..];
                let y = block * window + index;
                if index + 1 == count {
                    lay(row, y, false);
                } else {
                    row.copy_from_slice(&after[..across]);
                    lay(row, y, true);
                }
            }
        },
    );
    let (prefix, suffix) = (&*prefix, &*suffix);
    each_chunk(
        most,
        across,
        threads,
        || (),
        |(), y, row| {
            let ends = &suffix[y * across..(y + 1) * across];
            let starts = &prefix[(y + 2 * radius) * across..(y + 2 * radius + 1) * across];
            for ((slot, end), start) in row.iter_mut().zip(ends).zip(starts) {
                *slot = (*end).max(*start);
            }
        },
    );
}

/// How much of the line lies under a covered pixel of `alpha`, out of 255. A
/// pixel as strong as those around it is the inside of what is drawn, and
/// the line would show through a translucent stroke; a weaker one is its
/// edge, soft or antialiased, and needs the line right up to it
/// (evidence.md section 112).
#[inline]
fn under(alpha: u8, strongest: u8) -> u32 {
    match alpha {
        0 => 255,
        255 => 0,
        _ => 255 - u32::from(alpha) * 255 / u32::from(strongest),
    }
}

/// Pixels of the layer, row `y` from column `left`, into a row left empty.
pub type Read<'a> = &'a (dyn Fn(usize, usize, &mut [Pixel]) + Sync);

struct Target<'a> {
    pixels: &'a mut [Pixel],
    stride: usize,
    origin: [usize; 2],
}

struct Row {
    heights: Vec<u16>,
    squares: Vec<f32>,
    v: Vec<usize>,
    z: Vec<f64>,
}

/// Writes the layer `read` gives, with its outline under it, over `keep` of
/// `target`. What lies in `area` is all that is looked at.
fn draw(
    read: Read<'_>,
    target: Target<'_>,
    [left, top, right, bottom]: [usize; 4],
    [keep_left, keep_top, keep_right, keep_bottom]: [usize; 4],
    look: &Look,
    threads: usize,
    scratch: &mut Scratch,
) {
    let limit = look.reach as u16;
    let (across, down) = (right - left, bottom - top);
    scratch.sized(across * down);
    each_chunk(
        &mut scratch.pixels,
        across,
        threads,
        || (),
        |(), y, row| {
            row.fill([0; 4]);
            read(y + top, left, row);
        },
    );
    {
        let Scratch {
            pixels, heights, ..
        } = scratch;
        let held = &*pixels;
        each_chunk(
            heights,
            STRIP * down,
            threads,
            || (),
            |(), strip, part| sweep(part, strip * STRIP, down, across, held, limit),
        );
    }
    strongest_nearby(scratch, across, down, look.nearby, threads);
    let solid = if look.width >= 0.5 {
        (look.width - 0.5).powi(2)
    } else {
        -1.0
    };
    let far = (look.width + 0.5).powi(2);
    let first = keep_left - left;
    let shown = keep_right - keep_left;
    let Scratch {
        pixels,
        heights: swept,
        most,
        ..
    } = &*scratch;
    let Target {
        pixels: out,
        stride,
        origin,
    } = target;
    let rows = &mut out[(keep_top - origin[1]) * stride..(keep_bottom - origin[1]) * stride];
    let [line_left, line_right] = [keep_left - origin[0], keep_right - origin[0]];
    each_chunk(
        rows,
        stride,
        threads,
        || Row {
            heights: vec![0; across],
            squares: vec![0.0; shown],
            v: Vec::new(),
            z: Vec::new(),
        },
        |state, index, row| {
            let at = index + keep_top - top;
            let src = &pixels[at * across + first..at * across + first + shown];
            let inside = &mut row[line_left..line_right];
            for (strip, part) in state.heights.chunks_mut(STRIP).enumerate() {
                let from = strip * STRIP * down + at * part.len();
                part.copy_from_slice(&swept[from..from + part.len()]);
            }
            let Row {
                heights,
                squares,
                v,
                z,
            } = state;
            if !envelope(squares, heights, limit, first, v, z) {
                inside.copy_from_slice(src);
                return;
            }
            let strongest = &most[at * across + first..at * across + first + shown];
            for (((pixel, &from), &strongest), (&height, &square)) in inside
                .iter_mut()
                .zip(src)
                .zip(strongest)
                .zip(heights[first..first + shown].iter().zip(squares.iter()))
            {
                let square = if height == 0 { 0.0 } else { square };
                let line = if square >= far {
                    0
                } else if square <= solid {
                    255
                } else {
                    let cover = (look.width + 0.5 - square.sqrt()).clamp(0.0, 1.0);
                    (cover * 255.0).round() as u32
                };
                let add = (line * under(from[3], strongest) + 127) / 255;
                let add = (add * look.opacity + 127) / 255;
                let tint = |channel: u8| ((u32::from(channel) * add + 127) / 255) as u8;
                *pixel = [
                    from[0] + tint(look.color[0]),
                    from[1] + tint(look.color[1]),
                    from[2] + tint(look.color[2]),
                    from[3] + add as u8,
                ];
            }
        },
    );
}

fn worth(rect: [usize; 4], threads: usize) -> usize {
    if (rect[2] - rect[0]) * (rect[3] - rect[1]) > WORTH_THREADS {
        threads
    } else {
        1
    }
}

/// `source` over its outline, on a surface of its own.
pub fn around(
    source: &TiledSurface,
    look: &Look,
    threads: usize,
    scratch: &mut Scratch,
) -> TiledSurface {
    let mut out = TiledSurface::new(source.size(), source.edge());
    let Some((span, reached)) = source.reached_around(look.reach) else {
        return out;
    };
    let (origin, extent) = out.area(span);
    out.set(span, scratch.pixmap(extent), reached);
    let origin = origin.map(|at| at as usize);
    let rect = [
        origin[0],
        origin[1],
        origin[0] + usize::from(extent[0]),
        origin[1] + usize::from(extent[1]),
    ];
    let read = |y: usize, left: usize, row: &mut [Pixel]| {
        for (x, part) in source.row(y as u32, left as u32, (left + row.len()) as u32) {
            let from = x as usize - left;
            row[from..from + part.len()].copy_from_slice(part);
        }
    };
    let pixels = out.pixels_mut().expect("just set");
    let target = Target {
        pixels,
        stride: usize::from(extent[0]),
        origin,
    };
    // Nothing lies outside `rect`: it holds the layer and the reach around it.
    draw(
        &read,
        target,
        rect,
        rect,
        look,
        worth(rect, threads),
        scratch,
    );
    out
}

/// Brings `outlined`, the layer with its outline, up to date after the
/// layer's pixels changed in `dirty`; `read` gives the layer as it is now.
/// Returns the pixels of `outlined` that were written.
pub fn redraw(
    read: Read<'_>,
    outlined: &mut TiledSurface,
    dirty: PixelRect,
    look: &Look,
    threads: usize,
    scratch: &mut Scratch,
) -> PixelRect {
    let [width, height] = outlined.size();
    let margin = look.margin() as u32;
    let grown = |[left, top, right, bottom]: PixelRect| {
        [
            left.saturating_sub(margin),
            top.saturating_sub(margin),
            (right + margin).min(width),
            (bottom + margin).min(height),
        ]
    };
    let keep = grown(dirty);
    if keep[0] >= keep[2] || keep[1] >= keep[3] {
        return keep;
    }
    let area = grown(keep).map(|at| at as usize);
    outlined.ensure(keep);
    let (origin, extent) = outlined.area(outlined.span().expect("just ensured"));
    let pixels = outlined.pixels_mut().expect("just ensured");
    let target = Target {
        pixels,
        stride: usize::from(extent[0]),
        origin: origin.map(|at| at as usize),
    };
    let rect = keep.map(|at| at as usize);
    draw(
        read,
        target,
        area,
        rect,
        look,
        worth(area, threads),
        scratch,
    );
    keep
}

#[cfg(test)]
mod tests {
    use super::*;
    use ugu_core::ops::Rgba8;

    fn look(width: f32, color: [u8; 4]) -> Look {
        Look::new(
            &Outline {
                width,
                color: Rgba8(color),
            },
            1,
        )
    }

    fn surface(size: [u32; 2], edge: u32, pixels: &[(u32, u32, Pixel)]) -> TiledSurface {
        let mut surface = TiledSurface::new(size, edge);
        for &(x, y, pixel) in pixels {
            surface.ensure([x, y, x + 1, y + 1]);
            for (_, row) in surface.rows_mut([x, y, x + 1, y + 1]) {
                row[0] = pixel;
            }
        }
        surface
    }

    fn scattered(w: u32, h: u32) -> Vec<(u32, u32, Pixel)> {
        let mut state = 12345u32;
        let mut pixels = Vec::new();
        for _ in 0..40 {
            state = state.wrapping_mul(1664525).wrapping_add(1013904223);
            let (x, y) = ((state >> 8) % w, (state >> 20) % h);
            let alpha = 1 + (state >> 3) as u8 % 255;
            pixels.push((x, y, [alpha / 2, 0, alpha / 3, alpha]));
        }
        for (x, y) in (20..34).flat_map(|x| (10..24).map(move |y| (x, y))) {
            pixels.push((x, y, [90, 0, 0, 90]));
        }
        pixels
    }

    fn whole(surface: &TiledSurface) -> Vec<Pixel> {
        surface
            .to_pixmap()
            .data_as_u8_slice()
            .as_chunks::<4>()
            .0
            .to_vec()
    }

    #[test]
    fn the_line_is_where_a_search_for_the_nearest_covered_pixel_says() {
        let (w, h) = (64u32, 48u32);
        let pixels = scattered(w, h);
        let source = surface([w, h], 8, &pixels);
        let drawn = whole(&source);
        let look = look(5.0, [255, 255, 255, 255]);
        let got = whole(&around(&source, &look, 1, &mut Scratch::default()));
        let covered: Vec<_> = (0..h)
            .flat_map(|y| (0..w).map(move |x| (x, y)))
            .filter(|&(x, y)| drawn[(y * w + x) as usize][3] > 0)
            .collect();
        for y in 0..h {
            for x in 0..w {
                let at = (y * w + x) as usize;
                if drawn[at][3] > 0 {
                    continue;
                }
                let nearest = covered
                    .iter()
                    .map(|&(cx, cy)| {
                        let (dx, dy) = (cx as f32 - x as f32, cy as f32 - y as f32);
                        (dx * dx + dy * dy).sqrt()
                    })
                    .fold(f32::MAX, f32::min);
                let want = ((5.5 - nearest).clamp(0.0, 1.0) * 255.0).round() as i32;
                assert!((i32::from(got[at][3]) - want).abs() <= 1, "{x}, {y}");
            }
        }
    }

    #[test]
    fn the_strongest_alpha_nearby_is_what_looking_at_each_pixel_finds() {
        let (w, h) = (64usize, 48usize);
        let source = surface([w as u32, h as u32], 8, &scattered(w as u32, h as u32));
        let drawn = whole(&source);
        for (radius, threads) in [(1, 1), (6, 1), (6, 4), (20, 4)] {
            let mut scratch = Scratch {
                pixels: drawn.clone(),
                ..Scratch::default()
            };
            strongest_nearby(&mut scratch, w, h, radius, threads);
            for y in 0..h {
                for x in 0..w {
                    let mut most = 0;
                    for ny in y.saturating_sub(radius)..(y + radius + 1).min(h) {
                        for nx in x.saturating_sub(radius)..(x + radius + 1).min(w) {
                            most = most.max(drawn[ny * w + nx][3]);
                        }
                    }
                    assert_eq!(scratch.most[y * w + x], most, "{x}, {y} radius {radius}");
                }
            }
        }
    }

    #[test]
    fn the_line_does_not_show_through_a_translucent_inside() {
        let pixels: Vec<_> = (10..30)
            .flat_map(|x| (10..30).map(move |y| (x, y, [40u8, 0, 0, 102u8])))
            .collect();
        let source = surface([40, 40], 8, &pixels);
        let got = whole(&around(
            &source,
            &look(3.0, [255, 0, 0, 255]),
            1,
            &mut Scratch::default(),
        ));
        assert_eq!(got[20 * 40 + 20], [40, 0, 0, 102]);
        assert_eq!(got[20 * 40 + 8], [255, 0, 0, 255]);
    }

    #[test]
    fn threads_and_tile_sizes_change_nothing() {
        let (w, h) = (96u32, 80u32);
        let pixels = scattered(w, h);
        let look = look(12.5, [10, 200, 30, 180]);
        let expected = whole(&around(
            &surface([w, h], 8, &pixels),
            &look,
            1,
            &mut Scratch::default(),
        ));
        for (edge, threads) in [(8, 4), (16, 1), (32, 8), (4, 2)] {
            let got = around(
                &surface([w, h], edge, &pixels),
                &look,
                threads,
                &mut Scratch::default(),
            );
            assert!(
                whole(&got) == expected,
                "tiles of {edge}, {threads} threads"
            );
        }
    }

    #[test]
    fn a_redraw_of_what_changed_is_what_drawing_it_all_again_gives() {
        let (w, h) = (96u32, 80u32);
        let before = scattered(w, h);
        for width in [1.0, 4.0, 11.5, 60.0] {
            let look = look(width, [255, 0, 0, 200]);
            let mut scratch = Scratch::default();
            let mut layer = surface([w, h], 8, &before);
            let mut outlined = around(&layer, &look, 1, &mut scratch);
            // Paints, then takes away, in different places.
            for (index, dirty) in [[50, 40, 70, 60], [20, 10, 34, 24], [0, 0, 6, 6]]
                .into_iter()
                .enumerate()
            {
                layer.ensure(dirty);
                for (_, row) in layer.rows_mut(dirty) {
                    row.fill(if index == 1 { [0; 4] } else { [0, 80, 0, 80] });
                }
                let read = |y: usize, left: usize, row: &mut [Pixel]| {
                    for (x, part) in layer.row(y as u32, left as u32, (left + row.len()) as u32) {
                        let from = x as usize - left;
                        row[from..from + part.len()].copy_from_slice(part);
                    }
                };
                redraw(&read, &mut outlined, dirty, &look, 1, &mut scratch);
                let full = around(&layer, &look, 1, &mut Scratch::default());
                assert!(
                    whole(&outlined) == whole(&full),
                    "width {width}, change {index}"
                );
            }
        }
    }
}

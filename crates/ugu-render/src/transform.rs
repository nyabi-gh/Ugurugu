// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! Layers moved by their keys, sampled one row at a time as they are put
//! together.
//!
//! Each pixel takes its value from where its centre lands on the layer's
//! own surface, by that alone, so a frame put together in pieces, on any
//! number of threads, has the same bytes as one put together whole.

use ugu_core::ops::Affine;
use vello_cpu::Pixmap;

use crate::composite::Source;
use crate::raster::PixelRect;
use crate::tile::Pixel;

/// Where a layer's pixels go, and how they are read.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Moved {
    /// From the layer's pixels to where they are shown.
    pub forward: Affine,
    /// From where they are shown back to the layer's pixels.
    inverse: Affine,
    /// The nearest pixel, for pixel mode; otherwise the four around it,
    /// weighted.
    pub nearest: bool,
    /// `inverse` in 2⁻³² pixels from a pixel's index, less half a pixel:
    /// where each pixel reads, by whole-number sums only.
    fixed: [i64; 6],
}

impl Moved {
    /// `None` when `forward` cannot be undone, which keys never make.
    pub fn new(forward: Affine, nearest: bool) -> Option<Self> {
        let inverse = forward.inverse()?;
        let [a, b, c, d, e, f] = inverse.0;
        // A pixel's centre is half a pixel past its index.
        let fixed = [a, b, c + (a + b - 1.0) / 2.0, d, e, f + (d + e - 1.0) / 2.0]
            .map(|value| (value * ONE as f64).round() as i64);
        Some(Self {
            forward,
            inverse,
            nearest,
            fixed,
        })
    }

    /// The same move for surfaces drawn at 1/`shrink` of the canvas size.
    pub fn scaled(self, shrink: u32) -> Self {
        if shrink == 1 {
            return self;
        }
        let shrink = f64::from(shrink);
        let [a, b, c, d, e, f] = self.forward.0;
        Self::new(Affine([a, b, c / shrink, d, e, f / shrink]), self.nearest)
            .expect("the same turn and scale as before")
    }

    /// Whether it only moves by whole pixels, so every pixel is copied.
    fn whole_pixels(&self) -> Option<[i64; 2]> {
        let [a, b, c, d, e, f] = self.forward.0;
        (a == 1.0 && b == 0.0 && d == 0.0 && e == 1.0 && c.fract() == 0.0 && f.fract() == 0.0)
            .then_some([c as i64, f as i64])
    }

    /// The pixels a rectangle of the layer's (left, top, right, bottom)
    /// reaches once moved, within `size`.
    pub fn reach(&self, rect: PixelRect, size: [u32; 2]) -> Option<PixelRect> {
        let [left, top, right, bottom] = rect.map(f64::from);
        let corners = [[left, top], [right, top], [left, bottom], [right, bottom]]
            .map(|corner| self.forward.apply(corner));
        let fold = |axis: usize, pick: fn(f64, f64) -> f64, start: f64| {
            corners.iter().map(|corner| corner[axis]).fold(start, pick)
        };
        // A pixel reads its neighbours, so one more each way.
        let reached = [
            (fold(0, f64::min, f64::INFINITY).floor() - 1.0).max(0.0),
            (fold(1, f64::min, f64::INFINITY).floor() - 1.0).max(0.0),
            (fold(0, f64::max, f64::NEG_INFINITY).ceil() + 1.0).min(f64::from(size[0])),
            (fold(1, f64::max, f64::NEG_INFINITY).ceil() + 1.0).min(f64::from(size[1])),
        ];
        (reached[0] < reached[2] && reached[1] < reached[3])
            .then(|| reached.map(|edge| edge as u32))
    }
}

/// Changes a source's pixel at `x`, `y` before it is read, such as a stroke
/// being drawn on it.
pub type Touch<'a> = &'a dyn Fn(usize, usize, &mut Pixel);

/// Row `y` within `left..right` of `source` moved by `moved`.
pub fn row(
    source: Source<'_>,
    moved: &Moved,
    y: usize,
    left: usize,
    right: usize,
    touch: Option<Touch<'_>>,
) -> Vec<Pixel> {
    let mut row = vec![[0; 4]; right - left];
    let Some(lookup) = Lookup::of(source, touch.is_some()) else {
        return row;
    };
    let read = |x: i64, y: i64| -> Pixel {
        let mut pixel = lookup.get(x, y);
        if let Some(touch) = touch
            && lookup.inside(x, y)
        {
            touch(x as usize, y as usize, &mut pixel);
        }
        pixel
    };
    if let Some([dx, dy]) = moved.whole_pixels() {
        let from = y as i64 - dy;
        for (offset, pixel) in row.iter_mut().enumerate() {
            *pixel = read((left + offset) as i64 - dx, from);
        }
        return row;
    }
    // Only pixels whose centres land near the pixels held can read any;
    // the rest stay transparent.
    let Some([first, last]) = lookup.span(&moved.inverse, y as f64 + 0.5, left, right) else {
        return row;
    };
    let [a, b, c, d, e, f] = moved.fixed;
    let (row_y, from) = (y as i64, first as i64);
    // Whole-number sums are exact, so stepping gives each pixel the same
    // place wherever the row starts.
    let mut sx = a * from + b * row_y + c;
    let mut sy = d * from + e * row_y + f;
    for pixel in &mut row[first - left..last - left] {
        let (at_x, at_y) = (sx, sy);
        sx += a;
        sy += d;
        if moved.nearest {
            *pixel = read((at_x + HALF) >> 32, (at_y + HALF) >> 32);
            continue;
        }
        // In 256ths of a pixel, rounded: the whole pixel and the share of
        // the next.
        let [qx, qy] = [at_x, at_y].map(|value| (value + (1 << 23)) >> 24);
        let (x0, y0) = (qx >> 8, qy >> 8);
        let (wx, wy) = ((qx & 255) as u64, (qy & 255) as u64);
        let taps = match touch {
            None => lookup.quad(x0, y0),
            Some(_) => [
                read(x0, y0),
                read(x0 + 1, y0),
                read(x0, y0 + 1),
                read(x0 + 1, y0 + 1),
            ],
        };
        if taps.iter().all(|tap| tap[3] == 0) {
            continue;
        }
        let weights = [
            (256 - wx) * (256 - wy),
            wx * (256 - wy),
            (256 - wx) * wy,
            wx * wy,
        ];
        // Two channels to a u64, 32 bits apart: a weighted sum stays below
        // 255 · 65536, so they never reach each other.
        let (mut even, mut odd) = (0u64, 0u64);
        for (tap, weight) in taps.iter().zip(weights) {
            even += (u64::from(tap[0]) | u64::from(tap[2]) << 32) * weight;
            odd += (u64::from(tap[1]) | u64::from(tap[3]) << 32) * weight;
        }
        let round = 32_768 | 32_768 << 32;
        let (even, odd) = ((even + round) >> 16, (odd + round) >> 16);
        *pixel = [even as u8, odd as u8, (even >> 32) as u8, (odd >> 32) as u8];
    }
    row
}

/// A pixel in the fixed point of `Moved::fixed`, and half of one.
const ONE: i64 = 1 << 32;
const HALF: i64 = 1 << 31;

/// A source's pixels, read at random.
struct Lookup<'a> {
    pixels: &'a [Pixel],
    stride: usize,
    rows: usize,
    /// Where `pixels` start in the source.
    origin: [i64; 2],
    /// Which tiles of a tiled surface hold anything, their columns and
    /// their edge.
    tiles: Option<(&'a [bool], usize, Edge)>,
    /// The source's size; reads outside it are transparent.
    size: [i64; 2],
    /// Left, top, right and bottom of what can read as anything.
    reach: [i64; 4],
}

#[derive(Clone, Copy)]
enum Edge {
    Shift(u32),
    Length(i64),
}

impl<'a> Lookup<'a> {
    /// `None` when nothing reads as anything. `everywhere` counts every
    /// pixel as possibly something, for a change drawn over the source.
    fn of(source: Source<'a>, everywhere: bool) -> Option<Self> {
        let size = source.size()?.map(i64::from);
        let mut lookup = Self {
            pixels: &[],
            stride: 0,
            rows: 0,
            origin: [0, 0],
            tiles: None,
            size,
            reach: [0, 0, size[0], size[1]],
        };
        match source {
            Source::Tiles(surface) => {
                if let Some(backing) = surface.backing() {
                    let pixmap = backing.pixmap;
                    lookup.pixels = pixmap.data_as_u8_slice().as_chunks::<4>().0;
                    lookup.stride = usize::from(pixmap.width());
                    lookup.rows = usize::from(pixmap.height());
                    lookup.origin = backing.origin.map(i64::from);
                    let edge = if backing.edge.is_power_of_two() {
                        Edge::Shift(backing.edge.trailing_zeros())
                    } else {
                        Edge::Length(i64::from(backing.edge))
                    };
                    lookup.tiles = Some((backing.reached, backing.columns as usize, edge));
                }
            }
            Source::Whole(pixmap) => {
                lookup.pixels = pixmap.data_as_u8_slice().as_chunks::<4>().0;
                lookup.stride = usize::from(pixmap.width());
                lookup.rows = usize::from(pixmap.height());
            }
            Source::Empty => {}
        }
        if !everywhere {
            if lookup.pixels.is_empty() {
                return None;
            }
            let [x, y] = lookup.origin;
            lookup.reach = [x, y, x + lookup.stride as i64, y + lookup.rows as i64];
        }
        Some(lookup)
    }

    fn inside(&self, x: i64, y: i64) -> bool {
        x >= 0 && y >= 0 && x < self.size[0] && y < self.size[1]
    }

    #[inline]
    fn get(&self, x: i64, y: i64) -> Pixel {
        let (local_x, local_y) = (x - self.origin[0], y - self.origin[1]);
        if local_x < 0
            || local_y < 0
            || local_x >= self.stride as i64
            || local_y >= self.rows as i64
            || !self.inside(x, y)
        {
            return [0; 4];
        }
        if let Some((reached, columns, edge)) = self.tiles {
            let (column, row) = match edge {
                Edge::Shift(shift) => (x >> shift, y >> shift),
                Edge::Length(length) => (x / length, y / length),
            };
            if !reached[row as usize * columns + column as usize] {
                return [0; 4];
            }
        }
        self.pixels[local_y as usize * self.stride + local_x as usize]
    }

    /// The pixels at `x`, `y` and right, below and right below it.
    #[inline]
    fn quad(&self, x: i64, y: i64) -> [Pixel; 4] {
        let (local_x, local_y) = (x - self.origin[0], y - self.origin[1]);
        let within = local_x >= 0
            && local_y >= 0
            && local_x + 1 < self.stride as i64
            && local_y + 1 < self.rows as i64
            && x + 1 < self.size[0]
            && y + 1 < self.size[1];
        // All four in one tile, the usual case, need one look at it.
        let one_tile = match self.tiles {
            _ if !within => None,
            None => Some(true),
            Some((reached, columns, Edge::Shift(shift))) => {
                let (column, row) = (x >> shift, y >> shift);
                ((x + 1) >> shift == column && (y + 1) >> shift == row)
                    .then(|| reached[row as usize * columns + column as usize])
            }
            Some(_) => None,
        };
        match (within, one_tile) {
            (true, Some(true)) => {
                let at = local_y as usize * self.stride + local_x as usize;
                let below = at + self.stride;
                [
                    self.pixels[at],
                    self.pixels[at + 1],
                    self.pixels[below],
                    self.pixels[below + 1],
                ]
            }
            (true, Some(false)) => [[0; 4]; 4],
            _ => [
                self.get(x, y),
                self.get(x + 1, y),
                self.get(x, y + 1),
                self.get(x + 1, y + 1),
            ],
        }
    }

    /// The pixels of the row at `centre_y` within `left..right` whose
    /// centres `inverse` takes within a pixel and a half of what can read as
    /// anything, as first and one past the last; every other one reads
    /// transparency.
    fn span(
        &self,
        inverse: &Affine,
        centre_y: f64,
        left: usize,
        right: usize,
    ) -> Option<[usize; 2]> {
        let [a, b, c, d, e, f] = inverse.0;
        let [from_x, from_y, to_x, to_y] = self.reach.map(|edge| edge as f64);
        let (mut low, mut high) = (left as f64, right as f64);
        // Each source coordinate is linear in x: slope · (x + 0.5) + at.
        for (slope, at, from, to) in [
            (a, b * centre_y + c, from_x - 1.5, to_x + 1.5),
            (d, e * centre_y + f, from_y - 1.5, to_y + 1.5),
        ] {
            if slope == 0.0 {
                if !(from..=to).contains(&at) {
                    return None;
                }
                continue;
            }
            let ends = [(from - at) / slope - 0.5, (to - at) / slope - 0.5];
            low = low.max(ends[0].min(ends[1]).floor() - 1.0);
            high = high.min(ends[0].max(ends[1]).ceil() + 2.0);
        }
        (low < high).then_some([low as usize, high as usize])
    }
}

/// `source` moved by `moved` on a whole surface of `size`.
pub fn whole(source: Source<'_>, moved: &Moved, size: [u32; 2]) -> Pixmap {
    let mut pixmap = Pixmap::new(size[0] as u16, size[1] as u16);
    let width = size[0] as usize;
    let pixels = pixmap.data_as_u8_slice_mut().as_chunks_mut::<4>().0;
    for (y, line) in pixels.chunks_mut(width).enumerate() {
        line.copy_from_slice(&row(source, moved, y, 0, width, None));
    }
    pixmap
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tile::TiledSurface;

    /// A surface with a pixel pattern in one corner tile and nothing in the
    /// others.
    fn surface() -> TiledSurface {
        let mut surface = TiledSurface::new([40, 24], 16);
        surface.ensure([2, 3, 14, 12]);
        for (y, line) in surface.rows_mut([2, 3, 14, 12]) {
            for (x, pixel) in line.iter_mut().enumerate() {
                let alpha = ((x * 37 + y as usize * 11) % 256) as u8;
                *pixel = [alpha / 2, alpha / 3, alpha, alpha];
            }
        }
        surface
    }

    fn moves() -> Vec<Moved> {
        let turn = Affine::rotation_about(0.4, [8.0, 8.0]);
        [
            (Affine::translation(5.0, -2.0), false),
            (Affine::translation(2.25, 1.5), false),
            (turn, false),
            (turn, true),
            (Affine::scaling_about([-1.5, 1.5], [10.0, 6.0]), false),
        ]
        .into_iter()
        .map(|(forward, nearest)| Moved::new(forward, nearest).unwrap())
        .collect()
    }

    #[test]
    fn a_piece_of_a_moved_layer_has_the_bytes_of_the_whole() {
        let surface = surface();
        for moved in moves() {
            let whole = whole(Source::Tiles(&surface), &moved, [40, 24]);
            let pixels = whole.data_as_u8_slice().as_chunks::<4>().0;
            for (y, left, right) in [(0, 0, 40), (7, 3, 9), (11, 17, 40), (23, 0, 1)] {
                assert_eq!(
                    row(Source::Tiles(&surface), &moved, y, left, right, None),
                    pixels[y * 40 + left..y * 40 + right],
                    "{moved:?} row {y}"
                );
            }
        }
    }

    /// Each pixel read one at a time, with none of the shortcuts.
    fn plain(source: Source<'_>, moved: &Moved, y: usize, width: usize) -> Vec<Pixel> {
        let size = source.size().unwrap().map(i64::from);
        let read = |x: i64, y: i64| {
            if x < 0 || y < 0 || x >= size[0] || y >= size[1] {
                [0; 4]
            } else {
                source.pixel(x as u32, y as u32)
            }
        };
        let [a, b, c, d, e, f] = moved.fixed;
        (0..width as i64)
            .map(|x| {
                let (sx, sy) = (a * x + b * y as i64 + c, d * x + e * y as i64 + f);
                if moved.nearest {
                    return read((sx + HALF) >> 32, (sy + HALF) >> 32);
                }
                let [qx, qy] = [sx, sy].map(|value| (value + (1 << 23)) >> 24);
                let (x0, y0, wx, wy) = (qx >> 8, qy >> 8, (qx & 255) as u32, (qy & 255) as u32);
                let taps = [
                    read(x0, y0),
                    read(x0 + 1, y0),
                    read(x0, y0 + 1),
                    read(x0 + 1, y0 + 1),
                ];
                let weights = [
                    (256 - wx) * (256 - wy),
                    wx * (256 - wy),
                    (256 - wx) * wy,
                    wx * wy,
                ];
                std::array::from_fn(|channel| {
                    let sum: u32 = taps
                        .iter()
                        .zip(weights)
                        .map(|(tap, weight)| u32::from(tap[channel]) * weight)
                        .sum();
                    ((sum + 32_768) >> 16) as u8
                })
            })
            .collect()
    }

    #[test]
    fn the_shortcuts_read_what_each_pixel_on_its_own_reads() {
        // Tiles of 12, which is no power of two, with some never reached.
        let mut odd = TiledSurface::new([40, 24], 12);
        for rect in [[1, 1, 11, 11], [25, 13, 35, 23]] {
            odd.ensure(rect);
            for (y, line) in odd.rows_mut(rect) {
                for (x, pixel) in line.iter_mut().enumerate() {
                    let alpha = ((x * 53 + y as usize * 7) % 256) as u8;
                    *pixel = [alpha / 3, alpha / 2, alpha / 4, alpha];
                }
            }
        }
        for surface in [surface(), odd] {
            let flat = surface.to_pixmap();
            for moved in moves() {
                for source in [Source::Tiles(&surface), Source::Whole(&flat)] {
                    for y in 0..24 {
                        assert_eq!(
                            row(source, &moved, y, 0, 40, None),
                            plain(source, &moved, y, 40),
                            "{moved:?} row {y}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn whole_pixel_moves_copy_and_stay_premultiplied() {
        let surface = surface();
        let flat = surface.to_pixmap();
        let source = flat.data_as_u8_slice().as_chunks::<4>().0;
        let moved = Moved::new(Affine::translation(5.0, -2.0), false).unwrap();
        let shifted = whole(Source::Tiles(&surface), &moved, [40, 24]);
        let out = shifted.data_as_u8_slice().as_chunks::<4>().0;
        assert_eq!(out[5 * 40 + 12], source[7 * 40 + 7]);
        // A whole-pixel move read through the weighted path is the same.
        let mut weighted = moved;
        weighted.forward.0[2] += 1e-12;
        weighted.inverse = weighted.forward.inverse().unwrap();
        let weighted = whole(Source::Tiles(&surface), &weighted, [40, 24]);
        assert_eq!(weighted.data_as_u8_slice(), shifted.data_as_u8_slice());
        for moved in moves() {
            let image = whole(Source::Tiles(&surface), &moved, [40, 24]);
            for pixel in image.data_as_u8_slice().as_chunks::<4>().0 {
                assert!(pixel[..3].iter().all(|&value| value <= pixel[3]));
            }
        }
    }

    #[test]
    fn the_reach_of_a_moved_rectangle_covers_every_pixel_it_changes() {
        let surface = surface();
        let mut changed = surface.clone();
        for (_, line) in changed.rows_mut([4, 4, 6, 6]) {
            line.fill([255; 4]);
        }
        for moved in moves() {
            let before = whole(Source::Tiles(&surface), &moved, [40, 24]);
            let after = whole(Source::Tiles(&changed), &moved, [40, 24]);
            let reach = moved.reach([4, 4, 6, 6], [40, 24]).unwrap();
            let (before, after) = (
                before.data_as_u8_slice().as_chunks::<4>().0,
                after.data_as_u8_slice().as_chunks::<4>().0,
            );
            for y in 0..24 {
                for x in 0..40 {
                    let inside =
                        (reach[0]..reach[2]).contains(&x) && (reach[1]..reach[3]).contains(&y);
                    let index = (y * 40 + x) as usize;
                    assert!(inside || before[index] == after[index], "{moved:?} {x},{y}");
                }
            }
        }
    }
}

#[cfg(test)]
mod bench {
    use super::*;

    #[test]
    #[ignore = "timing"]
    fn sampling_cost() {
        let mut pixmap = Pixmap::new(2048, 2048);
        for (index, pixel) in pixmap.data_as_u8_slice_mut().chunks_mut(4).enumerate() {
            let alpha = (index % 251) as u8;
            pixel.copy_from_slice(&[alpha / 2, alpha / 3, alpha, alpha]);
        }
        let moved = Moved::new(Affine::rotation_about(0.2, [1024.0, 1024.0]), false).unwrap();
        let source = Source::Whole(&pixmap);
        for _ in 0..3 {
            let started = std::time::Instant::now();
            let mut sum = 0u64;
            for y in 0..2048 {
                let row = row(source, &moved, y, 0, 2048, None);
                sum += u64::from(row[y][3]);
            }
            println!("{:?} {sum}", started.elapsed());
        }
    }
}

// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! The selection: the canvas pixels later edits may touch. It belongs to
//! the session, not the document; an edit that uses it keeps a copy of its
//! mask (ADR section 2). Shapes are filled as 2.2.13 fills them: without
//! antialiasing, a pixel whose centre is inside, even-odd for a freehand
//! loop.

use std::sync::Arc;

use crate::ops::Affine;
use crate::store::Mask;

/// A non-empty set of pixels on a canvas, its mask cut to the pixels set.
#[derive(Clone, Debug, PartialEq)]
pub struct Selection {
    canvas: [u32; 2],
    mask: Mask,
}

/// A shape dragged on the canvas, in document pixels.
#[derive(Clone, Debug, PartialEq)]
pub enum Shape {
    /// Between opposite corners.
    Rectangle([f64; 2], [f64; 2]),
    /// Fitting the rectangle between opposite corners.
    Ellipse([f64; 2], [f64; 2]),
    /// A loop through the points, closed back to the first.
    Freehand(Vec<[f64; 2]>),
}

/// How a new shape changes the selection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Combine {
    Replace,
    Add,
    Subtract,
}

/// Where a flood fill stops, as 2.2.13 `FloodFillMask` decides it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Compare {
    /// At pixels at least half opaque.
    AlphaBoundary,
    /// At pixels whose straight colour differs from the clicked one by more
    /// than this in any channel, alpha included; 0 stops as `AlphaBoundary`.
    Color(u8),
}

fn straight(pixel: [u8; 4]) -> [u8; 4] {
    crate::ops::Rgba8::from_premultiplied(pixel).0
}

/// Pixels as one byte each over a whole canvas, for combining.
struct Canvas {
    size: [usize; 2],
    pixels: Vec<bool>,
}

impl Canvas {
    fn empty(size: [u32; 2]) -> Self {
        let size = size.map(|edge| edge as usize);
        Self {
            size,
            pixels: vec![false; size[0] * size[1]],
        }
    }

    fn of(selection: &Selection) -> Self {
        let mut canvas = Self::empty(selection.canvas);
        let [left, top, width, height] = selection.mask.bounds;
        let row_bytes = Mask::row_bytes(width);
        for row in 0..height as usize {
            let bits = &selection.mask.bits[row * row_bytes..(row + 1) * row_bytes];
            let start = (top as usize + row) * canvas.size[0] + left as usize;
            for column in 0..width as usize {
                canvas.pixels[start + column] = bits[column / 8] & (0x80 >> (column % 8)) != 0;
            }
        }
        canvas
    }

    /// Sets the pixels from `from` to `to` (exclusive) on row `y`.
    fn fill(&mut self, y: usize, from: usize, to: usize) {
        let start = y * self.size[0];
        self.pixels[start + from..start + to].fill(true);
    }

    /// The selection of the pixels set; `None` when there are none.
    fn selection(&self) -> Option<Selection> {
        let [width, height] = self.size;
        let rows = (0..height).filter(|&y| self.pixels[y * width..(y + 1) * width].contains(&true));
        let (top, bottom) = rows.fold((usize::MAX, 0), |(top, _), y| (top.min(y), y + 1));
        if top >= bottom {
            return None;
        }
        let (mut left, mut right) = (usize::MAX, 0);
        for y in top..bottom {
            let row = &self.pixels[y * width..(y + 1) * width];
            if let Some(first) = row.iter().position(|&set| set) {
                left = left.min(first);
                right = right.max(row.iter().rposition(|&set| set).unwrap_or(first) + 1);
            }
        }
        let mask_width = right - left;
        let row_bytes = Mask::row_bytes(mask_width as i32);
        let mut bits = vec![0u8; row_bytes * (bottom - top)];
        for y in top..bottom {
            let row = &self.pixels[y * width + left..y * width + right];
            let out = &mut bits[(y - top) * row_bytes..(y - top + 1) * row_bytes];
            for (column, _) in row.iter().enumerate().filter(|(_, set)| **set) {
                out[column / 8] |= 0x80 >> (column % 8);
            }
        }
        Some(Selection {
            canvas: [width as u32, height as u32],
            mask: Mask {
                bounds: [
                    left as i32,
                    top as i32,
                    mask_width as i32,
                    (bottom - top) as i32,
                ],
                bits: Arc::from(bits),
            },
        })
    }
}

/// The pixels whose centres lie from `from` to `to` (exclusive) along one
/// axis of `edge` pixels.
fn centres(from: f64, to: f64, edge: usize) -> (usize, usize) {
    let first = (from - 0.5).ceil().clamp(0.0, edge as f64) as usize;
    let end = (to - 0.5).ceil().clamp(0.0, edge as f64) as usize;
    (first, end.max(first))
}

impl Selection {
    /// Every pixel of a canvas of `canvas`; `None` for an empty canvas.
    pub fn all(canvas: [u32; 2]) -> Option<Self> {
        let [width, height] = canvas.map(|edge| edge as i32);
        if width <= 0 || height <= 0 {
            return None;
        }
        let row_bytes = Mask::row_bytes(width);
        let mut row = vec![0xffu8; row_bytes];
        if width % 8 != 0 {
            row[row_bytes - 1] = 0xff << (8 - width % 8);
        }
        Some(Self {
            canvas,
            mask: Mask {
                bounds: [0, 0, width, height],
                bits: Arc::from(row.repeat(height as usize)),
            },
        })
    }

    /// The pixels of `shape` on a canvas of `canvas`; `None` for a shape too
    /// small to select (a rectangle or ellipse under a pixel across, a loop
    /// of fewer than three points) or one that covers no pixel centre.
    pub fn of_shape(shape: &Shape, canvas: [u32; 2]) -> Option<Self> {
        let mut pixels = Canvas::empty(canvas);
        let [width, height] = pixels.size;
        match shape {
            Shape::Rectangle(a, b) | Shape::Ellipse(a, b) => {
                let [left, right] = [a[0].min(b[0]), a[0].max(b[0])];
                let [top, bottom] = [a[1].min(b[1]), a[1].max(b[1])];
                if !(right - left >= 1.0 && bottom - top >= 1.0) {
                    return None;
                }
                let (first_row, end_row) = centres(top, bottom, height);
                let ellipse = matches!(shape, Shape::Ellipse(..));
                let [cx, cy] = [(left + right) / 2.0, (top + bottom) / 2.0];
                let [rx, ry] = [(right - left) / 2.0, (bottom - top) / 2.0];
                for y in first_row..end_row {
                    let (from, to) = if ellipse {
                        let dy = (y as f64 + 0.5 - cy) / ry;
                        let reach = 1.0 - dy * dy;
                        if reach <= 0.0 {
                            continue;
                        }
                        let half = rx * reach.sqrt();
                        (cx - half, cx + half)
                    } else {
                        (left, right)
                    };
                    let (first, end) = centres(from, to, width);
                    pixels.fill(y, first, end);
                }
            }
            Shape::Freehand(points) => {
                if points.len() < 3 {
                    return None;
                }
                // Each edge adds a crossing to the rows whose centres it spans.
                let mut rows: Vec<Vec<f64>> = vec![Vec::new(); height];
                for (index, a) in points.iter().enumerate() {
                    let b = points[(index + 1) % points.len()];
                    let (low, high) = (a[1].min(b[1]), a[1].max(b[1]));
                    let (first, end) = centres(low, high, height);
                    for (y, crossings) in rows.iter_mut().enumerate().take(end).skip(first) {
                        let centre = y as f64 + 0.5;
                        if (a[1] <= centre) != (b[1] <= centre) {
                            crossings.push(a[0] + (centre - a[1]) * (b[0] - a[0]) / (b[1] - a[1]));
                        }
                    }
                }
                for (y, crossings) in rows.iter_mut().enumerate() {
                    crossings.sort_by(f64::total_cmp);
                    for [from, to] in crossings.as_chunks::<2>().0 {
                        let (first, end) = centres(*from, *to, width);
                        pixels.fill(y, first, end);
                    }
                }
            }
        }
        pixels.selection()
    }

    /// The canvas pixels whose centres `contours`, closed polygons, enclose
    /// by the non-zero rule; `None` when none.
    pub fn of_contours(contours: &[Vec<[f64; 2]>], canvas: [u32; 2]) -> Option<Self> {
        let [width, height] = canvas.map(|edge| edge as usize);
        let points = || contours.iter().flatten();
        let low = points().map(|p| p[1]).fold(f64::MAX, f64::min);
        let high = points().map(|p| p[1]).fold(f64::MIN, f64::max);
        let left = points().map(|p| p[0]).fold(f64::MAX, f64::min);
        let right = points().map(|p| p[0]).fold(f64::MIN, f64::max);
        let (first_row, end_row) = centres(low, high, height);
        let (first_column, end_column) = centres(left, right, width);
        if first_row >= end_row || first_column >= end_column {
            return None;
        }
        // Each edge crosses the rows whose centres it spans, upward or down.
        let mut rows: Vec<Vec<(f64, i32)>> = vec![Vec::new(); end_row - first_row];
        for contour in contours.iter().filter(|contour| contour.len() >= 3) {
            for (index, a) in contour.iter().enumerate() {
                let b = contour[(index + 1) % contour.len()];
                let (from, to) = centres(a[1].min(b[1]), a[1].max(b[1]), height);
                for y in from.max(first_row)..to.min(end_row) {
                    let centre = y as f64 + 0.5;
                    if (a[1] <= centre) != (b[1] <= centre) {
                        let x = a[0] + (centre - a[1]) * (b[0] - a[0]) / (b[1] - a[1]);
                        rows[y - first_row].push((x, if b[1] > a[1] { 1 } else { -1 }));
                    }
                }
            }
        }
        let columns = (end_column - first_column) as i32;
        let row_bytes = Mask::row_bytes(columns);
        let mut bits = vec![0u8; row_bytes * rows.len()];
        for (crossings, out) in rows.iter_mut().zip(bits.chunks_exact_mut(row_bytes)) {
            crossings.sort_by(|a, b| a.0.total_cmp(&b.0));
            let mut winding = 0;
            for pair in crossings.windows(2) {
                winding += pair[0].1;
                if winding == 0 {
                    continue;
                }
                let (first, end) = centres(pair[0].0, pair[1].0, width);
                for x in first.max(first_column)..end.min(end_column) {
                    let x = x - first_column;
                    out[x / 8] |= 0x80 >> (x % 8);
                }
            }
        }
        tight(
            canvas,
            [
                first_column as i32,
                first_row as i32,
                columns,
                (end_row - first_row) as i32,
            ],
            bits,
        )
    }

    /// The pixels reached from `seed` through edge neighbours that `compare`
    /// lets through, in `pixels`: premultiplied rows of a canvas of
    /// `canvas`. `None` when the seed is outside or itself stops the fill.
    pub fn flood(
        pixels: &[[u8; 4]],
        canvas: [u32; 2],
        seed: [u32; 2],
        compare: Compare,
    ) -> Option<Self> {
        let [width, height] = canvas.map(|edge| edge as usize);
        let [seed_x, seed_y] = seed.map(|at| at as usize);
        if pixels.len() != width * height || seed_x >= width || seed_y >= height {
            return None;
        }
        let target = straight(pixels[seed_y * width + seed_x]);
        let open = |x: usize, y: usize| {
            let pixel = pixels[y * width + x];
            match compare {
                Compare::AlphaBoundary | Compare::Color(0) => pixel[3] < 128,
                Compare::Color(tolerance) => straight(pixel)
                    .iter()
                    .zip(target)
                    .all(|(&channel, target)| channel.abs_diff(target) <= tolerance),
            }
        };
        if !open(seed_x, seed_y) {
            return None;
        }
        let mut reached = Canvas::empty(canvas);
        let mut pending = vec![[seed_x, seed_y]];
        while let Some([x, y]) = pending.pop() {
            let row = y * width;
            if reached.pixels[row + x] || !open(x, y) {
                continue;
            }
            let mut left = x;
            while left > 0 && !reached.pixels[row + left - 1] && open(left - 1, y) {
                left -= 1;
            }
            let mut right = x;
            while right + 1 < width && !reached.pixels[row + right + 1] && open(right + 1, y) {
                right += 1;
            }
            reached.fill(y, left, right + 1);
            for next in [y.wrapping_sub(1), y + 1] {
                if next >= height {
                    continue;
                }
                let row = next * width;
                let mut x = left;
                while x <= right {
                    if !reached.pixels[row + x] && open(x, next) {
                        pending.push([x, next]);
                        while x < right && !reached.pixels[row + x + 1] && open(x + 1, next) {
                            x += 1;
                        }
                    }
                    x += 1;
                }
            }
        }
        reached.selection()
    }

    /// Where the selected pixels land when `transform` moves them: every
    /// canvas pixel a moved pixel square overlaps. The renderer draws moved
    /// content cut to that shape with soft edges, so later strokes cut to
    /// this selection reach those edge pixels too. `None` when nothing lands
    /// on the canvas or it flattens.
    pub fn transformed(&self, transform: Affine) -> Option<Self> {
        transform.inverse()?;
        let [left, top, width, height] = self.mask.bounds;
        let row_bytes = Mask::row_bytes(width);
        let runs = |row: i32| {
            let bits = &self.mask.bits[row as usize * row_bytes..(row as usize + 1) * row_bytes];
            let set = |x: i32| bits[x as usize / 8] & (0x80 >> (x % 8)) != 0;
            let mut runs = Vec::new();
            let mut x = 0;
            while x < width {
                if bits[x as usize / 8] == 0 && x % 8 == 0 {
                    x += 8;
                    continue;
                }
                if !set(x) {
                    x += 1;
                    continue;
                }
                let start = x;
                while x < width && set(x) {
                    x += 1;
                }
                runs.push([start, x]);
            }
            runs
        };
        let mut pixels = Canvas::empty(self.canvas);
        let [canvas_width, canvas_height] = self.canvas.map(f64::from);
        let mut row = 0;
        while row < height {
            // Rows with the same runs make rectangles, moved as one.
            let same = runs(row);
            let mut end = row + 1;
            while end < height && runs(end) == same {
                end += 1;
            }
            let [y0, y1] = [top + row, top + end].map(f64::from);
            for [from, to] in same {
                let [x0, x1] = [left + from, left + to].map(f64::from);
                // Quarter turns land on whole pixels give or take rounding.
                let snap = |value: f64| {
                    let whole = value.round();
                    if (value - whole).abs() < 1e-9 {
                        whole
                    } else {
                        value
                    }
                };
                let quad = [[x0, y0], [x1, y0], [x1, y1], [x0, y1]]
                    .map(|corner| transform.apply(corner).map(snap));
                let low = quad.iter().map(|corner| corner[1]).fold(f64::MAX, f64::min);
                let high = quad.iter().map(|corner| corner[1]).fold(f64::MIN, f64::max);
                let first = low.floor().clamp(0.0, canvas_height) as usize;
                let last = high.ceil().clamp(0.0, canvas_height) as usize;
                for y in first..last {
                    if let Some([from, to]) = across(&quad, y as f64) {
                        let clamp = |value: f64| value.clamp(0.0, canvas_width) as usize;
                        pixels.fill(y, clamp(from.floor()), clamp(to.ceil()));
                    }
                }
            }
            row = end;
        }
        pixels.selection()
    }

    /// `shape`'s pixels combined with `current` by `how`; `None` when no
    /// pixel is left.
    pub fn combine(current: Option<&Self>, shape: Option<Self>, how: Combine) -> Option<Self> {
        match (how, current, shape) {
            (Combine::Replace, _, shape) => shape,
            (Combine::Add, None, shape) => shape,
            (_, current, None) => current.cloned(),
            (Combine::Subtract, None, _) => None,
            (_, Some(current), Some(shape)) => {
                let mut pixels = Canvas::of(current);
                let other = Canvas::of(&shape);
                for (pixel, &set) in pixels.pixels.iter_mut().zip(&other.pixels) {
                    if how == Combine::Add {
                        *pixel |= set;
                    } else {
                        *pixel &= !set;
                    }
                }
                pixels.selection()
            }
        }
    }

    /// The canvas pixels not selected; `None` when every one was.
    pub fn invert(&self) -> Option<Self> {
        let mut pixels = Canvas::of(self);
        for pixel in &mut pixels.pixels {
            *pixel = !*pixel;
        }
        pixels.selection()
    }

    /// The selection on the canvas left by moving the content by `offset`
    /// onto a canvas of `size`, as `Op::Crop` does.
    pub fn cropped(&self, offset: [i32; 2], size: [u32; 2]) -> Option<Self> {
        let [left, top, width, height] = self.mask.bounds;
        let [left, top] = [left + offset[0], top + offset[1]];
        let [canvas_width, canvas_height] = size.map(|edge| edge as i32);
        let [from_x, from_y] = [left.max(0), top.max(0)];
        let [to_x, to_y] = [
            (left + width).min(canvas_width),
            (top + height).min(canvas_height),
        ];
        if from_x >= to_x || from_y >= to_y {
            return None;
        }
        if [from_x, from_y, to_x, to_y] == [left, top, left + width, top + height] {
            return Some(Self {
                canvas: size,
                mask: Mask {
                    bounds: [left, top, width, height],
                    bits: self.mask.bits.clone(),
                },
            });
        }
        let source_bytes = Mask::row_bytes(width);
        let kept = to_x - from_x;
        let row_bytes = Mask::row_bytes(kept);
        let mut bits = vec![0u8; row_bytes * (to_y - from_y) as usize];
        for (y, out) in (from_y..to_y).zip(bits.chunks_exact_mut(row_bytes)) {
            let row = (y - top) as usize * source_bytes;
            copy_bits(
                &self.mask.bits[row..row + source_bytes],
                (from_x - left) as usize,
                kept as usize,
                out,
            );
        }
        tight(size, [from_x, from_y, kept, to_y - from_y], bits)
    }

    /// The selection resampled to a canvas of `size` by the nearest pixel
    /// centre, as `Op::Resample` resamples with nearest sampling.
    pub fn resampled(&self, size: [u32; 2]) -> Option<Self> {
        let [left, top, width, height] = self.mask.bounds;
        let source = |index: usize, from_edge: u32, to_edge: u32| {
            let [from_edge, to_edge] = [from_edge as usize, to_edge as usize];
            ((2 * index + 1) * from_edge / (2 * to_edge)).min(from_edge - 1) as i32
        };
        // The target pixels whose source lies in the mask, along one axis.
        let reach = |start: i32, length: i32, axis: usize| {
            let inside = |index: &usize| {
                (start..start + length).contains(&source(*index, self.canvas[axis], size[axis]))
            };
            let mut indices = 0..size[axis] as usize;
            let first = indices.find(inside)?;
            let last = indices.rfind(inside).unwrap_or(first);
            Some((first, last + 1))
        };
        let (from_x, to_x) = reach(left, width, 0)?;
        let (from_y, to_y) = reach(top, height, 1)?;
        let columns: Vec<usize> = (from_x..to_x)
            .map(|x| (source(x, self.canvas[0], size[0]) - left) as usize)
            .collect();
        let source_bytes = Mask::row_bytes(width);
        let row_bytes = Mask::row_bytes(columns.len() as i32);
        let mut bits = vec![0u8; row_bytes * (to_y - from_y)];
        let mut previous = None;
        for y in from_y..to_y {
            let row = (source(y, self.canvas[1], size[1]) - top) as usize;
            let at = (y - from_y) * row_bytes;
            if previous == Some(row) {
                bits.copy_within(at - row_bytes..at, at);
                continue;
            }
            previous = Some(row);
            let input = &self.mask.bits[row * source_bytes..(row + 1) * source_bytes];
            let out = &mut bits[at..at + row_bytes];
            for (x, &column) in columns.iter().enumerate() {
                if input[column / 8] & (0x80 >> (column % 8)) != 0 {
                    out[x / 8] |= 0x80 >> (x % 8);
                }
            }
        }
        tight(
            size,
            [
                from_x as i32,
                from_y as i32,
                columns.len() as i32,
                (to_y - from_y) as i32,
            ],
            bits,
        )
    }

    /// This selection on a canvas `canvas` that `to_here` moved onto this
    /// one: every pixel there that lands on a selected one. `None` when none
    /// does.
    pub(crate) fn before(&self, canvas: [u32; 2], to_here: Affine) -> Option<Self> {
        let back = Self {
            canvas,
            mask: self.mask.clone(),
        };
        back.transformed(to_here.inverse()?)
    }

    pub fn canvas(&self) -> [u32; 2] {
        self.canvas
    }

    pub fn mask(&self) -> &Mask {
        &self.mask
    }

    /// Whether every canvas pixel is selected, so it limits nothing.
    pub fn covers_canvas(&self) -> bool {
        let [width, height] = self.canvas.map(|edge| edge as i32);
        self.mask.bounds == [0, 0, width, height]
            && Self::all(self.canvas).is_some_and(|all| all.mask == self.mask)
    }

    /// The edges between selected and other pixels; see `Mask::outline`.
    pub fn outline(&self) -> Vec<Vec<[i32; 2]>> {
        self.mask.outline()
    }
}

/// Copies `width` bits from bit `from` of `source` to the start of `out`,
/// leaving the bits after them clear.
pub(crate) fn copy_bits(source: &[u8], from: usize, width: usize, out: &mut [u8]) {
    let shift = from % 8;
    for (index, byte) in out.iter_mut().enumerate().take(width.div_ceil(8)) {
        let at = from / 8 + index;
        let next = source.get(at + 1).copied().unwrap_or(0);
        *byte = if shift == 0 {
            source[at]
        } else {
            source[at] << shift | next >> (8 - shift)
        };
    }
    if !width.is_multiple_of(8) {
        out[width / 8] &= 0xff << (8 - width % 8);
    }
}

/// The selection of the bits set in a mask at `bounds`, with the bounds
/// narrowed to them; `None` when there are none.
fn tight(canvas: [u32; 2], bounds: [i32; 4], bits: Vec<u8>) -> Option<Selection> {
    let [left, top, width, _] = bounds;
    let row_bytes = Mask::row_bytes(width);
    let rows: Vec<&[u8]> = bits.chunks_exact(row_bytes).collect();
    let set = |row: &&[u8]| row.iter().any(|&byte| byte != 0);
    let first = rows.iter().position(set)?;
    let last = rows.iter().rposition(set).unwrap_or(first) + 1;
    let (mut from, mut to) = (usize::MAX, 0);
    for row in &rows[first..last] {
        if let Some(byte) = row.iter().position(|&byte| byte != 0) {
            from = from.min(byte * 8 + row[byte].leading_zeros() as usize);
        }
        if let Some(byte) = row.iter().rposition(|&byte| byte != 0) {
            to = to.max(byte * 8 + 8 - row[byte].trailing_zeros() as usize);
        }
    }
    let narrowed = [
        left + from as i32,
        top + first as i32,
        (to - from) as i32,
        (last - first) as i32,
    ];
    let bits = if narrowed == bounds {
        bits
    } else {
        let out_bytes = Mask::row_bytes(narrowed[2]);
        let mut out = vec![0u8; out_bytes * (last - first)];
        for (row, target) in rows[first..last]
            .iter()
            .zip(out.chunks_exact_mut(out_bytes))
        {
            copy_bits(row, from, to - from, target);
        }
        out
    };
    Some(Selection {
        canvas,
        mask: Mask {
            bounds: narrowed,
            bits: Arc::from(bits),
        },
    })
}

/// How far the convex `quad` reaches across the row of pixels from `y` to
/// `y + 1`: the least and greatest x of the part inside it; `None` when that
/// part has no height.
fn across(quad: &[[f64; 2]; 4], y: f64) -> Option<[f64; 2]> {
    let (top, bottom) = (y, y + 1.0);
    let mut reach: Option<[f64; 2]> = None;
    let mut take = |x: f64| {
        reach = Some(reach.map_or([x, x], |[low, high]| [low.min(x), high.max(x)]));
    };
    for (index, &[ax, ay]) in quad.iter().enumerate() {
        if (top..=bottom).contains(&ay) {
            take(ax);
        }
        let [bx, by] = quad[(index + 1) % 4];
        for line in [top, bottom] {
            if (ay - line) * (by - line) < 0.0 {
                take(ax + (line - ay) * (bx - ax) / (by - ay));
            }
        }
    }
    let low = quad.iter().map(|corner| corner[1]).fold(f64::MAX, f64::min);
    let high = quad.iter().map(|corner| corner[1]).fold(f64::MIN, f64::max);
    reach.filter(|[from, to]| to > from && low < bottom && high > top)
}

#[cfg(test)]
mod tests {
    use super::*;

    const CANVAS: [u32; 2] = [40, 30];

    fn pixels(selection: Option<&Selection>) -> Vec<[i32; 2]> {
        let Some(selection) = selection else {
            return Vec::new();
        };
        let mut set = Vec::new();
        for y in 0..CANVAS[1] as i32 {
            for x in 0..CANVAS[0] as i32 {
                if selection.mask.contains(x, y) {
                    set.push([x, y]);
                }
            }
        }
        set
    }

    fn rectangle(a: [f64; 2], b: [f64; 2]) -> Option<Selection> {
        Selection::of_shape(&Shape::Rectangle(a, b), CANVAS)
    }

    #[test]
    fn a_rectangle_takes_the_pixels_whose_centres_are_inside() {
        let selection = rectangle([2.4, 3.6], [5.6, 4.8]).unwrap();
        assert_eq!(pixels(Some(&selection)), [[2, 4], [3, 4], [4, 4], [5, 4]]);
        assert_eq!(selection.mask.bounds, [2, 4, 4, 1]);
        // Dragged either way, the same.
        assert_eq!(rectangle([5.6, 4.8], [2.4, 3.6]), Some(selection));
        // Less than a pixel across selects nothing, as a click.
        assert_eq!(rectangle([2.0, 2.0], [2.9, 9.0]), None);
        // Off the canvas is cut off.
        let edge = rectangle([-5.0, -5.0], [1.0, 1.0]).unwrap();
        assert_eq!(pixels(Some(&edge)), [[0, 0]]);
    }

    #[test]
    fn an_ellipse_fits_the_rectangle() {
        let shape = Shape::Ellipse([10.0, 10.0], [20.0, 16.0]);
        let ellipse = Selection::of_shape(&shape, CANVAS).unwrap();
        assert_eq!(ellipse.mask.bounds, [10, 10, 10, 6]);
        // The middle row is full, the corners are not.
        for x in 10..20 {
            assert!(ellipse.mask.contains(x, 12));
        }
        for [x, y] in [[10, 10], [19, 10], [10, 15], [19, 15]] {
            assert!(!ellipse.mask.contains(x, y));
        }
        let rect = rectangle([10.0, 10.0], [20.0, 16.0]).unwrap();
        assert!(pixels(Some(&ellipse)).len() < pixels(Some(&rect)).len());
    }

    #[test]
    fn a_freehand_loop_fills_even_odd_and_closes_itself() {
        let square = vec![[2.0, 2.0], [8.0, 2.0], [8.0, 8.0], [2.0, 8.0]];
        let filled = Selection::of_shape(&Shape::Freehand(square), CANVAS).unwrap();
        assert_eq!(filled, rectangle([2.0, 2.0], [8.0, 8.0]).unwrap());
        // A loop around twice in opposite turns: the overlap is left out.
        let bow = vec![[0.0, 0.0], [10.0, 10.0], [10.0, 0.0], [0.0, 10.0]];
        let bow = Selection::of_shape(&Shape::Freehand(bow), CANVAS).unwrap();
        assert!(bow.mask.contains(1, 5));
        assert!(bow.mask.contains(8, 5));
        assert!(!bow.mask.contains(5, 1));
        assert!(!bow.mask.contains(5, 8));
        // Too few points, or points on a line, select nothing.
        assert_eq!(
            Selection::of_shape(&Shape::Freehand(vec![[0.0, 0.0], [9.0, 9.0]]), CANVAS),
            None
        );
        let line = vec![[0.0, 0.0], [5.0, 5.0], [9.0, 9.0]];
        assert_eq!(Selection::of_shape(&Shape::Freehand(line), CANVAS), None);
    }

    #[test]
    fn shapes_add_and_subtract() {
        let a = rectangle([0.0, 0.0], [4.0, 1.0]);
        let b = rectangle([2.0, 0.0], [6.0, 1.0]);
        let added = Selection::combine(a.as_ref(), b.clone(), Combine::Add).unwrap();
        assert_eq!(added, rectangle([0.0, 0.0], [6.0, 1.0]).unwrap());
        let less = Selection::combine(a.as_ref(), b.clone(), Combine::Subtract).unwrap();
        assert_eq!(less, rectangle([0.0, 0.0], [2.0, 1.0]).unwrap());
        assert_eq!(
            Selection::combine(a.as_ref(), a.clone(), Combine::Subtract),
            None
        );
        assert_eq!(
            Selection::combine(a.as_ref(), b.clone(), Combine::Replace),
            b
        );
        assert_eq!(Selection::combine(None, b.clone(), Combine::Add), b);
        assert_eq!(Selection::combine(None, b.clone(), Combine::Subtract), None);
        // A shape that selects nothing leaves adding and subtracting as they
        // were and clears on replacing.
        assert_eq!(Selection::combine(a.as_ref(), None, Combine::Add), a);
        assert_eq!(Selection::combine(a.as_ref(), None, Combine::Subtract), a);
        assert_eq!(Selection::combine(a.as_ref(), None, Combine::Replace), None);
    }

    #[test]
    fn all_and_invert_follow_the_canvas() {
        let all = Selection::all(CANVAS).unwrap();
        assert_eq!(pixels(Some(&all)).len(), 40 * 30);
        assert!(all.covers_canvas());
        assert_eq!(all.invert(), None);
        let corner = rectangle([0.0, 0.0], [1.0, 1.0]).unwrap();
        assert!(!corner.covers_canvas());
        let rest = corner.invert().unwrap();
        assert_eq!(pixels(Some(&rest)).len(), 40 * 30 - 1);
        assert!(!rest.mask.contains(0, 0));
        assert_eq!(rest.invert(), Some(corner));
        // Widths that are not whole bytes.
        let odd = Selection::all([13, 2]).unwrap();
        assert!(odd.mask.contains(12, 1) && !odd.mask.contains(13, 1));
    }

    /// The set pixels of `selection` anywhere on its canvas, and whether its
    /// bounds are the least that hold them.
    fn set_pixels(selection: &Selection) -> (Vec<[i32; 2]>, bool) {
        let [width, height] = selection.canvas.map(|edge| edge as i32);
        let set: Vec<[i32; 2]> = (0..height)
            .flat_map(|y| (0..width).map(move |x| [x, y]))
            .filter(|&[x, y]| selection.mask.contains(x, y))
            .collect();
        let low = |axis: usize| set.iter().map(|p| p[axis]).min().unwrap();
        let high = |axis: usize| set.iter().map(|p| p[axis]).max().unwrap() + 1;
        let tight = selection.mask.bounds == [low(0), low(1), high(0) - low(0), high(1) - low(1)];
        (set, tight)
    }

    #[test]
    fn crops_and_resamples_match_moving_each_pixel() {
        let canvas = [37, 29];
        let star = Shape::Freehand(
            (0..23)
                .map(|i| {
                    let turn = f64::from(i) / 23.0 * std::f64::consts::TAU;
                    let reach = if i % 2 == 0 { 15.0 } else { 6.0 };
                    [18.3 + reach * turn.cos(), 14.1 + reach * turn.sin()]
                })
                .collect(),
        );
        let shapes = [
            star,
            Shape::Ellipse([3.0, 2.0], [30.0, 27.0]),
            Shape::Rectangle([0.0, 0.0], [37.0, 29.0]),
            Shape::Rectangle([9.0, 4.0], [10.0, 25.0]),
        ];
        for shape in &shapes {
            let selection = Selection::of_shape(shape, canvas).unwrap();
            let (before, _) = set_pixels(&selection);
            for (offset, size) in [
                ([0, 0], canvas),
                ([5, 3], [60, 40]),
                ([-7, -3], [20, 21]),
                ([-11, 6], [17, 9]),
                ([-9, 0], [3, 29]),
            ] {
                let expected: Vec<[i32; 2]> = before
                    .iter()
                    .map(|&[x, y]| [x + offset[0], y + offset[1]])
                    .filter(|&[x, y]| x >= 0 && y >= 0 && x < size[0] as i32 && y < size[1] as i32)
                    .collect();
                let mut expected = expected;
                expected.sort_by_key(|&[x, y]| (y, x));
                match selection.cropped(offset, size) {
                    None => assert!(expected.is_empty()),
                    Some(cropped) => {
                        assert_eq!(cropped.canvas, size);
                        assert_eq!(
                            set_pixels(&cropped),
                            (expected, true),
                            "{offset:?} {size:?}"
                        );
                    }
                }
            }
            for size in [[74, 58], [19, 14], [37, 7], [3, 61], [36, 30], [1, 1]] {
                let source = |index: i32, from: u32, to: u32| {
                    ((2 * index as u32 + 1) * from / (2 * to)).min(from - 1) as i32
                };
                let expected: Vec<[i32; 2]> = (0..size[1] as i32)
                    .flat_map(|y| (0..size[0] as i32).map(move |x| [x, y]))
                    .filter(|&[x, y]| {
                        selection
                            .mask
                            .contains(source(x, canvas[0], size[0]), source(y, canvas[1], size[1]))
                    })
                    .collect();
                match selection.resampled(size) {
                    None => assert!(expected.is_empty()),
                    Some(resampled) => {
                        assert_eq!(resampled.canvas, size);
                        assert_eq!(set_pixels(&resampled), (expected, true), "{size:?}");
                    }
                }
            }
        }
    }

    #[test]
    fn contours_fill_by_the_non_zero_rule() {
        let star: Vec<[f64; 2]> = (0..5)
            .map(|i| {
                let turn = f64::from(i * 2) / 5.0 * std::f64::consts::TAU;
                [20.0 + 15.0 * turn.sin(), 20.0 - 15.0 * turn.cos()]
            })
            .collect();
        let canvas = [40, 40];
        // A simple polygon fills alike by either rule.
        let triangle = vec![[3.0, 3.0], [30.0, 8.0], [9.0, 27.5]];
        assert_eq!(
            Selection::of_contours(std::slice::from_ref(&triangle), canvas),
            Selection::of_shape(&Shape::Freehand(triangle), canvas)
        );
        // A pentagram's middle winds twice: filled here, open by even-odd.
        let filled = Selection::of_contours(std::slice::from_ref(&star), canvas).unwrap();
        let even_odd = Selection::of_shape(&Shape::Freehand(star), canvas).unwrap();
        assert!(filled.mask.contains(20, 20) && !even_odd.mask.contains(20, 20));
        assert_eq!(filled.mask.bounds, even_odd.mask.bounds);
    }

    #[test]
    fn crops_and_resamples_move_the_selection_with_the_content() {
        let square = rectangle([4.0, 4.0], [8.0, 8.0]).unwrap();
        let cropped = square.cropped([-2, 3], [20, 20]).unwrap();
        assert_eq!(cropped.canvas, [20, 20]);
        assert_eq!(cropped.mask.bounds, [2, 7, 4, 4]);
        assert_eq!(square.cropped([-30, 0], [20, 20]), None);
        let doubled = square.resampled([80, 60]).unwrap();
        assert_eq!(doubled.mask.bounds, [8, 8, 8, 8]);
        let halved = square.resampled([20, 15]).unwrap();
        assert_eq!(halved.mask.bounds, [2, 2, 2, 2]);
    }

    #[test]
    fn the_outline_follows_pixel_edges_with_the_selection_on_the_right() {
        let square = rectangle([1.0, 2.0], [4.0, 4.0]).unwrap();
        let outline = square.outline();
        assert_eq!(outline.len(), 1);
        let mut corners = outline[0].clone();
        corners.sort();
        assert_eq!(corners, [[1, 2], [1, 4], [4, 2], [4, 4]]);
        // Clockwise on screen (y down), which keeps the inside on the right.
        let area: i32 = (0..4)
            .map(|index| {
                let ([ax, ay], [bx, by]) = (outline[0][index], outline[0][(index + 1) % 4]);
                ax * by - bx * ay
            })
            .sum();
        assert!(area > 0);
        // A hole is its own loop.
        let ring = Selection::combine(
            Some(&rectangle([0.0, 0.0], [9.0, 9.0]).unwrap()),
            rectangle([3.0, 3.0], [6.0, 6.0]),
            Combine::Subtract,
        )
        .unwrap();
        assert_eq!(ring.outline().len(), 2);
        // Pixels touching only at a corner make loops that close.
        let diagonal = Selection::combine(
            rectangle([0.0, 0.0], [1.0, 1.0]).as_ref(),
            rectangle([1.0, 1.0], [2.0, 2.0]),
            Combine::Add,
        )
        .unwrap();
        let edges: usize = diagonal.outline().iter().map(Vec::len).sum();
        assert_eq!(edges, 8);
    }

    /// Every pixel edge between a selected and another pixel, from the
    /// corner it starts at, the selection on the right.
    fn boundary(selection: &Selection) -> Vec<([i32; 2], [i32; 2])> {
        let set = |x: i32, y: i32| selection.mask.contains(x, y);
        let mut edges = Vec::new();
        for y in -1..=CANVAS[1] as i32 {
            for x in -1..=CANVAS[0] as i32 {
                match (set(x, y - 1), set(x, y)) {
                    (false, true) => edges.push(([x, y], [x + 1, y])),
                    (true, false) => edges.push(([x + 1, y], [x, y])),
                    _ => {}
                }
                match (set(x - 1, y), set(x, y)) {
                    (false, true) => edges.push(([x, y + 1], [x, y])),
                    (true, false) => edges.push(([x, y], [x, y + 1])),
                    _ => {}
                }
            }
        }
        edges.sort();
        edges
    }

    #[test]
    fn the_outline_takes_every_boundary_edge_once_in_closed_loops() {
        let mut state = 0x2545_f491_4f6c_dd1d_u64;
        for density in [2, 5, 8] {
            let mut pixels = Canvas::empty(CANVAS);
            for pixel in &mut pixels.pixels {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                *pixel = state % 10 < density;
            }
            let selection = pixels.selection().unwrap();
            let mut traced = Vec::new();
            for corners in selection.outline() {
                for (index, &from) in corners.iter().enumerate() {
                    let to = corners[(index + 1) % corners.len()];
                    // Runs are straight and turn at every corner.
                    assert!(from[0] == to[0] || from[1] == to[1]);
                    let next = corners[(index + 2) % corners.len()];
                    assert!(next[0] != from[0] && next[1] != from[1] || corners.len() == 2);
                    let step = [(to[0] - from[0]).signum(), (to[1] - from[1]).signum()];
                    let mut at = from;
                    while at != to {
                        let end = [at[0] + step[0], at[1] + step[1]];
                        traced.push((at, end));
                        at = end;
                    }
                }
            }
            traced.sort();
            assert_eq!(traced, boundary(&selection), "density {density}");
        }
    }

    /// A transparent canvas with a closed square ring of opaque black, two
    /// pixels thick, as 2.2.13's flood fill test draws it.
    fn ring() -> (Vec<[u8; 4]>, [u32; 2]) {
        let size = [24, 20];
        let mut pixels = vec![[0; 4]; 24 * 20];
        for y in 5..15 {
            for x in 6..18 {
                if !(8..16).contains(&x) || !(7..13).contains(&y) {
                    pixels[y * 24 + x] = [0, 0, 0, 255];
                }
            }
        }
        (pixels, size)
    }

    #[test]
    fn a_flood_fills_the_area_its_seed_is_in_up_to_the_lines() {
        let (pixels, size) = ring();
        let inside = Selection::flood(&pixels, size, [10, 10], Compare::AlphaBoundary).unwrap();
        assert_eq!(inside.mask.bounds, [8, 7, 8, 6]);
        assert_eq!(
            inside
                .mask
                .bits
                .iter()
                .map(|byte| byte.count_ones())
                .sum::<u32>(),
            48
        );
        // Colour with no tolerance stops as the alpha boundary does.
        assert_eq!(
            Selection::flood(&pixels, size, [10, 10], Compare::Color(0)),
            Some(inside)
        );
        let outside = Selection::flood(&pixels, size, [0, 0], Compare::Color(32)).unwrap();
        assert_eq!(outside.mask.bounds, [0, 0, 24, 20]);
        assert!(!outside.mask.contains(6, 5) && !outside.mask.contains(10, 10));
        // A seed on a line, or outside the canvas, selects nothing.
        assert_eq!(
            Selection::flood(&pixels, size, [6, 5], Compare::AlphaBoundary),
            None
        );
        assert_eq!(
            Selection::flood(&pixels, size, [24, 0], Compare::AlphaBoundary),
            None
        );
    }

    #[test]
    fn a_colour_flood_compares_every_straight_channel_with_the_tolerance() {
        let premultiplied = |[r, g, b, a]: [u8; 4]| {
            let scale = |channel: u8| ((u32::from(channel) * u32::from(a) + 127) / 255) as u8;
            [scale(r), scale(g), scale(b), a]
        };
        let pixels = [
            [100, 100, 100, 255],
            [105, 96, 103, 250],
            [112, 100, 100, 255],
            [100, 100, 100, 200],
        ]
        .map(premultiplied);
        let tight = Selection::flood(&pixels, [4, 1], [0, 0], Compare::Color(5)).unwrap();
        assert_eq!(tight.mask.bounds, [0, 0, 2, 1]);
        let widest = Selection::flood(&pixels, [4, 1], [0, 0], Compare::Color(255)).unwrap();
        assert_eq!(widest.mask.bounds, [0, 0, 4, 1]);
    }

    #[test]
    fn unpremultiplying_rounds_as_qt_does() {
        assert_eq!(straight([0, 0, 0, 0]), [0; 4]);
        assert_eq!(straight([10, 20, 30, 255]), [10, 20, 30, 255]);
        // Qt: (c · ⌊0xff00ff / a⌋ + 0x8000) >> 16.
        assert_eq!(straight([1, 63, 127, 128]), [2, 126, 253, 128]);
        assert_eq!(straight([1, 2, 3, 3]), [85, 170, 255, 3]);
    }

    #[test]
    fn affine_helpers_compose_invert_and_turn_about_a_centre() {
        let turn = Affine::rotation_about(std::f64::consts::FRAC_PI_2, [10.0, 10.0]);
        let [x, y] = turn.apply([20.0, 10.0]);
        assert!(
            (x - 10.0).abs() < 1e-9 && (y - 20.0).abs() < 1e-9,
            "clockwise on screen"
        );
        let both = Affine::scaling_about([2.0, 3.0], [1.0, 1.0]).then(turn);
        let back = both.inverse().unwrap();
        let [x, y] = back.apply(both.apply([7.0, -4.0]));
        assert!((x - 7.0).abs() < 1e-9 && (y + 4.0).abs() < 1e-9);
        assert_eq!(
            Affine::scaling_about([0.0, 1.0], [0.0, 0.0]).inverse(),
            None
        );
    }

    #[test]
    fn a_transformed_selection_holds_every_pixel_the_moved_squares_reach() {
        let square = rectangle([4.0, 4.0], [10.0, 8.0]).unwrap();
        // Whole-pixel moves shift the bits; off the canvas is dropped.
        let moved = square.transformed(Affine::translation(3.0, -2.0)).unwrap();
        assert_eq!(moved.mask.bounds, [7, 2, 6, 4]);
        assert_eq!(pixels(Some(&moved)).len(), 24);
        let off = square.transformed(Affine::translation(33.0, 0.0)).unwrap();
        assert_eq!(off.mask.bounds, [37, 4, 3, 4]);
        assert_eq!(square.transformed(Affine::translation(100.0, 0.0)), None);
        // A quarter turn about a pixel corner turns the rectangle exactly.
        let turned = square
            .transformed(Affine::rotation_about(
                std::f64::consts::FRAC_PI_2,
                [10.0, 8.0],
            ))
            .unwrap();
        assert_eq!(turned.mask.bounds, [10, 2, 4, 6]);
        assert_eq!(pixels(Some(&turned)).len(), 24);
        // Half a pixel off: each moved square reaches two columns.
        let half = square.transformed(Affine::translation(0.5, 0.0)).unwrap();
        assert_eq!(half.mask.bounds, [4, 4, 7, 4]);
        // Turned 30°: every pixel a point of the moved squares lands in is
        // held, and nothing far from them.
        let transform = Affine::rotation_about(std::f64::consts::FRAC_PI_6, [7.0, 6.0]);
        let tilted = square.transformed(transform).unwrap();
        let inverse = transform.inverse().unwrap();
        for y in 0..CANVAS[1] as i32 {
            for x in 0..CANVAS[0] as i32 {
                let hits = (0..8)
                    .flat_map(|i| (0..8).map(move |j| (i, j)))
                    .any(|(i, j)| {
                        let point = [
                            f64::from(x) + (f64::from(i) + 0.5) / 8.0,
                            f64::from(y) + (f64::from(j) + 0.5) / 8.0,
                        ];
                        let [sx, sy] = inverse.apply(point);
                        square.mask.contains(sx.floor() as i32, sy.floor() as i32)
                    });
                if hits {
                    assert!(tilted.mask.contains(x, y), "{x},{y} reached but not held");
                }
                if tilted.mask.contains(x, y) {
                    let [sx, sy] = inverse.apply([f64::from(x) + 0.5, f64::from(y) + 0.5]);
                    assert!(
                        (3.0..11.5).contains(&sx) && (3.0..9.5).contains(&sy),
                        "{x},{y} too far"
                    );
                }
            }
        }
    }
}

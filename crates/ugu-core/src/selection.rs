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

/// Pixels as bits over a whole canvas, each row padded to whole bytes as
/// in a `Mask`, for combining.
struct Canvas {
    size: [usize; 2],
    row_bytes: usize,
    bits: Vec<u8>,
}

impl Canvas {
    fn empty(size: [u32; 2]) -> Self {
        let size = size.map(|edge| edge as usize);
        let row_bytes = size[0].div_ceil(8);
        Self {
            size,
            row_bytes,
            bits: vec![0; row_bytes * size[1]],
        }
    }

    fn of(selection: &Selection) -> Self {
        let mut canvas = Self::empty(selection.canvas);
        let [left, top, width, _] = selection.mask.bounds;
        let mask_bytes = Mask::row_bytes(width);
        let shift = left as usize % 8;
        for (row, source) in selection.mask.bits.chunks_exact(mask_bytes).enumerate() {
            let at = (top as usize + row) * canvas.row_bytes + left as usize / 8;
            let out = &mut canvas.bits[at..(top as usize + row + 1) * canvas.row_bytes];
            // The mask's bits past its width are clear, so they set nothing.
            for (index, &byte) in source.iter().enumerate() {
                out[index] |= byte >> shift;
                if shift != 0
                    && let Some(next) = out.get_mut(index + 1)
                {
                    *next |= byte << (8 - shift);
                }
            }
        }
        canvas
    }

    fn get(&self, x: usize, y: usize) -> bool {
        self.bits[y * self.row_bytes + x / 8] & (0x80 >> (x % 8)) != 0
    }

    /// Sets the pixels from `from` to `to` (exclusive) on row `y`.
    fn fill(&mut self, y: usize, from: usize, to: usize) {
        if from >= to {
            return;
        }
        let row = &mut self.bits[y * self.row_bytes..(y + 1) * self.row_bytes];
        let [first, last] = [from / 8, (to - 1) / 8];
        let head = 0xffu8 >> (from % 8);
        let tail = 0xffu8 << (7 - (to - 1) % 8);
        if first == last {
            row[first] |= head & tail;
        } else {
            row[first] |= head;
            row[first + 1..last].fill(0xff);
            row[last] |= tail;
        }
    }

    /// Flips every pixel, leaving the padding after each row clear.
    fn invert(&mut self) {
        let width = self.size[0];
        let padding = if width.is_multiple_of(8) {
            0xff
        } else {
            0xffu8 << (8 - width % 8)
        };
        for row in self.bits.chunks_exact_mut(self.row_bytes) {
            for byte in row.iter_mut() {
                *byte = !*byte;
            }
            if let Some(last) = row.last_mut() {
                *last &= padding;
            }
        }
    }

    /// The selection of the pixels set; `None` when there are none.
    fn selection(self) -> Option<Selection> {
        let [width, height] = self.size.map(|edge| edge as i32);
        let canvas = [width as u32, height as u32];
        tight(canvas, [0, 0, width, height], self.bits)
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
    /// Its mask's size in bytes.
    pub fn bytes(&self) -> usize {
        self.mask.bits.len()
    }

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
            if reached.get(x, y) || !open(x, y) {
                continue;
            }
            let mut left = x;
            while left > 0 && !reached.get(left - 1, y) && open(left - 1, y) {
                left -= 1;
            }
            let mut right = x;
            while right + 1 < width && !reached.get(right + 1, y) && open(right + 1, y) {
                right += 1;
            }
            reached.fill(y, left, right + 1);
            for next in [y.wrapping_sub(1), y + 1] {
                if next >= height {
                    continue;
                }
                let mut x = left;
                while x <= right {
                    if !reached.get(x, next) && open(x, next) {
                        pending.push([x, next]);
                        while x < right && !reached.get(x + 1, next) && open(x + 1, next) {
                            x += 1;
                        }
                    }
                    x += 1;
                }
            }
        }
        reached.selection()
    }

    /// The pixels of `pixels`, premultiplied rows of a canvas of `canvas`,
    /// that stop a fill by alpha; `None` when there are none.
    pub fn opaque(pixels: &[[u8; 4]], canvas: [u32; 2]) -> Option<Self> {
        let width = canvas[0] as usize;
        if width == 0 || pixels.len() != width * canvas[1] as usize {
            return None;
        }
        let mut drawn = Canvas::empty(canvas);
        for (y, row) in pixels.chunks_exact(width).enumerate() {
            let mut x = 0;
            while x < width {
                let Some(from) = row[x..].iter().position(|pixel| pixel[3] >= 128) else {
                    break;
                };
                let from = x + from;
                let to = row[from..]
                    .iter()
                    .position(|pixel| pixel[3] < 128)
                    .map_or(width, |run| from + run);
                drawn.fill(y, from, to);
                x = to;
            }
        }
        drawn.selection()
    }

    /// This flooded area without what it reached through openings up to
    /// about `gap` pixels wide; `seed` is the pixel it was flooded from.
    ///
    /// The area's wide parts are its pixels farther from its edge than the
    /// middle of such an opening can be, joined by their edges, so no
    /// opening joins two of them. The result is the wide part at the seed,
    /// or the one nearest it, with the narrow pixels that are no nearer
    /// another wide part: an opening is shared down its middle with what is
    /// past it, and a corner or a strand that leads nowhere else is kept
    /// whole. An area with one wide part or none is returned as it is. The
    /// canvas's edge is not an edge of the area: a margin along it is as
    /// wide as the lines leave it.
    pub fn closing_gaps(&self, seed: [u32; 2], gap: u8) -> Selection {
        const NARROW: u8 = 1;
        const WIDE: u8 = 2;
        const ROOMY: u8 = 3;
        const KEPT: u8 = 4;
        const OTHERS: u8 = 5;
        let [left, top, width, height] = self.mask.bounds;
        if gap == 0 || !self.mask.contains(seed[0] as i32, seed[1] as i32) {
            return self.clone();
        }
        let [width, height] = [width as usize, height as usize];
        let seed = (seed[1] as usize - top as usize) * width + (seed[0] as usize - left as usize);
        let ends_at_canvas = [
            left == 0,
            top == 0,
            left as usize + width == self.canvas[0] as usize,
            top as usize + height == self.canvas[1] as usize,
        ];
        // With d the distance between a pixel's centre and the nearest
        // centre outside the area, a pixel is narrow when 4 d² <= (gap + 1)²:
        // the middle pixel of an opening `gap` wide is (gap + 1) / 2 from
        // the nearer end. It is roomy when d is over a pixel more.
        let gap = usize::from(gap);
        let limits = [(gap + 1).pow(2), (gap + 3).pow(2)];
        let reach = (gap + 3) / 2;
        // How far along a row a pixel `rows` rows from the outside keeps
        // pixels within each limit.
        let along = limits.map(|limit| {
            (0..=reach)
                .map(|rows| {
                    (0..=reach)
                        .rev()
                        .find(|columns| 4 * (rows * rows + columns * columns) <= limit)
                        .map_or(-1, |columns| columns as isize)
                })
                .collect::<Vec<_>>()
        });
        let row_bytes = Mask::row_bytes(width as i32);
        // Rows to the nearest pixel outside the area in each column, at most
        // one past `reach`. Everything past the bounds is outside the area,
        // except where the canvas ends there.
        let cap = reach as u8 + 1;
        let past = |at_canvas: bool| if at_canvas { cap } else { 1 };
        let mut labels = vec![0u8; width * height];
        let mut rows_out = vec![0u8; width * height];
        for y in 0..height {
            let bits = &self.mask.bits[y * row_bytes..][..row_bytes];
            let labels = &mut labels[y * width..][..width];
            let (above, row) = rows_out.split_at_mut(y * width);
            let above = (y > 0).then(|| &above[(y - 1) * width..]);
            for x in 0..width {
                if bits[x / 8] & (0x80 >> (x % 8)) != 0 {
                    labels[x] = ROOMY;
                    row[x] = above.map_or(past(ends_at_canvas[1]), |above| (above[x] + 1).min(cap));
                }
            }
        }
        for y in (0..height).rev() {
            let (row, below) = rows_out[y * width..].split_at_mut(width);
            for (x, rows) in row.iter_mut().enumerate() {
                let down = below
                    .get(x)
                    .map_or(past(ends_at_canvas[3]), |&below| below + 1);
                *rows = (*rows).min(down);
            }
        }
        // Every pixel starts roomy, and each pixel near the outside in its
        // column takes the room of those beside it in its row. Few pixels
        // are near the outside, so most of a row is passed over.
        for (row, out) in labels
            .chunks_exact_mut(width)
            .zip(rows_out.chunks_exact(width))
        {
            let mut narrow = |x: isize, rows: u8| {
                for (along, label) in along.iter().zip([NARROW, WIDE]) {
                    let columns = along[usize::from(rows)];
                    let from = (x - columns).max(0) as usize;
                    let to = (x + columns + 1).clamp(0, width as isize) as usize;
                    if columns >= 0 && from < to {
                        for at in &mut row[from..to] {
                            *at = (*at).min(label);
                        }
                    }
                }
            };
            if !ends_at_canvas[0] {
                narrow(-1, 0);
            }
            if !ends_at_canvas[2] {
                narrow(width as isize, 0);
            }
            for (x, &rows) in out.iter().enumerate() {
                // Of the pixels outside, the ones beside the area do it all.
                let beside =
                    || (x > 0 && out[x - 1] != 0) || out.get(x + 1).is_some_and(|&next| next != 0);
                if rows < cap && (rows != 0 || beside()) {
                    narrow(x as isize, rows);
                }
            }
        }
        let neighbours = |at: usize| {
            let x = at % width;
            [
                (x > 0).then(|| at - 1),
                (x + 1 < width).then_some(at + 1),
                (at >= width).then(|| at - width),
                (at + width < width * height).then_some(at + width),
            ]
            .into_iter()
            .flatten()
        };
        // Marks the wide part `start` is in as `to`; returns whether any of
        // it is roomy.
        let mut pending = Vec::new();
        let mut mark =
            |labels: &mut [u8], start: usize, from: std::ops::RangeInclusive<u8>, to: u8| {
                let mut roomy = false;
                pending.push(start);
                while let Some(at) = pending.pop() {
                    if !from.contains(&labels[at]) {
                        continue;
                    }
                    let row = at - at % width;
                    let mut first = at;
                    while first > row && from.contains(&labels[first - 1]) {
                        first -= 1;
                    }
                    let mut last = at;
                    while last + 1 < row + width && from.contains(&labels[last + 1]) {
                        last += 1;
                    }
                    roomy |= labels[first..=last].contains(&ROOMY);
                    labels[first..=last].fill(to);
                    let rows = [
                        (first >= width).then(|| first - width),
                        (last + width < width * height).then_some(first + width),
                    ];
                    for start in rows.into_iter().flatten() {
                        let mut run = false;
                        for (next, label) in (start..).zip(&labels[start..=start + (last - first)])
                        {
                            let wide = from.contains(label);
                            if wide && !run {
                                pending.push(next);
                            }
                            run = wide;
                        }
                    }
                }
                roomy
            };
        // The wide part at the seed, or the first one reached from it
        // through narrow pixels.
        let mut start = (labels[seed] != NARROW).then_some(seed);
        if start.is_none() {
            let mut seen = vec![false; width * height];
            seen[seed] = true;
            let mut queue = std::collections::VecDeque::from([seed]);
            'search: while let Some(at) = queue.pop_front() {
                for next in neighbours(at) {
                    if labels[next] > NARROW {
                        start = Some(next);
                        break 'search;
                    }
                    if labels[next] == NARROW && !seen[next] {
                        seen[next] = true;
                        queue.push_back(next);
                    }
                }
            }
        }
        let Some(start) = start else {
            return self.clone();
        };
        mark(&mut labels, start, WIDE..=ROOMY, KEPT);
        // Another wide part takes its share only when some of it is roomy.
        // Where a passage is about as wide as the gap, its pixels are wide
        // in specks that join nothing, and each would stop the area.
        let mut others = false;
        for at in 0..labels.len() {
            if labels[at] == WIDE || labels[at] == ROOMY {
                if mark(&mut labels, at, WIDE..=ROOMY, OTHERS) {
                    others = true;
                } else {
                    mark(&mut labels, at, OTHERS..=OTHERS, NARROW);
                }
            }
        }
        if !others {
            return self.clone();
        }
        // Each narrow pixel goes to the wide part it is fewest steps from,
        // the kept one when as near.
        let mut kept = Vec::new();
        let mut theirs = Vec::new();
        for at in 0..labels.len() {
            if labels[at] != NARROW {
                continue;
            }
            let mut beside = [false; 2];
            for next in neighbours(at) {
                beside[0] |= labels[next] == KEPT;
                beside[1] |= labels[next] == OTHERS;
            }
            if beside[0] {
                kept.push(at);
            } else if beside[1] {
                theirs.push(at);
            }
        }
        // A ring of steps is marked only once it is whole, so that its
        // pixels do not count as wide for each other.
        for (ring, label) in [(&kept, KEPT), (&theirs, OTHERS)] {
            for &at in ring {
                labels[at] = label;
            }
        }
        let mut next_ring = Vec::new();
        while !kept.is_empty() || !theirs.is_empty() {
            for (ring, label) in [(&mut kept, KEPT), (&mut theirs, OTHERS)] {
                for &at in ring.iter() {
                    for next in neighbours(at) {
                        if labels[next] == NARROW {
                            labels[next] = label;
                            next_ring.push(next);
                        }
                    }
                }
                ring.clear();
                std::mem::swap(ring, &mut next_ring);
            }
        }
        let mut bits = vec![0u8; row_bytes * height];
        for (row, out) in labels
            .chunks_exact(width)
            .zip(bits.chunks_exact_mut(row_bytes))
        {
            for (x, &label) in row.iter().enumerate() {
                if label == KEPT {
                    out[x / 8] |= 0x80 >> (x % 8);
                }
            }
        }
        tight(self.canvas, self.mask.bounds, bits).unwrap_or_else(|| self.clone())
    }

    /// This area grown into the lines that bound it, up to their middles:
    /// the pixels of `pixels` (premultiplied rows of the canvas) at least
    /// half opaque that are nearer this area than any other open pixel, at
    /// most `reach` steps in. A fill laid behind the lines then meets them
    /// when a line's edges move by less than half its width, and never shows
    /// past it.
    pub fn under_lines(&self, pixels: &[[u8; 4]], reach: u8) -> Selection {
        let [width, height] = self.canvas.map(|edge| edge as usize);
        if pixels.len() != width * height || reach == 0 {
            return self.clone();
        }
        let inside = Canvas::of(self);
        let line = |x: usize, y: usize| pixels[y * width + x][3] >= 128;
        let [left, top, mask_width, mask_height] = self.mask.bounds;
        let reach_px = i32::from(reach);
        let [x0, y0] = [left - reach_px, top - reach_px].map(|at| at.max(0) as usize);
        let x1 = ((left + mask_width + reach_px) as usize).min(width);
        let y1 = ((top + mask_height + reach_px) as usize).min(height);
        let box_width = x1 - x0;
        let at = |x: usize, y: usize| (y - y0) * box_width + (x - x0);
        // Steps into the lines from this area and from every other open
        // pixel, each from the line pixels next to them.
        let mut near = vec![u8::MAX; box_width * (y1 - y0)];
        let mut far = near.clone();
        let mut from_near = std::collections::VecDeque::new();
        let mut from_far = std::collections::VecDeque::new();
        let neighbours = |x: usize, y: usize| {
            [
                (x > 0).then(|| (x - 1, y)),
                (x + 1 < width).then_some((x + 1, y)),
                (y > 0).then(|| (x, y - 1)),
                (y + 1 < height).then_some((x, y + 1)),
            ]
            .into_iter()
            .flatten()
        };
        for y in y0..y1 {
            for x in x0..x1 {
                if !line(x, y) {
                    continue;
                }
                for (nx, ny) in neighbours(x, y) {
                    if line(nx, ny) {
                        continue;
                    }
                    if inside.get(nx, ny) {
                        if near[at(x, y)] != 1 {
                            near[at(x, y)] = 1;
                            from_near.push_back((x, y));
                        }
                    } else if far[at(x, y)] != 1 {
                        far[at(x, y)] = 1;
                        from_far.push_back((x, y));
                    }
                }
            }
        }
        for (steps, queue) in [(&mut near, &mut from_near), (&mut far, &mut from_far)] {
            while let Some((x, y)) = queue.pop_front() {
                let next = steps[at(x, y)] + 1;
                if next > reach {
                    continue;
                }
                for (nx, ny) in neighbours(x, y) {
                    let inside_box = (x0..x1).contains(&nx) && (y0..y1).contains(&ny);
                    if inside_box && line(nx, ny) && steps[at(nx, ny)] > next {
                        steps[at(nx, ny)] = next;
                        queue.push_back((nx, ny));
                    }
                }
            }
        }
        let mut grown = inside;
        for y in y0..y1 {
            for x in x0..x1 {
                let (near, far) = (near[at(x, y)], far[at(x, y)]);
                // A middle pixel as near either side is the other side's:
                // the fill's soft edge goes behind it.
                if near <= reach && near < far {
                    grown.fill(y, x, x + 1);
                }
            }
        }
        grown.selection().unwrap_or_else(|| self.clone())
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
        let bits =
            |row: i32| &self.mask.bits[row as usize * row_bytes..(row as usize + 1) * row_bytes];
        let mut pixels = Canvas::empty(self.canvas);
        let [canvas_width, canvas_height] = self.canvas.map(f64::from);
        let mut row = 0;
        while row < height {
            // Rows with the same runs make rectangles, moved as one.
            let same = bits(row);
            let mut end = row + 1;
            while end < height && bits(end) == same {
                end += 1;
            }
            let [y0, y1] = [top + row, top + end].map(f64::from);
            for [from, to] in runs(same, width) {
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
                for (byte, &set) in pixels.bits.iter_mut().zip(&other.bits) {
                    if how == Combine::Add {
                        *byte |= set;
                    } else {
                        *byte &= !set;
                    }
                }
                pixels.selection()
            }
        }
    }

    /// The canvas pixels not selected; `None` when every one was.
    pub fn invert(&self) -> Option<Self> {
        let mut pixels = Canvas::of(self);
        pixels.invert();
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

/// The runs of set bits in a row of `width` bits, as from and to
/// (exclusive).
fn runs(bits: &[u8], width: i32) -> Vec<[i32; 2]> {
    let mut runs = Vec::new();
    let mut start = None;
    for (index, &byte) in bits.iter().enumerate() {
        let at = index as i32 * 8;
        if (byte == 0 && start.is_none()) || (byte == 0xff && start.is_some()) {
            continue;
        }
        for bit in 0..8 {
            let set = byte & (0x80 >> bit) != 0;
            match (set, start) {
                (true, None) => start = Some(at + bit),
                (false, Some(from)) => {
                    runs.push([from, at + bit]);
                    start = None;
                }
                _ => {}
            }
        }
    }
    if let Some(from) = start {
        runs.push([from, width]);
    }
    runs
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
            let [width, height] = pixels.size;
            for y in 0..height {
                for x in 0..width {
                    state ^= state << 13;
                    state ^= state >> 7;
                    state ^= state << 17;
                    if state % 10 < density {
                        pixels.fill(y, x, x + 1);
                    }
                }
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
    fn under_lines_a_fill_reaches_the_middle_of_each_line_that_stops_it() {
        // A line 6 pixels wide at x 10..16 and one 1 pixel wide at x 20.
        let size = [30, 9];
        let pixels: Vec<[u8; 4]> = (0..30 * 9)
            .map(|at| match at % 30 {
                10..16 | 20 => [0, 0, 0, 255],
                _ => [0; 4],
            })
            .collect();
        let columns = |selection: &Selection| {
            (0..30)
                .filter(|&x| (0..9).all(|y| selection.mask.contains(x, y)))
                .collect::<Vec<_>>()
        };
        let left = Selection::flood(&pixels, size, [2, 4], Compare::AlphaBoundary).unwrap();
        assert_eq!(
            columns(&left.under_lines(&pixels, 16)),
            (0..13).collect::<Vec<_>>()
        );
        assert_eq!(
            columns(&left.under_lines(&pixels, 2)),
            (0..12).collect::<Vec<_>>()
        );
        // Between the two: half the wide line, none of the thin one, whose
        // only pixel is as near the other side.
        let between = Selection::flood(&pixels, size, [17, 4], Compare::AlphaBoundary).unwrap();
        assert_eq!(
            columns(&between.under_lines(&pixels, 16)),
            (13..20).collect::<Vec<_>>()
        );
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
    fn opaque_is_what_a_fill_by_alpha_stops_at() {
        let size = [20u32, 6];
        let mut pixels = vec![[0u8; 4]; 120];
        for (x, y, alpha) in [(3, 1, 255), (4, 1, 128), (5, 1, 127), (17, 4, 200)] {
            pixels[y * 20 + x] = [0, 0, 0, alpha];
        }
        let drawn = Selection::opaque(&pixels, size).unwrap();
        assert_eq!(drawn.mask.bounds, [3, 1, 15, 4]);
        for y in 0..6 {
            for x in 0..20 {
                assert_eq!(
                    drawn.mask.contains(x, y),
                    [(3, 1), (4, 1), (17, 4)].contains(&(x, y)),
                    "{x} {y}"
                );
            }
        }
        // With the area a fill reaches from a clear pixel, the whole canvas.
        let clear = Selection::flood(&pixels, size, [0, 0], Compare::AlphaBoundary).unwrap();
        assert_eq!(
            Selection::combine(Some(&clear), Some(drawn), Combine::Add),
            Selection::all(size)
        );
        assert_eq!(Selection::opaque(&[[0; 4]; 120], size), None);
        assert_eq!(Selection::opaque(&pixels, [20, 5]), None);
    }

    /// An 80×60 canvas with an opaque box outline around x 20..60, y 15..45,
    /// two pixels thick, open for `gap` rows from y 30 in its right side.
    fn open_box(gap: usize) -> (Vec<[u8; 4]>, [u32; 2]) {
        let mut pixels = vec![[0; 4]; 80 * 60];
        for y in 15..45 {
            for x in 20..60 {
                let wall = !(22..58).contains(&x) || !(17..43).contains(&y);
                if wall && !(x >= 58 && (30..30 + gap).contains(&y)) {
                    pixels[y * 80 + x] = [0, 0, 0, 255];
                }
            }
        }
        (pixels, [80, 60])
    }

    fn count(selection: &Selection) -> u32 {
        selection
            .mask
            .bits
            .iter()
            .map(|byte| byte.count_ones())
            .sum()
    }

    #[test]
    fn closing_gaps_keeps_a_flood_inside_an_outline_with_an_opening() {
        for gap in 1..=8u8 {
            let (pixels, size) = open_box(usize::from(gap));
            let seed = [40, 30];
            let leaked = Selection::flood(&pixels, size, seed, Compare::AlphaBoundary).unwrap();
            assert_eq!(leaked.mask.bounds, [0, 0, 80, 60], "a {gap} px opening");
            assert_eq!(leaked.closing_gaps(seed, 0), leaked);
            let inside = leaked.closing_gaps(seed, gap);
            // The whole inside and the inner half of the opening.
            assert_eq!(
                (inside.mask.bounds, count(&inside)),
                ([22, 17, 37, 26], 36 * 26 + u32::from(gap)),
                "a {gap} px opening"
            );
            // The outside takes the rest, so filling both leaves no hole.
            let outside = leaked.closing_gaps([2, 2], gap);
            assert_eq!(
                Selection::combine(Some(&inside), Some(outside.clone()), Combine::Add),
                Some(leaked.clone()),
                "a {gap} px opening"
            );
            assert_eq!(
                count(&inside) + count(&outside),
                count(&leaked),
                "a {gap} px opening"
            );
            // An opening two pixels wider than asked for is a way through.
            if gap > 2 {
                assert_eq!(leaked.closing_gaps(seed, gap - 2), leaked);
            }
        }
    }

    #[test]
    fn closing_gaps_changes_nothing_in_an_area_with_no_opening() {
        let (pixels, size) = ring();
        let inside = Selection::flood(&pixels, size, [10, 10], Compare::AlphaBoundary).unwrap();
        let outside = Selection::flood(&pixels, size, [0, 0], Compare::AlphaBoundary).unwrap();
        for gap in [1, 2, 5, 20, 255] {
            assert_eq!(inside.closing_gaps([10, 10], gap), inside);
            assert_eq!(outside.closing_gaps([0, 0], gap), outside);
        }
    }

    /// Two rooms, x 2..30 and x 50..78 of y 2..38, joined by a corridor
    /// `width` rows high from y 20, with a strand one pixel wide running up
    /// from the left room to y 0 at x 10. All in an opaque 80×40 canvas.
    fn rooms(width: usize) -> (Vec<[u8; 4]>, [u32; 2]) {
        let mut pixels = vec![[0, 0, 0, 255]; 80 * 40];
        for y in 0..40 {
            for x in 0..80 {
                let room = (2..38).contains(&y) && ((2..30).contains(&x) || (50..78).contains(&x));
                let corridor = (20..20 + width).contains(&y) && (30..50).contains(&x);
                if room || corridor || (x == 10 && y < 2) {
                    pixels[y * 80 + x] = [0; 4];
                }
            }
        }
        (pixels, [80, 40])
    }

    #[test]
    fn closing_gaps_goes_through_a_corridor_wider_than_the_gap() {
        for (gap, width) in [(1, 3), (2, 4), (3, 5), (4, 6), (8, 10), (16, 18)] {
            let (pixels, size) = rooms(width);
            let both = Selection::flood(&pixels, size, [10, 10], Compare::AlphaBoundary).unwrap();
            assert_eq!(both.mask.bounds, [2, 0, 76, 38]);
            assert_eq!(
                both.closing_gaps([10, 10], gap),
                both,
                "gap {gap}, a corridor of {width}"
            );
        }
    }

    #[test]
    fn closing_gaps_shares_a_narrow_corridor_and_keeps_what_leads_nowhere() {
        for (gap, width) in [(1, 1), (2, 2), (4, 3), (4, 4), (10, 9)] {
            let (pixels, size) = rooms(width);
            let both = Selection::flood(&pixels, size, [10, 10], Compare::AlphaBoundary).unwrap();
            let left = both.closing_gaps([10, 10], gap);
            // The left room with its strand and the nearer half of the
            // corridor.
            assert_eq!(
                (left.mask.bounds, count(&left)),
                ([2, 0, 38, 38], (28 * 36 + 2 + 10 * width) as u32),
                "gap {gap}, a corridor of {width}"
            );
            // A click in the strand or in a corner fills the room it is in,
            // one in the corridor the nearer room.
            for seed in [[10, 0], [2, 2], [29, 37], [32, 20]] {
                assert_eq!(both.closing_gaps(seed, gap), left, "from {seed:?}");
            }
            let right = both.closing_gaps([48, 20], gap);
            assert_eq!(right.mask.bounds, [40, 2, 38, 36]);
            assert_eq!(count(&left) + count(&right), count(&both));
        }
    }

    #[test]
    fn closing_gaps_keeps_a_fill_in_a_small_outline_with_an_opening() {
        // A box 4 pixels across inside, open for one row on its right, in
        // a wide canvas: wide in its middle, but nowhere roomy. The fill
        // keeps the box and the pixel of its opening.
        let size = [40u32, 30];
        let mut pixels = vec![[0u8; 4]; 40 * 30];
        for y in 10..16 {
            for x in 10..16 {
                let wall = !(11..15).contains(&x) || !(11..15).contains(&y);
                if wall && !(x == 15 && y == 12) {
                    pixels[y * 40 + x] = [0, 0, 0, 255];
                }
            }
        }
        let leaked = Selection::flood(&pixels, size, [12, 12], Compare::AlphaBoundary).unwrap();
        assert_eq!(leaked.mask.bounds, [0, 0, 40, 30]);
        for seed in [[12, 12], [11, 11], [14, 14]] {
            let inside = leaked.closing_gaps(seed, 2);
            assert_eq!(
                (inside.mask.bounds, count(&inside)),
                ([11, 11, 5, 4], 17),
                "from {seed:?}"
            );
        }
    }

    #[test]
    fn closing_gaps_keeps_an_area_too_small_to_have_a_wide_part() {
        let (pixels, size) = ring();
        let inside = Selection::flood(&pixels, size, [10, 10], Compare::AlphaBoundary).unwrap();
        // 8 by 6 pixels: nothing is more than 3 from the line.
        assert_eq!(inside.closing_gaps([10, 10], 6), inside);
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
        // Finite, but its inverse moves past the largest float.
        assert_eq!(Affine([1e-300, 0.0, 1e100, 0.0, 1.0, 0.0]).inverse(), None);
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

    /// What flooding a fill again on every frame would cost (evidence.md
    /// section 81), at
    /// sizes from a small box to a fill across a large canvas.
    #[test]
    #[ignore]
    fn measure_flood_per_frame() {
        for (edge, radius, width) in [
            (320u32, 80.0f64, 2.0f64),
            (1024, 400.0, 3.0),
            (2048, 900.0, 4.0),
            (4096, 1900.0, 6.0),
        ] {
            let n = edge as usize;
            let centre = n as f64 / 2.0;
            // A wobbly ring of opaque line pixels.
            let pixels: Vec<[u8; 4]> = (0..n * n)
                .map(|i| {
                    let (x, y) = ((i % n) as f64 + 0.5 - centre, (i / n) as f64 + 0.5 - centre);
                    let angle = y.atan2(x);
                    let r = radius * (1.0 + 0.03 * (angle * 7.0).sin());
                    if ((x * x + y * y).sqrt() - r).abs() <= width / 2.0 {
                        [0, 0, 0, 255]
                    } else {
                        [0; 4]
                    }
                })
                .collect();
            let mut flood = Vec::new();
            let mut under = Vec::new();
            for _ in 0..15 {
                let started = std::time::Instant::now();
                let area = Selection::flood(
                    &pixels,
                    [edge, edge],
                    [edge / 2, edge / 2],
                    Compare::AlphaBoundary,
                )
                .unwrap();
                flood.push(started.elapsed().as_secs_f64() * 1000.0);
                let started = std::time::Instant::now();
                let grown = area.under_lines(&pixels, 16);
                under.push(started.elapsed().as_secs_f64() * 1000.0);
                std::hint::black_box(grown);
            }
            flood.sort_by(f64::total_cmp);
            under.sort_by(f64::total_cmp);
            let filled = std::f64::consts::PI * radius * radius;
            eprintln!(
                "{edge}x{edge}, fill about {:.0}k px: flood median {:.2} ms (min {:.2}), under_lines median {:.2} ms (min {:.2})",
                filled / 1000.0,
                flood[7],
                flood[0],
                under[7],
                under[0]
            );
        }
    }

    /// What closing gaps adds to finding an area (evidence.md section 136):
    /// a wobbly ring, closed or with a 2 pixel opening, filled from its
    /// middle.
    #[test]
    #[ignore]
    fn measure_closing_gaps() {
        for (edge, radius, width) in [
            (1024u32, 400.0f64, 3.0f64),
            (2048, 900.0, 4.0),
            (4096, 1900.0, 6.0),
            (8192, 3800.0, 8.0),
        ] {
            let n = edge as usize;
            let centre = n as f64 / 2.0;
            for opening in [0.0, 2.0] {
                let pixels: Vec<[u8; 4]> = (0..n * n)
                    .map(|i| {
                        let (x, y) = ((i % n) as f64 + 0.5 - centre, (i / n) as f64 + 0.5 - centre);
                        let angle = y.atan2(x);
                        let r = radius * (1.0 + 0.03 * (angle * 7.0).sin());
                        let line = ((x * x + y * y).sqrt() - r).abs() <= width / 2.0;
                        if line && angle.abs() * r >= opening / 2.0 {
                            [0, 0, 0, 255]
                        } else {
                            [0; 4]
                        }
                    })
                    .collect();
                let seed = [edge / 2, edge / 2];
                let median = |run: &mut dyn FnMut()| {
                    let mut times: Vec<f64> = (0..9)
                        .map(|_| {
                            let started = std::time::Instant::now();
                            run();
                            started.elapsed().as_secs_f64() * 1000.0
                        })
                        .collect();
                    times.sort_by(f64::total_cmp);
                    times[4]
                };
                let flood = || {
                    Selection::flood(&pixels, [edge, edge], seed, Compare::AlphaBoundary).unwrap()
                };
                let area = flood();
                let flooding = median(&mut || {
                    std::hint::black_box(flood());
                });
                let closing = [2u8, 20].map(|gap| {
                    median(&mut || {
                        std::hint::black_box(area.closing_gaps(seed, gap));
                    })
                });
                let closed = area.closing_gaps(seed, 2);
                let under = median(&mut || {
                    std::hint::black_box(closed.under_lines(&pixels, 16));
                });
                eprintln!(
                    "{edge}x{edge}, opening {opening}: flooded {} px in {flooding:.1} ms, \
                     closing gaps of 2 {:.1} ms and of 20 {:.1} ms leaves {} px, under_lines {under:.1} ms",
                    count(&area),
                    closing[0],
                    closing[1],
                    count(&closed),
                );
            }
        }
    }

    /// How closing gaps holds on slanted lines (evidence.md section 136):
    /// how often a fill gets out of a ring through an opening of a given
    /// length, and how many clicks cover two rooms joined by a slanted
    /// corridor of a given width.
    #[test]
    #[ignore]
    fn measure_closing_gaps_on_slanted_lines() {
        let n = 240usize;
        let drawn = |open: &dyn Fn(f64, f64) -> bool| -> Vec<[u8; 4]> {
            (0..n * n)
                .map(|i| {
                    let at = [i % n, i / n].map(|at| at as f64 + 0.5 - 120.0);
                    if open(at[0], at[1]) {
                        [0; 4]
                    } else {
                        [0, 0, 0, 255]
                    }
                })
                .collect()
        };
        let flood = |pixels: &[[u8; 4]], seed| {
            Selection::flood(pixels, [n as u32; 2], seed, Compare::AlphaBoundary).unwrap()
        };
        for gap in [1u8, 2, 3, 4, 6, 10, 20] {
            let mut leaks = String::new();
            for tenths in (10..=(usize::from(gap) + 3) * 10).step_by(5) {
                let length = tenths as f64 / 10.0;
                let (mut out, mut total) = (0, 0);
                for thick in [1.2f64, 2.0, 3.0, 6.0] {
                    for angle in (0..90).step_by(2) {
                        let theta = f64::from(angle).to_radians();
                        let pixels = drawn(&|x, y| {
                            let mut turn = (y.atan2(x) - theta).abs();
                            if turn > std::f64::consts::PI {
                                turn = std::f64::consts::TAU - turn;
                            }
                            ((x * x + y * y).sqrt() - 80.0).abs() >= thick / 2.0
                                || turn * 80.0 < length / 2.0
                        });
                        let all = flood(&pixels, [120, 120]);
                        // A line this thin can hide an opening this short.
                        if !all.mask.contains(0, 0) {
                            continue;
                        }
                        total += 1;
                        out += u32::from(all.closing_gaps([120, 120], gap).mask.contains(0, 0));
                    }
                }
                leaks += &format!("{length}:{out}/{total} ");
            }
            let mut clicks = String::new();
            for tenths in (10..=(usize::from(gap) + 5) * 10).step_by(5) {
                let width = tenths as f64 / 10.0;
                let mut most = 0;
                for angle in (0..=45).step_by(3) {
                    let (sin, cos) = f64::from(angle).to_radians().sin_cos();
                    let pixels = drawn(&|x, y| {
                        let (u, v) = (x * cos + y * sin, y * cos - x * sin);
                        ((u.abs() - 80.0).abs() < 25.0 && v.abs() < 25.0)
                            || (u.abs() < 60.0 && v.abs() < width / 2.0)
                    });
                    let rooms = [-80.0, 80.0].map(|u: f64| [120.0 + u * cos, 120.0 + u * sin]);
                    let all = flood(&pixels, rooms[0].map(|at| at as u32));
                    // A click in each room, then wherever is left.
                    let mut covered: Option<Selection> = None;
                    let mut count = 0;
                    let mut seeds = rooms.map(|at| at.map(|at| at as u32)).to_vec();
                    while covered.as_ref() != Some(&all) {
                        let seed = seeds.pop().unwrap_or_else(|| {
                            let rest =
                                Selection::combine(Some(&all), covered.clone(), Combine::Subtract)
                                    .unwrap();
                            let [left, top, width, height] = rest.mask.bounds;
                            (top..top + height)
                                .flat_map(|y| (left..left + width).map(move |x| [x, y]))
                                .find(|&[x, y]| rest.mask.contains(x, y))
                                .unwrap()
                                .map(|at| at as u32)
                        });
                        let more = all.closing_gaps(seed, gap);
                        covered = Selection::combine(covered.as_ref(), Some(more), Combine::Add);
                        count += 1;
                    }
                    most = most.max(count);
                }
                clicks += &format!("{width}:{most} ");
            }
            eprintln!(
                "gap {gap}\n  opening length:leaks/tries {leaks}\n  corridor width:clicks {clicks}"
            );
        }
    }
}

// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! The selection: the canvas pixels later edits may touch. It belongs to
//! the session, not the document; an edit that uses it keeps a copy of its
//! mask (ADR section 2). Shapes are filled as 2.2.13 fills them: without
//! antialiasing, a pixel whose centre is inside, even-odd for a freehand
//! loop.

use std::sync::Arc;

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
        let from = Canvas::of(self);
        let mut to = Canvas::empty(size);
        let [width, height] = to.size;
        for y in 0..height {
            for x in 0..width {
                let source = [
                    x as i64 - i64::from(offset[0]),
                    y as i64 - i64::from(offset[1]),
                ];
                if source[0] >= 0
                    && source[1] >= 0
                    && (source[0] as usize) < from.size[0]
                    && (source[1] as usize) < from.size[1]
                {
                    to.pixels[y * width + x] =
                        from.pixels[source[1] as usize * from.size[0] + source[0] as usize];
                }
            }
        }
        to.selection()
    }

    /// The selection resampled to a canvas of `size` by the nearest pixel
    /// centre, as `Op::Resample` resamples with nearest sampling.
    pub fn resampled(&self, size: [u32; 2]) -> Option<Self> {
        let from = Canvas::of(self);
        let mut to = Canvas::empty(size);
        let [width, height] = to.size;
        let source = |index: usize, from_edge: usize, to_edge: usize| {
            ((2 * index + 1) * from_edge / (2 * to_edge)).min(from_edge - 1)
        };
        for y in 0..height {
            let row = source(y, from.size[1], height) * from.size[0];
            for x in 0..width {
                to.pixels[y * width + x] = from.pixels[row + source(x, from.size[0], width)];
            }
        }
        to.selection()
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

    /// The edges between selected and other pixels as closed loops of pixel
    /// corners, the selection on the right of each step; straight runs are
    /// one step.
    pub fn outline(&self) -> Vec<Vec<[i32; 2]>> {
        let [left, top, width, height] = self.mask.bounds;
        let set = |x: i32, y: i32| self.mask.contains(x, y);
        // Each edge from one corner to the next, keyed by where it starts.
        let mut next: std::collections::HashMap<[i32; 2], Vec<[i32; 2]>> =
            std::collections::HashMap::new();
        let mut edge = |from: [i32; 2], to: [i32; 2]| next.entry(from).or_default().push(to);
        for y in top..=top + height {
            for x in left..left + width {
                // The edge on top of pixel (x, y): going right with the
                // selection below, left with it above.
                match (set(x, y - 1), set(x, y)) {
                    (false, true) => edge([x, y], [x + 1, y]),
                    (true, false) => edge([x + 1, y], [x, y]),
                    _ => {}
                }
            }
        }
        for x in left..=left + width {
            for y in top..top + height {
                match (set(x - 1, y), set(x, y)) {
                    (false, true) => edge([x, y + 1], [x, y]),
                    (true, false) => edge([x, y], [x, y + 1]),
                    _ => {}
                }
            }
        }
        let mut loops = Vec::new();
        while let Some(&start) = next.keys().next() {
            let mut corners = vec![start];
            let mut at = start;
            loop {
                let ends = next.get_mut(&at).expect("every corner is left");
                let to = ends.pop().expect("an edge leaves it");
                if ends.is_empty() {
                    next.remove(&at);
                }
                if to == start {
                    break;
                }
                corners.push(to);
                at = to;
            }
            loops.push(straightened(corners));
        }
        loops
    }
}

/// `corners` without those in the middle of a straight run.
fn straightened(corners: Vec<[i32; 2]>) -> Vec<[i32; 2]> {
    let count = corners.len();
    (0..count)
        .filter(|&index| {
            let [before, here, after] = [
                corners[(index + count - 1) % count],
                corners[index],
                corners[(index + 1) % count],
            ];
            let turn = (here[0] - before[0]) * (after[1] - here[1])
                - (here[1] - before[1]) * (after[0] - here[0]);
            turn != 0
        })
        .map(|index| corners[index])
        .collect()
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
}

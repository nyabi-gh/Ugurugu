// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! The pixels a pixel brush stroke covers on a frame (decisions.md, pixel mode).
//!
//! Each raw point lands in the pixel under it. Consecutive pixels are joined
//! by Bresenham's line, and a pixel that only turns an L corner between its
//! neighbours is left out, so a one-pixel line has no doubled corners. Each
//! pixel then takes the tip's block of `width` pixels. The wobble moves the
//! line as it moves a line of the same width, sampled at the same spacing,
//! and each move is rounded to whole pixels, so the stroke moves a pixel at
//! a time. Pressure changes neither width nor wobble.
//!
//! The points are not resampled: a stroke drawn pixel by pixel keeps every
//! pixel it passed. The wobble moves each pixel of that still line by the
//! move at its distance along the stroke, and the moved pixels are joined
//! and their corners left out again. Moving the raw points instead let a
//! point land behind the one before, which left clumps of three pixels.

use ugu_core::store::{Point, TipShape};

use crate::stroke::{self, Pen, Resampler, Sample};

/// The block's side in pixels for a stroke of `width`.
pub fn side(width: f32) -> i32 {
    (width.round() as i32).max(1)
}

/// The outline of the pixels a tip of `width` covers around the pixel
/// whose top-left corner is the origin: a closed polygon of pixel corners,
/// down the right side and up the left, for the brush cursor.
pub fn tip_outline(width: f32, tip: TipShape) -> Vec<[i32; 2]> {
    let block = Block::new(side(width), tip);
    let rows: Vec<(i32, i32, i32)> = block
        .rows
        .iter()
        .enumerate()
        .filter(|(_, (from, to))| from < to)
        .map(|(row, &(from, to))| {
            (
                row as i32 - block.centre,
                from - block.centre,
                to - block.centre,
            )
        })
        .collect();
    let mut outline = Vec::with_capacity(rows.len() * 4);
    for &(y, _, to) in &rows {
        outline.extend([[to, y], [to, y + 1]]);
    }
    for &(y, from, _) in rows.iter().rev() {
        outline.extend([[from, y + 1], [from, y]]);
    }
    outline.dedup();
    outline
}

/// Covered pixels as runs within rows, sorted by row then start, apart and
/// not touching.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Cells {
    /// Row, first column, column after the last.
    runs: Vec<[i32; 3]>,
}

impl Cells {
    pub fn runs(&self) -> &[[i32; 3]] {
        &self.runs
    }

    pub fn is_empty(&self) -> bool {
        self.runs.is_empty()
    }

    /// Left, top, right, bottom; `None` when empty.
    pub fn bounds(&self) -> Option<[i32; 4]> {
        let first = self.runs.first()?;
        let last = self.runs.last()?;
        let (left, right) = self
            .runs
            .iter()
            .fold((i32::MAX, i32::MIN), |(left, right), run| {
                (left.min(run[1]), right.max(run[2]))
            });
        Some([left, first[0], right, last[0] + 1])
    }

    pub fn contains(&self, x: i32, y: i32) -> bool {
        let at = self.runs.partition_point(|run| (run[0], run[2]) <= (y, x));
        self.runs
            .get(at)
            .is_some_and(|run| run[0] == y && run[1] <= x)
    }

    fn from_runs(mut runs: Vec<[i32; 3]>) -> Self {
        runs.sort_unstable();
        let mut merged: Vec<[i32; 3]> = Vec::with_capacity(runs.len());
        for run in runs {
            match merged.last_mut() {
                Some(last) if last[0] == run[0] && run[1] <= last[2] => {
                    last[2] = last[2].max(run[2]);
                }
                _ => merged.push(run),
            }
        }
        Self { runs: merged }
    }

    /// The runs in `self` and not in `other`, then those in `other` and
    /// not in `self`.
    pub fn difference(&self, other: &Self) -> (Vec<[i32; 3]>, Vec<[i32; 3]>) {
        (
            minus(&self.runs, &other.runs),
            minus(&other.runs, &self.runs),
        )
    }
}

/// The parts of `a`'s runs outside `b`'s; both sorted and merged.
fn minus(a: &[[i32; 3]], b: &[[i32; 3]]) -> Vec<[i32; 3]> {
    let mut out = Vec::new();
    let mut next = 0;
    for &[y, from, to] in a {
        while next < b.len() && (b[next][0], b[next][2]) <= (y, from) {
            next += 1;
        }
        let mut start = from;
        let mut at = next;
        while start < to && at < b.len() && b[at][0] == y && b[at][1] < to {
            if b[at][1] > start {
                out.push([y, start, b[at][1]]);
            }
            start = start.max(b[at][2]);
            at += 1;
        }
        if start < to {
            out.push([y, start, to]);
        }
    }
    out
}

/// The pixels of a stroke through `points` on `frame`.
pub fn cells(points: &[Point], pen: &Pen, frame: u32) -> Cells {
    if points.is_empty() {
        return Cells::default();
    }
    let moves = Moves::new(points, pen, frame);
    let mut arc = 0.0;
    let mut arcs = Vec::with_capacity(points.len());
    let mut at = Vec::with_capacity(points.len());
    for (index, point) in points.iter().enumerate() {
        if index > 0 {
            let before = &points[index - 1];
            arc += f64::from(point.x - before.x).hypot(f64::from(point.y - before.y));
        }
        arcs.push(arc);
        at.push([
            f64::from(point.x).floor() as i32,
            f64::from(point.y).floor() as i32,
        ]);
    }
    let moved = |cell: [i32; 2], arc: f64| {
        let [dx, dy] = moves.at(arc);
        [cell[0] + dx, cell[1] + dy]
    };
    let block = Block::new(side(pen.width), pen.brush.tip);
    // Moves round to whole pixels and may take a row above the first.
    let top = at.iter().map(|cell| cell[1]).min().unwrap_or(0) - block.centre;
    let mut runs = Gather::new(top);
    let mut path: Vec<[i32; 2]> = Vec::new();
    let mut stamp = |path: &mut Vec<[i32; 2]>| {
        for &cell in path.iter() {
            block.stamp(cell, &mut runs);
        }
        path.clear();
    };
    if points.len() == 1 {
        path.push(moved(at[0], 0.0));
    }
    let mut still = Vec::new();
    for index in 0..points.len().saturating_sub(1) {
        let (from, to) = (arcs[index], arcs[index + 1]);
        if !moves.shows((from + to) / 2.0) {
            stamp(&mut path);
            continue;
        }
        if path.is_empty() {
            path.push(moved(at[index], from));
        }
        still.clear();
        line(at[index], at[index + 1], &mut still);
        for (step, &cell) in still.iter().enumerate() {
            let along = from + (to - from) * (step + 1) as f64 / still.len() as f64;
            join(&mut path, moved(cell, along), moves.wobbles());
        }
    }
    stamp(&mut path);
    Cells::from_runs(runs.runs)
}

/// Runs as blocks are stamped along a path. The blocks of neighbouring
/// pixels overlap, so a run that meets the row's last one lengthens it, and
/// a long stroke of a wide block makes about as many runs as it covers rows
/// and turns rather than one per pixel and row.
struct Gather {
    top: i32,
    /// For each row from `top`, one more than the index of its last run.
    last: Vec<usize>,
    runs: Vec<[i32; 3]>,
}

impl Gather {
    fn new(top: i32) -> Self {
        Self {
            top,
            last: Vec::new(),
            runs: Vec::new(),
        }
    }

    fn add(&mut self, [y, from, to]: [i32; 3]) {
        // Moves round to whole pixels and may take a row above the first.
        if y < self.top {
            let more = (self.top - y) as usize;
            self.last.splice(0..0, std::iter::repeat_n(0, more));
            self.top = y;
        }
        let row = (y - self.top) as usize;
        if row >= self.last.len() {
            self.last.resize(row + 1, 0);
        }
        if let Some(index) = self.last[row].checked_sub(1) {
            let run = &mut self.runs[index];
            if from <= run[2] && to >= run[1] {
                run[1] = run[1].min(from);
                run[2] = run[2].max(to);
                return;
            }
        }
        self.runs.push([y, from, to]);
        self.last[row] = self.runs.len();
    }
}

/// Appends the pixels after `from` to `to` on Bresenham's line, leaving out
/// a pixel that only turns an L corner.
fn line(from: [i32; 2], to: [i32; 2], path: &mut Vec<[i32; 2]>) {
    let (dx, dy) = ((to[0] - from[0]).abs(), -(to[1] - from[1]).abs());
    let (sx, sy) = ((to[0] - from[0]).signum(), (to[1] - from[1]).signum());
    let mut error = dx + dy;
    let [mut x, mut y] = from;
    while [x, y] != to {
        let twice = 2 * error;
        if twice >= dy {
            error += dy;
            x += sx;
        }
        if twice <= dx {
            error += dx;
            y += sy;
        }
        push_perfect(path, [x, y]);
    }
}

/// Adds `cell` to the end of `path`, joined by Bresenham's line where a
/// move leaves a gap. When the line wobbles, a pixel whose neighbours on
/// the path touch each other is left out too: a move that steps aside and
/// back makes such a bump, which the L corner rule does not catch. A still
/// line keeps them, as the hand drew them.
fn join(path: &mut Vec<[i32; 2]>, cell: [i32; 2], wobbles: bool) {
    match path.last() {
        Some(&last) if last == cell => return,
        Some(&last) if apart(last, cell) > 1 => line(last, cell, path),
        _ => push_perfect(path, cell),
    }
    while wobbles
        && let [.., a, _, c] = path[..]
        && apart(a, c) == 1
    {
        let at = path.len() - 2;
        path.remove(at);
    }
}

/// Chebyshev distance.
fn apart(a: [i32; 2], b: [i32; 2]) -> i32 {
    (a[0] - b[0]).abs().max((a[1] - b[1]).abs())
}

fn push_perfect(path: &mut Vec<[i32; 2]>, cell: [i32; 2]) {
    if let [.., a, b] = path[..] {
        let turns = (a[0] == b[0] || a[1] == b[1])
            && (cell[0] == b[0] || cell[1] == b[1])
            && a[0] != cell[0]
            && a[1] != cell[1];
        if turns {
            path.pop();
        }
    }
    path.push(cell);
}

/// The wobble's whole-pixel moves and a broken line's gaps along a stroke,
/// by distance along it.
struct Moves {
    /// Distance along the stroke of each of the line's samples.
    arcs: Vec<f64>,
    /// The move at each sample; empty when nothing moves.
    moved: Vec<[f64; 2]>,
    /// Whether each segment between samples shows; `None` without breaks.
    shown: Option<Vec<bool>>,
}

impl Moves {
    fn new(points: &[Point], pen: &Pen, frame: u32) -> Self {
        // Pressure moves a line's samples a little more or less; not here.
        let even: Vec<Point> = points
            .iter()
            .map(|point| Point {
                pressure: 1.0,
                ..*point
            })
            .collect();
        let samples: Vec<Sample> = Resampler::whole(&even, stroke::spacing(pen.width));
        let positions = stroke::displaced(&samples, pen, frame);
        let moved: Vec<[f64; 2]> = samples
            .iter()
            .zip(&positions)
            .map(|(sample, moved)| [moved[0] - sample.position[0], moved[1] - sample.position[1]])
            .collect();
        let still = moved.iter().all(|moved| *moved == [0.0, 0.0]);
        Self {
            arcs: samples.iter().map(|sample| sample.arc).collect(),
            moved: if still { Vec::new() } else { moved },
            shown: stroke::shown(&samples, pen, frame),
        }
    }

    /// The move at distance `arc`, rounded to whole pixels.
    fn at(&self, arc: f64) -> [i32; 2] {
        if self.moved.is_empty() {
            return [0, 0];
        }
        let after = self.arcs.partition_point(|&at| at <= arc);
        let moved = match after {
            0 => self.moved[0],
            n if n == self.arcs.len() => self.moved[n - 1],
            n => {
                let (a, b) = (self.arcs[n - 1], self.arcs[n]);
                let (from, to) = (self.moved[n - 1], self.moved[n]);
                let t = if b > a { (arc - a) / (b - a) } else { 0.0 };
                [
                    from[0] + (to[0] - from[0]) * t,
                    from[1] + (to[1] - from[1]) * t,
                ]
            }
        };
        moved.map(|value| value.round() as i32)
    }

    /// Whether anything moves.
    fn wobbles(&self) -> bool {
        !self.moved.is_empty()
    }

    /// Whether the line shows at distance `arc`.
    fn shows(&self, arc: f64) -> bool {
        let Some(shown) = &self.shown else {
            return true;
        };
        let segment = self.arcs.partition_point(|&at| at <= arc).saturating_sub(1);
        shown
            .get(segment.min(shown.len().saturating_sub(1)))
            .copied()
            .unwrap_or(true)
    }
}

/// The tip's pixels around a point's pixel: a square of `side`, or for a
/// round tip the pixels whose centres are inside its circle, with the
/// corners of small circles cut as pixel art draws them.
struct Block {
    /// Per row from the top: first and after-last column, from the left.
    rows: Vec<(i32, i32)>,
    /// The point's pixel's place in the block.
    centre: i32,
}

impl Block {
    fn new(side: i32, tip: TipShape) -> Self {
        let rows = (0..side)
            .map(|row| match tip {
                TipShape::Square => (0, side),
                _ if side <= 2 => (0, side),
                TipShape::Round => {
                    let radius = f64::from(side) / 2.0;
                    let y = f64::from(row) + 0.5 - radius;
                    let limit = radius * radius - 0.5 - y * y;
                    let inside = (0..side)
                        .filter(|&column| {
                            let x = f64::from(column) + 0.5 - radius;
                            x * x < limit
                        })
                        .collect::<Vec<_>>();
                    match (inside.first(), inside.last()) {
                        (Some(&first), Some(&last)) => (first, last + 1),
                        _ => (0, 0),
                    }
                }
            })
            .collect();
        Self {
            rows,
            centre: (side - 1) / 2,
        }
    }

    fn stamp(&self, [x, y]: [i32; 2], runs: &mut Gather) {
        let (left, top) = (x - self.centre, y - self.centre);
        for (row, &(from, to)) in self.rows.iter().enumerate() {
            if from < to {
                runs.add([top + row as i32, left + from, left + to]);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use std::sync::Arc;
    use ugu_core::brush;
    use ugu_core::ops::{Rgba8, Wobble};
    use ugu_core::store::Stroke;

    fn stroke(points: &[[f32; 2]], width: f32) -> Stroke {
        Stroke {
            points: Arc::from(
                points
                    .iter()
                    .map(|&[x, y]| Point {
                        x,
                        y,
                        pressure: 0.4,
                    })
                    .collect::<Vec<_>>(),
            ),
            color: Rgba8([0, 0, 0, 255]),
            width,
            brush: brush::find("pixel-pencil").unwrap().brush,
            seed: 7,
        }
    }

    fn pixels(stroke: &Stroke, wobble: f32, frame: u32) -> HashSet<[i32; 2]> {
        let pen = Pen::new(stroke, Wobble::classic(wobble), 8);
        let cells = cells(&stroke.points, &pen, frame);
        let mut out = HashSet::new();
        for &[y, from, to] in cells.runs() {
            for x in from..to {
                out.insert([x, y]);
            }
        }
        out
    }

    /// Every pixel touches at most two others, and the pixels are joined:
    /// a one-pixel line without gaps or doubled corners.
    fn assert_thin_line(pixels: &HashSet<[i32; 2]>) {
        let neighbours = |[x, y]: [i32; 2]| {
            (-1..=1)
                .flat_map(move |dy| (-1..=1).map(move |dx| [x + dx, y + dy]))
                .filter(move |&other| other != [x, y])
        };
        for &pixel in pixels {
            let touching = neighbours(pixel)
                .filter(|other| pixels.contains(other))
                .count();
            assert!(touching <= 2, "{pixel:?} touches {touching}");
        }
        let start = *pixels.iter().next().unwrap();
        let mut reached = HashSet::from([start]);
        let mut open = vec![start];
        while let Some(pixel) = open.pop() {
            for other in neighbours(pixel) {
                if pixels.contains(&other) && reached.insert(other) {
                    open.push(other);
                }
            }
        }
        assert_eq!(reached.len(), pixels.len(), "the line has a gap");
    }

    #[test]
    fn straight_lines_take_one_pixel_a_step() {
        let across = pixels(&stroke(&[[2.5, 3.5], [12.9, 3.2]], 1.0), 0.0, 0);
        assert_eq!(across, (2..=12).map(|x| [x, 3]).collect());
        let down = pixels(&stroke(&[[4.1, 1.0], [4.7, 9.9]], 1.0), 0.0, 0);
        assert_eq!(down, (1..=9).map(|y| [4, y]).collect());
        let diagonal = pixels(&stroke(&[[0.5, 0.5], [7.5, 7.5]], 1.0), 0.0, 0);
        assert_eq!(diagonal, (0..=7).map(|i| [i, i]).collect());
        let shallow = pixels(&stroke(&[[0.5, 0.5], [20.5, 6.5]], 1.0), 0.0, 0);
        assert_eq!(shallow.len(), 21);
        assert_thin_line(&shallow);
    }

    #[test]
    fn a_curve_drawn_slowly_has_no_doubled_corners() {
        // A quarter circle given every half pixel, as a pointer moving slowly.
        let points: Vec<[f32; 2]> = (0..=120)
            .map(|step| {
                let angle = f32::from(step as u8) / 120.0 * std::f32::consts::FRAC_PI_2;
                [3.0 + 30.0 * angle.cos(), 3.0 + 30.0 * angle.sin()]
            })
            .collect();
        assert_thin_line(&pixels(&stroke(&points, 1.0), 0.0, 0));
    }

    #[test]
    fn a_click_is_exactly_the_tip() {
        assert_eq!(
            pixels(&stroke(&[[5.2, 6.8]], 1.0), 0.0, 0),
            HashSet::from([[5, 6]])
        );
        let three = pixels(&stroke(&[[5.2, 6.8]], 3.0), 0.0, 0);
        assert_eq!(
            three,
            (5..8)
                .flat_map(|y| (4..7).map(move |x| [x, y]))
                .collect::<HashSet<_>>()
        );
        assert_eq!(pixels(&stroke(&[[5.2, 6.8]], 4.4), 0.0, 0).len(), 16);
        let mut round = stroke(&[[5.2, 6.8]], 4.0);
        round.brush.tip = TipShape::Round;
        // A four-pixel circle has its corners cut.
        assert_eq!(pixels(&round, 0.0, 0).len(), 12);
        round.width = 3.0;
        assert_eq!(pixels(&round, 0.0, 0).len(), 5);
    }

    #[test]
    fn the_wobble_moves_pixels_by_whole_pixels_and_keeps_the_line_thin() {
        let points: Vec<[f32; 2]> = (0..=60).map(|x| [x as f32 * 0.5 + 0.5, 10.5]).collect();
        let line = stroke(&points, 1.0);
        let still = pixels(&line, 0.0, 0);
        let frames: Vec<_> = (0..8).map(|frame| pixels(&line, 2.0, frame)).collect();
        assert!(frames.iter().any(|frame| *frame != still), "it moves");
        assert!(frames.windows(2).any(|pair| pair[0] != pair[1]));
        for frame in &frames {
            assert_thin_line(frame);
        }
    }

    #[test]
    fn the_tip_outline_goes_round_the_pixels_a_click_covers() {
        assert_eq!(
            tip_outline(1.0, TipShape::Round),
            [[1, 0], [1, 1], [0, 1], [0, 0]]
        );
        assert_eq!(
            tip_outline(3.0, TipShape::Square),
            [
                [2, -1],
                [2, 0],
                [2, 1],
                [2, 2],
                [-1, 2],
                [-1, 1],
                [-1, 0],
                [-1, -1]
            ]
        );
        // Every pixel a click covers is inside the outline, and no other.
        for (width, tip) in [
            (5.0, TipShape::Round),
            (4.0, TipShape::Square),
            (7.0, TipShape::Round),
        ] {
            let outline = tip_outline(width, tip);
            let inside = |x: i32, y: i32| {
                let (px, py) = (f64::from(x) + 0.5, f64::from(y) + 0.5);
                let mut crossings = 0;
                for (index, a) in outline.iter().enumerate() {
                    let b = outline[(index + 1) % outline.len()];
                    let (ay, by) = (f64::from(a[1]), f64::from(b[1]));
                    if (ay > py) != (by > py)
                        && px < f64::from(a[0]) + (py - ay) / (by - ay) * f64::from(b[0] - a[0])
                    {
                        crossings += 1;
                    }
                }
                crossings % 2 == 1
            };
            let mut click = stroke(&[[0.5, 0.5]], width);
            click.brush.tip = tip;
            let covered = pixels(&click, 0.0, 0);
            for y in -6..7 {
                for x in -6..7 {
                    assert_eq!(
                        inside(x, y),
                        covered.contains(&[x, y]),
                        "{width} {tip:?} {x},{y}"
                    );
                }
            }
        }
    }

    #[test]
    fn wobbling_curves_stay_thin_and_joined() {
        // Half circles, which the old way of moving the raw points left with
        // clumps of three pixels on some frames.
        for radius in [6.0, 12.0, 25.0] {
            let points: Vec<[f32; 2]> = (0..=80)
                .map(|step| {
                    let turn = step as f32 / 80.0 * std::f32::consts::PI;
                    [40.3 + radius * turn.cos(), 40.7 + radius * turn.sin()]
                })
                .collect();
            let line = stroke(&points, 1.0);
            assert_thin_line(&pixels(&line, 0.0, 0));
            for wobble in [1.0, 1.6, 3.0] {
                for frame in 0..8 {
                    assert_thin_line(&pixels(&line, wobble, frame));
                }
            }
        }
    }

    #[test]
    fn a_broken_line_leaves_gaps_on_some_frames() {
        let points: Vec<[f32; 2]> = (0..=200).map(|x| [x as f32 * 0.5 + 0.5, 10.5]).collect();
        let line = stroke(&points, 1.0);
        let mut wobble = Wobble::classic(1.0);
        wobble.motion.broken = true;
        let pen = Pen::new(&line, wobble, 8);
        let whole = cells(&line.points, &Pen::new(&line, Wobble::classic(1.0), 8), 0);
        let broken: Vec<usize> = (0..8)
            .map(|frame| cells(&line.points, &pen, frame).runs().len())
            .collect();
        assert!(!whole.is_empty());
        assert!(broken.iter().any(|&runs| runs > 1), "{broken:?}");
    }

    #[test]
    fn differences_are_exact() {
        let a = Cells::from_runs(vec![[0, 0, 10], [1, 2, 4], [3, 0, 2]]);
        let b = Cells::from_runs(vec![[0, 3, 5], [0, 8, 12], [2, 0, 1], [3, 0, 2]]);
        let (only_a, only_b) = a.difference(&b);
        assert_eq!(only_a, [[0, 0, 3], [0, 5, 8], [1, 2, 4]]);
        assert_eq!(only_b, [[0, 10, 12], [2, 0, 1]]);
        assert!(a.contains(9, 0) && !a.contains(10, 0) && a.contains(3, 1));
        assert_eq!(a.bounds(), Some([0, 0, 10, 4]));
    }
}

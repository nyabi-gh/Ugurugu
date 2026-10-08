// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! Selection masks and fill coverage as runs of whole pixels per row, and as
//! paths of those pixels' squares. Drawn as a clip, such a path covers each
//! pixel fully or not at all at full size, so a mask cuts exactly as its
//! bits do; drawn smaller, edge pixels take the share of the mask they hold.

use ugu_core::store::Mask;
use vello_cpu::kurbo::BezPath;

/// Pixels per row, as runs from a left edge to a right edge (exclusive),
/// sorted and apart from each other.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Runs {
    /// The first row.
    top: i32,
    rows: Vec<Vec<[i32; 2]>>,
}

impl Runs {
    pub fn from_mask(mask: &Mask) -> Self {
        let [left, top, width, height] = mask.bounds;
        let row_bytes = Mask::row_bytes(width);
        let rows = (0..height.max(0) as usize)
            .map(|row| {
                let bits = &mask.bits[row * row_bytes..(row + 1) * row_bytes];
                let mut runs = Vec::new();
                let mut start = None;
                for column in 0..width {
                    let set = bits[column as usize / 8] & (0x80 >> (column % 8)) != 0;
                    match (set, start) {
                        (true, None) => start = Some(column),
                        (false, Some(from)) => {
                            runs.push([left + from, left + column]);
                            start = None;
                        }
                        _ => {}
                    }
                }
                if let Some(from) = start {
                    runs.push([left + from, left + width]);
                }
                runs
            })
            .collect();
        Self { top, rows }
    }

    /// The runs of row `y`.
    pub fn row(&self, y: i32) -> &[[i32; 2]] {
        usize::try_from(y - self.top)
            .ok()
            .and_then(|index| self.rows.get(index))
            .map_or(&[], Vec::as_slice)
    }

    fn span(&self) -> std::ops::Range<i32> {
        self.top..self.top + self.rows.len() as i32
    }

    fn from_rows(top: i32, bottom: i32, mut row: impl FnMut(i32) -> Vec<[i32; 2]>) -> Self {
        Self {
            top,
            rows: (top..bottom.max(top)).map(&mut row).collect(),
        }
    }

    /// The pixels in both.
    pub fn intersect(&self, other: &Self) -> Self {
        let (a, b) = (self.span(), other.span());
        Self::from_rows(a.start.max(b.start), a.end.min(b.end), |y| {
            intersect(self.row(y), other.row(y))
        })
    }

    /// The pixels outside these whose left, right, upper or lower
    /// neighbour is inside.
    pub fn fringe(&self) -> Self {
        let span = self.span();
        Self::from_rows(span.start - 1, span.end + 1, |y| {
            let widened: Vec<[i32; 2]> = self
                .row(y)
                .iter()
                .map(|&[left, right]| [left - 1, right + 1])
                .collect();
            let around = union(&union(&widened, self.row(y - 1)), self.row(y + 1));
            subtract(&around, self.row(y))
        })
    }

    /// What a fill of `coverage` cut to `clip` draws: the pixels it covers,
    /// and with `antialias` the fringe around them, laid behind what is
    /// there (2.2.13's edge rule, ADR section 2); `None` for no fringe.
    pub fn fill(coverage: &Mask, clip: Option<&Mask>, antialias: bool) -> (Self, Option<Self>) {
        let covered = Self::from_mask(coverage);
        let clip = clip.map(Self::from_mask);
        let cut = |runs: Self| match &clip {
            Some(clip) => runs.intersect(clip),
            None => runs,
        };
        let fringe = antialias
            .then(|| cut(covered.fringe()))
            .filter(|fringe| !fringe.is_empty());
        (cut(covered), fringe)
    }

    pub fn is_empty(&self) -> bool {
        self.rows.iter().all(Vec::is_empty)
    }

    /// Left, top, right, bottom of the pixels; `None` without any.
    pub fn bounds(&self) -> Option<[i32; 4]> {
        let mut bounds: Option<[i32; 4]> = None;
        for (index, runs) in self.rows.iter().enumerate() {
            let (Some(first), Some(last)) = (runs.first(), runs.last()) else {
                continue;
            };
            let y = self.top + index as i32;
            bounds = Some(
                bounds.map_or([first[0], y, last[1], y + 1], |[l, t, r, _]| {
                    [l.min(first[0]), t, r.max(last[1]), y + 1]
                }),
            );
        }
        bounds
    }

    /// The squares of the pixels, one rectangle per run, all wound the same
    /// way and apart, for the non-zero rule.
    pub fn path(&self) -> BezPath {
        let mut path = BezPath::new();
        for (index, runs) in self.rows.iter().enumerate() {
            let y = f64::from(self.top + index as i32);
            for &[left, right] in runs {
                let (left, right) = (f64::from(left), f64::from(right));
                path.move_to((left, y));
                path.line_to((right, y));
                path.line_to((right, y + 1.0));
                path.line_to((left, y + 1.0));
                path.close_path();
            }
        }
        path
    }
}

fn intersect(a: &[[i32; 2]], b: &[[i32; 2]]) -> Vec<[i32; 2]> {
    let (mut i, mut j, mut out) = (0, 0, Vec::new());
    while i < a.len() && j < b.len() {
        let (left, right) = (a[i][0].max(b[j][0]), a[i][1].min(b[j][1]));
        if left < right {
            out.push([left, right]);
        }
        if a[i][1] < b[j][1] {
            i += 1;
        } else {
            j += 1;
        }
    }
    out
}

fn union(a: &[[i32; 2]], b: &[[i32; 2]]) -> Vec<[i32; 2]> {
    let mut all: Vec<[i32; 2]> = a.iter().chain(b).copied().collect();
    all.sort_unstable();
    let mut out: Vec<[i32; 2]> = Vec::with_capacity(all.len());
    for run in all {
        match out.last_mut() {
            Some(last) if run[0] <= last[1] => last[1] = last[1].max(run[1]),
            _ => out.push(run),
        }
    }
    out
}

fn subtract(a: &[[i32; 2]], b: &[[i32; 2]]) -> Vec<[i32; 2]> {
    let mut out = Vec::new();
    for &[mut left, right] in a {
        for &[cut_left, cut_right] in b {
            if cut_right <= left || cut_left >= right {
                continue;
            }
            if cut_left > left {
                out.push([left, cut_left]);
            }
            left = left.max(cut_right);
        }
        if left < right {
            out.push([left, right]);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    /// A mask of the pixels `inside` picks within `bounds`.
    fn mask(bounds: [i32; 4], inside: impl Fn(i32, i32) -> bool) -> Mask {
        let [left, top, width, height] = bounds;
        let row_bytes = Mask::row_bytes(width);
        let mut bits = vec![0u8; row_bytes * height as usize];
        for row in 0..height {
            for column in 0..width {
                if inside(left + column, top + row) {
                    bits[row as usize * row_bytes + column as usize / 8] |= 0x80 >> (column % 8);
                }
            }
        }
        Mask {
            bounds,
            bits: Arc::from(bits),
        }
    }

    fn contains(runs: &Runs, x: i32, y: i32) -> bool {
        runs.row(y)
            .iter()
            .any(|&[left, right]| (left..right).contains(&x))
    }

    fn disc(x: i32, y: i32) -> bool {
        (x - 20) * (x - 20) + (y - 15) * (y - 15) < 120 || (x + y) % 7 == 0
    }

    fn ring(x: i32, y: i32) -> bool {
        let d = (x - 26) * (x - 26) + (y - 18) * (y - 18);
        (40..200).contains(&d)
    }

    #[test]
    fn runs_hold_the_mask_bits() {
        let mask = mask([3, 2, 37, 29], disc);
        let runs = Runs::from_mask(&mask);
        for y in -2..40 {
            for x in -2..50 {
                assert_eq!(contains(&runs, x, y), mask.contains(x, y), "at {x}, {y}");
            }
        }
    }

    #[test]
    fn intersections_and_fringes_are_pixel_exact() {
        let a = Runs::from_mask(&mask([3, 2, 37, 29], disc));
        let b = Runs::from_mask(&mask([10, 5, 40, 30], ring));
        let both = a.intersect(&b);
        let fringe = a.fringe();
        for y in -3..42 {
            for x in -3..55 {
                let (in_a, in_b) = (contains(&a, x, y), contains(&b, x, y));
                assert_eq!(contains(&both, x, y), in_a && in_b, "both at {x}, {y}");
                let next = contains(&a, x - 1, y)
                    || contains(&a, x + 1, y)
                    || contains(&a, x, y - 1)
                    || contains(&a, x, y + 1);
                assert_eq!(contains(&fringe, x, y), !in_a && next, "fringe at {x}, {y}");
            }
        }
    }
}

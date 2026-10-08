// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! Placed text as drawing, as 2.2.13 makes it: every contour of the letters
//! is a closed stroke in the brush colour and width, so the letters wobble
//! like drawn lines, and filled letters add a fill under them that holds
//! still. Holes in letters stay open by the non-zero rule.

use std::sync::Arc;

use crate::selection::Selection;
use crate::store::{Point, Stroke};

/// Laid-out text with its top left at the origin, in document pixels.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Outline {
    /// Closed polygons, the last point not repeating the first. Holes run
    /// the other way round from what holds them.
    pub contours: Vec<Vec<[f64; 2]>>,
    /// Width and height of the laid-out lines.
    pub size: [f64; 2],
}

/// How far a closed stroke runs on past its start, so that the seam
/// between its ends does not show once the ends wobble apart.
fn overlap(width: f32) -> f64 {
    f64::from(width * 0.55).clamp(2.0, 5.0)
}

/// The seed of the `index`th stroke of text placed with `base`: splitmix64,
/// as 2.2.13 derives them.
pub fn derived_seed(base: u64, index: u64) -> u64 {
    let mut value = base.wrapping_add((index + 1).wrapping_mul(0x9e37_79b9_7f4a_7c15));
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^ (value >> 31)
}

/// One stroke per contour, moved by `at`: `template`'s colour, width and
/// brush, a seed derived from `base_seed`. Contours of fewer than three
/// distinct points are left out.
pub fn strokes(
    contours: &[Vec<[f64; 2]>],
    at: [f64; 2],
    template: &Stroke,
    base_seed: u64,
) -> Vec<Stroke> {
    let reach = overlap(template.width);
    contours
        .iter()
        .filter_map(|contour| {
            let mut ring: Vec<[f64; 2]> = Vec::with_capacity(contour.len() + 8);
            for &[x, y] in contour {
                let point = [x + at[0], y + at[1]];
                if ring
                    .last()
                    .is_none_or(|last| distance(*last, point) >= 0.01)
                {
                    ring.push(point);
                }
            }
            if ring.len() < 3 {
                return None;
            }
            let first = ring[0];
            if distance(*ring.last().expect("three points"), first) >= 0.01 {
                ring.push(first);
            }
            // Run on along the start until `reach` is covered again.
            let closed = ring.len();
            let mut walked = 0.0;
            let mut index = 1;
            while walked < reach && index < closed {
                walked += distance(ring[index - 1], ring[index]);
                ring.push(ring[index]);
                index += 1;
            }
            Some(ring)
        })
        .enumerate()
        .map(|(index, ring)| Stroke {
            points: ring
                .iter()
                .map(|&[x, y]| Point {
                    x: x as f32,
                    y: y as f32,
                    pressure: 1.0,
                })
                .collect::<Arc<[Point]>>(),
            seed: derived_seed(base_seed, index as u64 + 1),
            ..template.clone()
        })
        .collect()
}

fn distance(a: [f64; 2], b: [f64; 2]) -> f64 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

/// The canvas pixels whose centres the contours, moved by `at`, enclose by
/// the non-zero rule: what filled letters cover. `None` when none.
pub fn coverage(contours: &[Vec<[f64; 2]>], at: [f64; 2], canvas: [u32; 2]) -> Option<Selection> {
    let moved: Vec<Vec<[f64; 2]>> = contours
        .iter()
        .map(|contour| {
            contour
                .iter()
                .map(|&[x, y]| [x + at[0], y + at[1]])
                .collect()
        })
        .collect();
    Selection::of_contours(&moved, canvas)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ops::Rgba8;
    use crate::store::Brush;

    fn template() -> Stroke {
        Stroke {
            points: Arc::from([]),
            color: Rgba8([200, 40, 40, 255]),
            width: 4.0,
            brush: Brush::default(),
            seed: 0,
        }
    }

    /// A 20 × 10 box with a 10 × 4 hole running the other way round.
    fn boxed() -> Vec<Vec<[f64; 2]>> {
        vec![
            vec![[0.0, 0.0], [20.0, 0.0], [20.0, 10.0], [0.0, 10.0]],
            vec![[5.0, 3.0], [5.0, 7.0], [15.0, 7.0], [15.0, 3.0]],
        ]
    }

    #[test]
    fn each_contour_is_a_closed_stroke_running_on_past_its_start() {
        let strokes = strokes(&boxed(), [30.0, 40.0], &template(), 7);
        assert_eq!(strokes.len(), 2);
        for stroke in &strokes {
            assert_eq!((stroke.color, stroke.width), (template().color, 4.0));
            let points: Vec<[f32; 2]> = stroke.points.iter().map(|p| [p.x, p.y]).collect();
            // Back to the start, then on along the first side for 2.2 px:
            // the whole first side.
            assert_eq!(points[4], points[0]);
            assert_eq!(points[5], points[1]);
            assert_eq!(points.len(), 6);
        }
        assert_eq!(strokes[0].points[0].x, 30.0);
        assert_eq!(strokes[1].points[0].y, 43.0);
        assert_ne!(strokes[0].seed, strokes[1].seed);
        assert_eq!(
            super::strokes(&boxed(), [30.0, 40.0], &template(), 7),
            strokes
        );
        assert_ne!(
            super::strokes(&boxed(), [30.0, 40.0], &template(), 8),
            strokes
        );
    }

    #[test]
    fn degenerate_contours_are_left_out() {
        let contours = vec![
            vec![[0.0, 0.0], [0.001, 0.0], [0.0, 0.002]],
            vec![[0.0, 0.0], [5.0, 0.0]],
        ];
        assert!(strokes(&contours, [0.0, 0.0], &template(), 1).is_empty());
    }

    #[test]
    fn the_fill_covers_the_letters_but_not_their_holes() {
        let filled = coverage(&boxed(), [2.0, 1.0], [40, 20]).unwrap();
        assert_eq!(filled.mask().bounds, [2, 1, 20, 10]);
        assert!(filled.mask().contains(3, 2));
        assert!(!filled.mask().contains(10, 6), "the hole");
        assert!(filled.mask().contains(10, 9));
        // Off the canvas, nothing.
        assert_eq!(coverage(&boxed(), [50.0, 0.0], [40, 20]), None);
    }
}

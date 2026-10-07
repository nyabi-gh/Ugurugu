// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! The coverage of a stroke while it is drawn, kept up to date a few samples
//! at a time on the calling thread.
//!
//! A sample's position on the frame depends on its neighbours, so the last
//! samples move as points arrive. Pieces of samples that can no longer move
//! are added to `coverage` once; the rest, the tail, are drawn again on every
//! update over a copy of `coverage` and replace the previous tail.

use ugu_core::store::Point;
use vello_cpu::Pixmap;
use vello_cpu::color::{AlphaColor, Srgb};

use crate::compose::{Stamp, clamp, erase, paint};
use crate::raster::PixelRect;
use crate::stroke::{self, Pen, Resampler};

/// The coverage over which an aliased pen paints a pixel, as the renderer
/// sets it.
const ALIASING_THRESHOLD: u8 = 128;

pub struct LiveStroke {
    pen: Pen,
    frame: u32,
    /// Premultiplied colour at full coverage.
    color: [u8; 4],
    erase: bool,
    resampler: Resampler,
    /// Raw points taken so far.
    points: usize,
    /// Samples whose pieces are in `coverage`.
    settled: usize,
    /// Alpha is the settled pieces' coverage; document size.
    coverage: Pixmap,
    /// The tail over the settled coverage, and where it lies.
    tail: Option<(PixelRect, Pixmap)>,
    stamp: Stamp,
}

impl LiveStroke {
    /// `color` is premultiplied, as the stroke paints or erases with it.
    pub fn new(size: [u16; 2], pen: Pen, frame: u32, color: [u8; 4], erase: bool) -> Self {
        Self {
            pen,
            frame,
            color,
            erase,
            resampler: Resampler::new(stroke::spacing(pen.width)),
            points: 0,
            settled: 0,
            coverage: Pixmap::new(size[0], size[1]),
            tail: None,
            stamp: Stamp::default(),
        }
    }

    /// Takes the points after those seen before and returns the pixels whose
    /// coverage changed.
    pub fn update(&mut self, points: &[Point]) -> Option<PixelRect> {
        for point in &points[self.points.min(points.len())..] {
            self.resampler.push(*point);
        }
        self.points = points.len();
        let samples = self.resampler.samples_with_tail();
        if samples.is_empty() {
            return None;
        }
        // A sample is fixed once the one after it is a regular sample.
        let fixed = self.resampler.settled().saturating_sub(1);
        let mut dirty = None;

        if fixed > self.settled {
            let pieces = stroke::pieces(
                &samples,
                &self.pen,
                self.frame,
                self.settled..fixed,
                self.settled.saturating_sub(1)..fixed.saturating_sub(1),
            );
            if let Some(rect) = clamp(pieces.bounds, self.size()) {
                let piece = self.draw(rect, &pieces.path);
                add_coverage(&mut self.coverage, &piece, rect);
                dirty = Some(rect);
            }
            self.settled = fixed;
        }

        let old_tail = self.tail.take().map(|(rect, _)| rect);
        let pieces = stroke::pieces(
            &samples,
            &self.pen,
            self.frame,
            self.settled..samples.len(),
            self.settled.saturating_sub(1)..samples.len() - 1,
        );
        if let Some(rect) = clamp(pieces.bounds, self.size()) {
            let piece = self.draw(rect, &pieces.path);
            let mut tail = copy(&self.coverage, rect);
            add_coverage(
                &mut tail,
                &piece,
                [0, 0, rect[2] - rect[0], rect[3] - rect[1]],
            );
            self.tail = Some((rect, tail));
            dirty = union(dirty, Some(rect));
        }
        union(dirty, old_tail)
    }

    /// Antialiased coverage even for an aliased pen: thresholding each
    /// batch alone would drop pixels that two batches share.
    fn draw(&mut self, rect: PixelRect, path: &vello_cpu::kurbo::BezPath) -> Pixmap {
        self.stamp.draw(rect, |context| {
            context.set_paint(AlphaColor::<Srgb>::WHITE);
            context.fill_path(path);
        })
    }

    fn cover(&self, x: usize, y: usize) -> u8 {
        let mut cover =
            self.coverage.data_as_u8_slice()[(y * usize::from(self.coverage.width()) + x) * 4 + 3];
        if let Some(([left, top, right, bottom], tail)) = &self.tail {
            let (x32, y32) = (x as u32, y as u32);
            if (*left..*right).contains(&x32) && (*top..*bottom).contains(&y32) {
                let at = ((y32 - top) * u32::from(tail.width()) + x32 - left) as usize * 4 + 3;
                cover = tail.data_as_u8_slice()[at];
            }
        }
        match self.pen.brush.antialias {
            true => cover,
            // Vello paints an aliased pixel whose coverage is over the
            // threshold.
            false if cover > ALIASING_THRESHOLD => 255,
            false => 0,
        }
    }

    /// Draws the stroke on a layer pixel at `x`, `y`.
    fn size(&self) -> [u32; 2] {
        [self.coverage.width(), self.coverage.height()].map(u32::from)
    }

    pub fn apply(&self, x: usize, y: usize, layer: &mut [u8; 4]) {
        let cover = self.cover(x, y);
        if cover == 0 {
            return;
        }
        if self.erase {
            erase(layer, self.color, cover);
        } else {
            paint(layer, self.color, cover);
        }
    }
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

fn copy(pixmap: &Pixmap, [left, top, right, bottom]: PixelRect) -> Pixmap {
    let (width, height) = ((right - left) as u16, (bottom - top) as u16);
    let mut out = Pixmap::new(width, height);
    let row = usize::from(width) * 4;
    let source_row = usize::from(pixmap.width()) * 4;
    let source = pixmap.data_as_u8_slice();
    for (y, target) in out.data_as_u8_slice_mut().chunks_exact_mut(row).enumerate() {
        let start = (top as usize + y) * source_row + left as usize * 4;
        target.copy_from_slice(&source[start..start + row]);
    }
    out
}

/// Adds the coverage in `piece`'s alpha to `target`'s within `rect`. Vello
/// adds the coverage of every piece of one non-zero fill and clamps it, so
/// pieces drawn in several batches add the same way; uniting them as
/// separate layers would leave a lighter seam where two batches share a
/// pixel.
fn add_coverage(target: &mut Pixmap, piece: &Pixmap, [left, top, right, bottom]: PixelRect) {
    let width = usize::from(target.width());
    let piece_width = usize::from(piece.width());
    let target = target.data_as_u8_slice_mut();
    let piece = piece.data_as_u8_slice();
    for y in top as usize..bottom as usize {
        for x in left as usize..right as usize {
            let cover = piece[((y - top as usize) * piece_width + x - left as usize) * 4 + 3];
            if cover != 0 {
                let at = (y * width + x) * 4 + 3;
                target[at] = target[at].saturating_add(cover);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compose::premultiplied;
    use crate::stroke::Resampler;
    use ugu_core::store::{Brush, BrushEngine};

    fn pen(antialias: bool) -> Pen {
        Pen {
            width: 10.0,
            brush: Brush {
                engine: BrushEngine::Line,
                opacity: 1.0,
                hardness: 1.0,
                antialias,
                size_dynamics: 0.8,
                wobble_scale: 1.0,
            },
            seed: 77,
            wobble: 3.0,
        }
    }

    fn points() -> Vec<Point> {
        (0..120)
            .map(|index| {
                let t = index as f32 / 119.0;
                Point {
                    x: 10.0 + 180.0 * t,
                    y: 60.0 + (t * 12.0).sin() * 35.0,
                    pressure: 0.3 + 0.7 * (t * 5.0).sin().abs(),
                }
            })
            .collect()
    }

    /// The alpha of the whole outline filled at once.
    /// The antialiased coverage of the whole outline filled at once.
    fn whole(pen: &Pen, points: &[Point]) -> Vec<u8> {
        let samples = Resampler::whole(points, stroke::spacing(pen.width));
        let path = stroke::outline(&samples, pen, 2).unwrap();
        let piece = Stamp::default().draw([0, 0, 200, 120], |context| {
            context.set_paint(AlphaColor::<Srgb>::WHITE);
            context.fill_path(&path);
        });
        piece
            .data_as_u8_slice()
            .chunks(4)
            .map(|pixel| pixel[3])
            .collect()
    }

    fn live_coverage(live: &LiveStroke) -> Vec<u8> {
        (0..120)
            .flat_map(|y| (0..200).map(move |x| (x, y)))
            .map(|(x, y)| live.cover(x, y))
            .collect()
    }

    #[test]
    fn drawn_point_by_point_it_covers_what_the_whole_stroke_covers() {
        for antialias in [true, false] {
            let pen = pen(antialias);
            let points = points();
            let mut live = LiveStroke::new([200, 120], pen, 2, [0, 0, 0, 255], false);
            let mut seen: Option<PixelRect> = None;
            for count in 1..=points.len() {
                seen = union(seen, live.update(&points[..count]));
            }
            let covered = live_coverage(&live);
            let expected = whole(&pen, &points);
            for (index, (&a, &b)) in covered.iter().zip(&expected).enumerate() {
                let at = (index % 200, index / 200);
                if antialias {
                    // Batches add coverage as one fill does, up to rounding.
                    assert!(a.abs_diff(b) <= 4, "at {at:?}: {a} against {b}");
                } else if b.abs_diff(ALIASING_THRESHOLD) > 8 {
                    let painted = if b > ALIASING_THRESHOLD { 255 } else { 0 };
                    assert_eq!(a, painted, "at {at:?}, whole coverage {b}");
                } else {
                    assert!(a == 0 || a == 255);
                }
            }
            // Every pixel that changed was reported.
            let [left, top, right, bottom] = seen.unwrap();
            for (index, &a) in covered.iter().enumerate() {
                let (x, y) = ((index % 200) as u32, (index / 200) as u32);
                if a > 0 {
                    assert!(x >= left && x < right && y >= top && y < bottom);
                }
            }
        }
    }

    #[test]
    fn the_tail_is_replaced_not_added_to() {
        let pen = pen(true);
        let points = points();
        let mut stepwise = LiveStroke::new([200, 120], pen, 2, [0, 0, 0, 255], false);
        for count in 1..=points.len() {
            stepwise.update(&points[..count]);
        }
        let mut at_once = LiveStroke::new([200, 120], pen, 2, [0, 0, 0, 255], false);
        at_once.update(&points);
        let most = live_coverage(&stepwise)
            .iter()
            .zip(live_coverage(&at_once))
            .map(|(a, b)| a.abs_diff(b))
            .max()
            .unwrap();
        // An old tail left behind would differ by far more than rounding.
        assert!(most <= 4, "differs by {most}");
    }

    #[test]
    fn a_translucent_live_stroke_does_not_darken_where_it_overlaps() {
        let pen = pen(true);
        let color = premultiplied([200, 0, 0, 100]);
        let mut live = LiveStroke::new([200, 120], pen, 2, color, false);
        let points = points();
        for count in 1..=points.len() {
            live.update(&points[..count]);
        }
        let mut darkest = 0;
        for y in 0..120 {
            for x in 0..200 {
                let mut layer = [0; 4];
                live.apply(x, y, &mut layer);
                darkest = darkest.max(layer[3]);
            }
        }
        assert_eq!(darkest, color[3]);
    }
}

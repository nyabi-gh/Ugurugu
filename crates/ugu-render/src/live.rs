// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! The coverage of a stroke while it is drawn, kept up to date a few samples
//! at a time on the calling thread.
//!
//! A sample's position on the frame depends on its neighbours, so the last
//! samples move as points arrive. Pieces of samples that can no longer move
//! are added to `coverage` once; the rest, the tail, are drawn again on every
//! update over a copy of `coverage` and replace the previous tail.
//!
//! An airbrush or spray keeps its painted dabs instead of a coverage, as
//! the renderer paints them, and lays them on the layer as it does.
//!
//! A broken line's shown segments are measured along the moved samples, so
//! they too are walked once up to the samples that no longer move.

use ugu_core::motion::{Breaks, Walk};
use ugu_core::store::{BrushEngine, Mask, Point};
use vello_cpu::Pixmap;
use vello_cpu::color::{AlphaColor, Srgb};

use crate::compose::{Stamp, clamp, dest_out, erase, inside, paint, premultiplied, src_over};
use crate::dab::{self, Dab, Look};
use crate::pixel;
use crate::raster::PixelRect;
use crate::stroke::{self, Pen, Resampler};

/// The coverage over which an aliased pen paints a pixel, as the renderer
/// sets it.
const ALIASING_THRESHOLD: u8 = 128;

pub struct LiveStroke {
    pen: Pen,
    frame: u32,
    /// The stroke's own colour, straight; black for an eraser's dabs.
    color: [u8; 4],
    /// A pen's premultiplied colour at full coverage, known from the first
    /// point.
    line: [u8; 4],
    erase: bool,
    /// How an airbrush or spray paints; `None` for a pen.
    dabs: Option<Look>,
    resampler: Resampler,
    /// Raw points taken so far.
    points: usize,
    /// Samples whose pieces are in `coverage`.
    settled: usize,
    /// Alpha is the settled pieces' coverage, or for dabs the settled dabs
    /// painted; document size.
    coverage: Pixmap,
    /// Where `coverage` has been written.
    written: Option<PixelRect>,
    /// The tail over the settled coverage, and where it lies.
    tail: Option<(PixelRect, Pixmap)>,
    stamp: Stamp,
    painter: dab::Painter,
    placed: Vec<Dab>,
    /// The selection the stroke is cut to.
    clip: Option<Mask>,
    /// Where a broken line breaks; `None` when it shows whole.
    breaks: Option<Breaks>,
    /// A pixel brush's pixels so far, in `coverage` at full coverage.
    pixels: Option<pixel::Cells>,
    /// The walk along the moved samples that no longer move.
    walk: Walk,
    /// Samples in `walk`.
    walked: usize,
    /// `stroke::shown` for the samples so far; the first `fixed_shown` no
    /// longer change.
    shown: Vec<bool>,
    fixed_shown: usize,
}

impl LiveStroke {
    /// `color` is the stroke's own, straight.
    /// `spare` is a previous stroke's coverage from `into_spare`, used again
    /// when it has the document's size: allocating and freeing a document's
    /// worth of pixels for every stroke costs milliseconds on large canvases.
    pub fn new(
        size: [u16; 2],
        pen: Pen,
        frame: u32,
        color: [u8; 4],
        erase: bool,
        spare: Option<Pixmap>,
    ) -> Self {
        let coverage = spare
            .filter(|spare| [spare.width(), spare.height()] == size)
            .unwrap_or_else(|| Pixmap::new(size[0], size[1]));
        let dabs = matches!(pen.brush.engine, BrushEngine::Airbrush | BrushEngine::Spray)
            .then(|| dab::look(&pen.brush));
        let spacing = match dabs {
            Some(_) => pen.dab_spacing(frame),
            None => stroke::spacing(pen.width),
        };
        let [r, g, b, a] = color;
        Self {
            pen,
            frame,
            color: if erase { [0, 0, 0, a] } else { [r, g, b, a] },
            line: [0; 4],
            erase,
            dabs,
            resampler: Resampler::new(spacing),
            points: 0,
            settled: 0,
            coverage,
            written: None,
            tail: None,
            stamp: Stamp::default(),
            painter: dab::Painter::default(),
            placed: Vec::new(),
            clip: None,
            breaks: pen.motion.breaks(pen.seed, frame),
            pixels: (pen.brush.engine == BrushEngine::Pixel).then(pixel::Cells::default),
            walk: Walk::default(),
            walked: 0,
            shown: Vec::new(),
            fixed_shown: 0,
        }
    }

    /// Cuts the stroke to `clip`, as the renderer cuts a stroke drawn in a
    /// selection.
    pub fn with_clip(mut self, clip: Option<Mask>) -> Self {
        self.clip = clip;
        self
    }

    /// Brings `shown` up to `samples`. The moved sample `i` depends on
    /// sample `i + 1`, so it no longer moves once that is a regular sample.
    fn walk(&mut self, samples: &[stroke::Sample]) {
        let Some(breaks) = self.breaks else {
            return;
        };
        self.shown.truncate(self.fixed_shown);
        let fixed = self
            .resampler
            .settled()
            .saturating_sub(1)
            .min(samples.len());
        let mut moved = stroke::Moved::new(samples, &self.pen, self.frame);
        for index in self.walked..fixed {
            self.shown
                .extend(breaks.step(&mut self.walk, moved.at(index)));
        }
        self.walked = self.walked.max(fixed);
        self.fixed_shown = self.shown.len();
        let mut walk = self.walk;
        for index in self.walked..samples.len() {
            self.shown.extend(breaks.step(&mut walk, moved.at(index)));
        }
    }

    /// `shown` as `stroke::pieces` and `dab::dabs_of` take it.
    fn shown(&self) -> Option<&[bool]> {
        (!self.shown.is_empty()).then_some(&self.shown[..])
    }

    /// The coverage cleared again, for the next stroke's `new`.
    pub fn into_spare(mut self) -> Pixmap {
        if let Some([left, top, right, bottom]) = self.written {
            let row = usize::from(self.coverage.width()) * 4;
            let pixels = self.coverage.data_as_u8_slice_mut();
            for y in top as usize..bottom as usize {
                pixels[y * row + left as usize * 4..y * row + right as usize * 4].fill(0);
            }
        }
        self.coverage
    }

    /// Takes the points after those seen before and returns the pixels whose
    /// coverage changed.
    pub fn update(&mut self, points: &[Point]) -> Option<PixelRect> {
        if self.points == 0
            && let Some(first) = points.first()
        {
            let [r, g, b, a] = self.color;
            let alpha = stroke::line_alpha_at(a, &self.pen.brush, f64::from(first.pressure));
            self.line = premultiplied([r, g, b, alpha]);
        }
        if self.pixels.is_some() {
            self.points = points.len();
            return self.update_pixels(points);
        }
        for point in &points[self.points.min(points.len())..] {
            self.resampler.push(*point);
        }
        self.points = points.len();
        let samples = self.resampler.samples_with_tail();
        if samples.is_empty() {
            return None;
        }
        self.walk(&samples);
        if let Some(look) = self.dabs {
            return self.update_dabs(&samples, look);
        }
        // A sample is fixed once the two after it are regular samples: its
        // circle is cut by the band to the next, which turns with the one after.
        let fixed = self.resampler.settled().saturating_sub(2);
        let mut dirty = None;

        if fixed > self.settled {
            let pieces = stroke::pieces(
                &samples,
                &self.pen,
                self.frame,
                self.settled..fixed,
                self.settled.saturating_sub(1)..fixed.saturating_sub(1),
                self.shown(),
            );
            if let Some(rect) = clamp(pieces.bounds, self.size()) {
                let piece = self.draw(rect, &pieces.path);
                add_coverage(&mut self.coverage, &piece, rect);
                self.written = union(self.written, Some(rect));
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
            self.shown(),
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

    /// `update` for a pixel brush. Its pixels are found again from all the
    /// points, since a new point can take back the last corner pixel and the
    /// wobble's moves near the end; only the pixels that changed are written.
    fn update_pixels(&mut self, points: &[Point]) -> Option<PixelRect> {
        let cells = pixel::cells(points, &self.pen, self.frame);
        let before = self.pixels.replace(cells).unwrap_or_default();
        let cells = self.pixels.as_ref().expect("just set");
        let (added, removed) = cells.difference(&before);
        let size = self.size();
        let width = usize::from(self.coverage.width());
        let pixels = self.coverage.data_as_u8_slice_mut();
        let mut dirty = None;
        for (runs, cover) in [(removed, 0), (added, 255)] {
            for [y, from, to] in runs {
                if y < 0 || y >= size[1] as i32 {
                    continue;
                }
                let (from, to) = (from.max(0), to.min(size[0] as i32));
                if from >= to {
                    continue;
                }
                let row = y as usize * width;
                for x in from as usize..to as usize {
                    pixels[(row + x) * 4 + 3] = cover;
                }
                dirty = union(
                    dirty,
                    Some([from as u32, y as u32, to as u32, y as u32 + 1]),
                );
            }
        }
        self.written = union(self.written, dirty);
        dirty
    }

    /// `update` for an airbrush or spray. A sample's dabs no longer move once
    /// the sample after it is a regular sample; on a broken line, once the
    /// segment after it no longer changes either.
    fn update_dabs(&mut self, samples: &[stroke::Sample], look: Look) -> Option<PixelRect> {
        let waits = if self.breaks.is_some() { 2 } else { 1 };
        let fixed = self.resampler.settled().saturating_sub(waits);
        let [r, g, b, alpha] = self.color;
        let mut dirty = None;
        if fixed > self.settled {
            self.placed.clear();
            let range = self.settled..fixed;
            let shown = (!self.shown.is_empty()).then_some(&self.shown[..]);
            dab::dabs_of(
                samples,
                range,
                &self.pen,
                self.frame,
                alpha,
                shown,
                &mut self.placed,
            );
            if let Some(rect) = clamp(dab::bounds(&self.placed), self.size()) {
                let width = usize::from(self.coverage.width());
                let pixels = self.coverage.data_as_u8_slice_mut();
                self.painter
                    .paint(pixels, width, &self.placed, look, [r, g, b]);
                self.written = union(self.written, Some(rect));
                dirty = Some(rect);
            }
            self.settled = fixed;
        }

        let old_tail = self.tail.take().map(|(rect, _)| rect);
        self.placed.clear();
        let range = self.settled..samples.len();
        let shown = (!self.shown.is_empty()).then_some(&self.shown[..]);
        dab::dabs_of(
            samples,
            range,
            &self.pen,
            self.frame,
            alpha,
            shown,
            &mut self.placed,
        );
        if let Some(rect) = clamp(dab::bounds(&self.placed), self.size()) {
            let mut tail = copy(&self.coverage, rect);
            for dab in &mut self.placed {
                dab.center = [
                    dab.center[0] - f64::from(rect[0]),
                    dab.center[1] - f64::from(rect[1]),
                ];
            }
            let width = usize::from(tail.width());
            self.painter.paint(
                tail.data_as_u8_slice_mut(),
                width,
                &self.placed,
                look,
                [r, g, b],
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

    /// The pixel at `x`, `y`: the tail's where it lies, else the settled one.
    fn pixel(&self, x: usize, y: usize) -> [u8; 4] {
        let (x32, y32) = (x as u32, y as u32);
        let (pixels, at) = match &self.tail {
            Some(([left, top, right, bottom], tail))
                if (*left..*right).contains(&x32) && (*top..*bottom).contains(&y32) =>
            {
                let at = ((y32 - top) * u32::from(tail.width()) + x32 - left) as usize;
                (tail, at)
            }
            _ => (&self.coverage, y * usize::from(self.coverage.width()) + x),
        };
        pixels.data_as_u8_slice()[at * 4..at * 4 + 4]
            .try_into()
            .expect("four bytes")
    }

    fn cover(&self, x: usize, y: usize) -> u8 {
        let cover = self.pixel(x, y)[3];
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
        if !inside(self.clip.as_ref(), x, y) {
            return;
        }
        if self.dabs.is_some() {
            let pixel = self.pixel(x, y);
            if pixel[3] == 0 {
            } else if self.erase {
                dest_out(layer, pixel[3]);
            } else {
                src_over(layer, &pixel);
            }
            return;
        }
        let cover = self.cover(x, y);
        if cover == 0 {
            return;
        }
        if self.erase {
            erase(layer, self.line, cover);
        } else {
            paint(layer, self.line, cover);
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
    use crate::stroke::Resampler;
    use ugu_core::motion::Mover;
    use ugu_core::ops::{Motion, MotionStyle};
    use ugu_core::store::{Brush, BrushEngine, TipShape};

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
                ..Brush::default()
            },
            seed: 77,
            wobble: 3.0,
            motion: Mover::new(Motion::DEFAULT, 30),
        }
    }

    /// A broken line of `style` that hides about half of a stroke.
    fn broken(style: MotionStyle) -> Mover {
        let motion = Motion {
            style,
            broken: true,
            break_amount: 0.45,
            break_range: 14.0,
            ..Motion::DEFAULT
        };
        Mover::new(motion, 30)
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
        let whole_line = Mover::new(Motion::DEFAULT, 30);
        let cases = [
            (true, whole_line, TipShape::Round),
            (false, whole_line, TipShape::Round),
            (true, broken(MotionStyle::Smooth), TipShape::Round),
            (false, broken(MotionStyle::Stepped), TipShape::Round),
            (true, broken(MotionStyle::Classic), TipShape::Square),
        ];
        for (antialias, motion, tip) in cases {
            let mut pen = Pen {
                motion,
                ..pen(antialias)
            };
            pen.brush.tip = tip;
            let points = points();
            let mut live = LiveStroke::new([200, 120], pen, 2, [0, 0, 0, 255], false, None);
            let mut seen: Option<PixelRect> = None;
            for count in 1..=points.len() {
                seen = union(seen, live.update(&points[..count]));
            }
            let covered = live_coverage(&live);
            let expected = whole(&pen, &points);
            if motion != whole_line {
                let full = whole(
                    &Pen {
                        motion: whole_line,
                        ..pen
                    },
                    &points,
                );
                let sum = |coverage: &[u8]| coverage.iter().map(|&a| u32::from(a)).sum::<u32>();
                let shown = f64::from(sum(&expected)) / f64::from(sum(&full));
                assert!((0.2..0.8).contains(&shown), "{shown} of the line shows");
            }
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
    fn a_pixel_stroke_drawn_point_by_point_is_its_whole_pixels() {
        let whole_line = Mover::new(Motion::DEFAULT, 30);
        for (motion, width) in [
            (whole_line, 1.0),
            (whole_line, 4.0),
            (broken(MotionStyle::Smooth), 1.0),
            (broken(MotionStyle::Stepped), 3.0),
        ] {
            let mut pen = Pen {
                motion,
                width,
                ..pen(false)
            };
            pen.brush = ugu_core::brush::find("pixel-pencil").unwrap().brush;
            let points = points();
            let mut live = LiveStroke::new([200, 120], pen, 2, [0, 0, 0, 255], false, None);
            let mut seen: Option<PixelRect> = None;
            let mut before = vec![0; 200 * 120];
            for count in 1..=points.len() {
                let dirty = live.update(&points[..count]);
                seen = union(seen, dirty);
                let now = live_coverage(&live);
                // Every pixel that changed, taken back ones too, was reported.
                for (index, (&a, &b)) in now.iter().zip(&before).enumerate() {
                    if a != b {
                        let (x, y) = ((index % 200) as u32, (index / 200) as u32);
                        let [left, top, right, bottom] = dirty.unwrap();
                        assert!(x >= left && x < right && y >= top && y < bottom);
                    }
                }
                before = now;
            }
            let cells = crate::pixel::cells(&points, &pen, 2);
            for (index, &a) in before.iter().enumerate() {
                let (x, y) = ((index % 200) as i32, (index / 200) as i32);
                let expected = if cells.contains(x, y) { 255 } else { 0 };
                assert_eq!(a, expected, "at {x}, {y}");
            }
            assert!(seen.is_some());
        }
    }

    #[test]
    fn a_stroke_in_a_selection_changes_only_pixels_inside_it() {
        let selection = ugu_core::selection::Selection::of_shape(
            &ugu_core::selection::Shape::Rectangle([60.0, 0.0], [120.0, 120.0]),
            [200, 120],
        )
        .unwrap();
        let mask = selection.mask().clone();
        let pen = pen(true);
        let points = points();
        let mut cut = LiveStroke::new([200, 120], pen, 2, [0, 0, 0, 255], false, None)
            .with_clip(Some(mask.clone()));
        let mut whole = LiveStroke::new([200, 120], pen, 2, [0, 0, 0, 255], false, None);
        for count in 1..=points.len() {
            cut.update(&points[..count]);
            whole.update(&points[..count]);
        }
        let mut inside = 0;
        for y in 0..120 {
            for x in 0..200 {
                let (mut a, mut b) = ([255; 4], [255; 4]);
                cut.apply(x, y, &mut a);
                whole.apply(x, y, &mut b);
                if mask.contains(x as i32, y as i32) {
                    assert_eq!(a, b);
                    inside += usize::from(a != [255; 4]);
                } else {
                    assert_eq!(a, [255; 4], "at {x}, {y}");
                }
            }
        }
        assert!(inside > 0);
    }

    #[test]
    fn the_tail_is_replaced_not_added_to() {
        let pen = pen(true);
        let points = points();
        let mut stepwise = LiveStroke::new([200, 120], pen, 2, [0, 0, 0, 255], false, None);
        for count in 1..=points.len() {
            stepwise.update(&points[..count]);
        }
        let mut at_once = LiveStroke::new([200, 120], pen, 2, [0, 0, 0, 255], false, None);
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
    fn a_stroke_on_a_used_coverage_covers_what_it_does_on_a_new_one() {
        let pen = pen(true);
        let points = points();
        let mut first = LiveStroke::new([200, 120], pen, 2, [0, 0, 0, 255], false, None);
        for count in 1..=points.len() {
            first.update(&points[..count]);
        }
        let spare = first.into_spare();
        assert!(spare.data_as_u8_slice().iter().all(|&value| value == 0));
        let other: Vec<Point> = points
            .iter()
            .map(|point| Point {
                y: 120.0 - point.y,
                ..*point
            })
            .collect();
        let mut reused = LiveStroke::new([200, 120], pen, 3, [0, 0, 0, 255], false, Some(spare));
        let mut fresh = LiveStroke::new([200, 120], pen, 3, [0, 0, 0, 255], false, None);
        for count in 1..=other.len() {
            reused.update(&other[..count]);
            fresh.update(&other[..count]);
        }
        assert_eq!(live_coverage(&reused), live_coverage(&fresh));
    }

    #[test]
    fn dabs_drawn_point_by_point_are_the_whole_stroke_painted() {
        let ids = ["soft-airbrush", "pixel-spray", "rough-spray", "soft-eraser"];
        let motions = [
            Mover::new(Motion::DEFAULT, 30),
            broken(MotionStyle::Smooth),
            broken(MotionStyle::Stepped),
        ];
        for (id, motion) in ids
            .into_iter()
            .flat_map(|id| motions.map(|motion| (id, motion)))
        {
            let preset = ugu_core::brush::find(id).unwrap();
            let pen = Pen {
                width: 24.0,
                brush: preset.brush,
                seed: 77,
                wobble: 3.0,
                motion,
            };
            let points = points();
            let color = [30, 120, 200, 230];
            let mut live = LiveStroke::new([200, 120], pen, 2, color, false, None);
            let mut seen: Option<PixelRect> = None;
            for count in 1..=points.len() {
                seen = union(seen, live.update(&points[..count]));
            }
            let mut dabs = Vec::new();
            dab::stroke_dabs(&points, &pen, 2, color[3], &mut dabs);
            if motion.motion().broken {
                let mut all = Vec::new();
                let whole_line = Mover::new(Motion::DEFAULT, 30);
                let unbroken = Pen {
                    motion: whole_line,
                    ..pen
                };
                dab::stroke_dabs(&points, &unbroken, 2, color[3], &mut all);
                let shown = dabs.len() as f64 / all.len() as f64;
                assert!(
                    (0.2..0.8).contains(&shown),
                    "{id}: {shown} of the dabs show"
                );
            }
            let mut whole = vec![0; 200 * 120 * 4];
            let look = dab::look(&pen.brush);
            dab::Painter::default().paint(&mut whole, 200, &dabs, look, [30, 120, 200]);
            let [left, top, right, bottom] = seen.unwrap();
            for y in 0..120 {
                for x in 0..200 {
                    let expected = &whole[(y * 200 + x) * 4..(y * 200 + x) * 4 + 4];
                    assert_eq!(live.pixel(x, y), expected, "{id} at {x}, {y}");
                    if expected[3] > 0 {
                        let (x, y) = (x as u32, y as u32);
                        assert!(x >= left && x < right && y >= top && y < bottom);
                    }
                }
            }
        }
    }

    #[test]
    fn a_translucent_live_stroke_does_not_darken_where_it_overlaps() {
        let pen = pen(true);
        let mut live = LiveStroke::new([200, 120], pen, 2, [200, 0, 0, 100], false, None);
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
        assert_eq!(darkest, 100);
    }
}

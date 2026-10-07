// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! The shape of a pen stroke on one frame.
//!
//! Raw input points are resampled at a fixed spacing along the stroke, each
//! sample is moved by the frame's motion, and the moved samples become an
//! outline: the union of a circle per sample and the band joining each pair,
//! filled once, so a translucent stroke is equally translucent everywhere.
//! Every stroke takes this one shape, whether its pressure varies or not, so
//! a small change in pressure never changes how a stroke is drawn.

use ugu_core::motion::classic;
use ugu_core::store::{Brush, Point};
use vello_cpu::kurbo::{self, BezPath, Circle, Shape};

/// Resampling never makes more samples than this; longer strokes are sampled
/// more sparsely.
pub const MAX_SAMPLES: usize = 200_000;
/// A last input point closer than this to the last sample adds no sample.
const TAIL_EPSILON: f64 = 0.01;
/// Curve flattening tolerance for circles, in pixels.
const TOLERANCE: f64 = 0.05;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sample {
    pub position: [f64; 2],
    pub pressure: f64,
    /// Distance along the resampled stroke from its first sample.
    pub arc: f64,
}

/// The base spacing of samples for a pen of `width`.
pub fn spacing(width: f32) -> f64 {
    (f64::from(width) * 0.55).clamp(2.0, 5.0)
}

/// Samples raw points as they arrive, so a stroke being drawn and the same
/// stroke drawn again from its points give the same samples.
#[derive(Clone, Debug)]
pub struct Resampler {
    spacing: f64,
    samples: Vec<Sample>,
    /// The last raw point, which the samples may not have reached.
    last: Option<Sample>,
    /// Distance from the last sample along the raw points so far.
    carried: f64,
}

impl Resampler {
    pub fn new(spacing: f64) -> Self {
        Self {
            spacing,
            samples: Vec::new(),
            last: None,
            carried: 0.0,
        }
    }

    /// Samples for all of `points`, with a spacing widened if needed so that
    /// there are at most `MAX_SAMPLES`.
    pub fn whole(points: &[Point], base_spacing: f64) -> Vec<Sample> {
        let length: f64 = points
            .windows(2)
            .map(|pair| distance(position(&pair[0]), position(&pair[1])))
            .sum();
        let mut resampler = Self::new(base_spacing.max(length / (MAX_SAMPLES - 2) as f64));
        for point in points {
            resampler.push(*point);
        }
        resampler.samples_with_tail()
    }

    pub fn spacing(&self) -> f64 {
        self.spacing
    }

    pub fn push(&mut self, point: Point) {
        let next = Sample {
            position: position(&point),
            pressure: f64::from(point.pressure),
            arc: 0.0,
        };
        let Some(previous) = self.last.replace(next) else {
            self.samples.push(next);
            return;
        };
        let length = distance(previous.position, next.position);
        if length == 0.0 {
            return;
        }
        // Distance along this segment of the next sample.
        let mut at = self.spacing - self.carried;
        while at <= length {
            let t = at / length;
            let arc = self.samples.last().map_or(0.0, |sample| sample.arc) + self.spacing;
            self.samples.push(Sample {
                position: lerp(previous.position, next.position, t),
                pressure: previous.pressure + (next.pressure - previous.pressure) * t,
                arc,
            });
            at += self.spacing;
        }
        self.carried = length - (at - self.spacing);
    }

    /// The samples so far: those at the spacing, and the last raw point
    /// unless a sample is already there.
    pub fn samples_with_tail(&self) -> Vec<Sample> {
        let mut samples = self.samples.clone();
        if let (Some(last), Some(sample)) = (self.last, self.samples.last())
            && distance(last.position, sample.position) > TAIL_EPSILON
        {
            samples.push(Sample {
                arc: sample.arc + self.carried,
                ..last
            });
        }
        samples
    }

    /// How many samples will not change as more points arrive.
    pub fn settled(&self) -> usize {
        self.samples.len()
    }
}

fn position(point: &Point) -> [f64; 2] {
    [f64::from(point.x), f64::from(point.y)]
}

fn distance(a: [f64; 2], b: [f64; 2]) -> f64 {
    (b[0] - a[0]).hypot(b[1] - a[1])
}

fn lerp(a: [f64; 2], b: [f64; 2], t: f64) -> [f64; 2] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]
}

/// What decides a stroke's shape on a frame besides its samples.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pen {
    pub width: f32,
    pub brush: Brush,
    pub seed: u64,
    /// The layer's wobble amount times the brush's wobble scale.
    pub wobble: f64,
}

impl Pen {
    fn pressure_scale(&self, pressure: f64) -> f64 {
        let dynamics = f64::from(self.brush.size_dynamics);
        1.0 - dynamics + pressure.clamp(0.0, 1.0) * dynamics
    }

    /// No part of the stroke reaches further than this from its raw points.
    pub fn reach(&self) -> f64 {
        let width = f64::from(self.width);
        // `classic::width` varies the width by at most 2.5%.
        (width * 1.025).max(0.5) * 0.5 + classic::max_displacement(width, self.wobble)
    }
}

/// The pixels a stroke can touch on any frame: left, top, right, bottom.
pub fn bounds(points: &[Point], pen: &Pen) -> [f64; 4] {
    let reach = pen.reach();
    points.iter().fold(
        [
            f64::INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NEG_INFINITY,
        ],
        |[left, top, right, bottom], point| {
            let [x, y] = position(point);
            [
                left.min(x - reach),
                top.min(y - reach),
                right.max(x + reach),
                bottom.max(y + reach),
            ]
        },
    )
}

/// Samples moved by the motion of `frame`.
pub fn displaced(samples: &[Sample], pen: &Pen, frame: u32) -> Vec<[f64; 2]> {
    let amplitude = classic::amplitude(f64::from(pen.width), pen.wobble);
    if amplitude == 0.0 {
        return samples.iter().map(|sample| sample.position).collect();
    }
    let last = samples.len().saturating_sub(1);
    (0..samples.len())
        .map(|index| {
            let before = samples[index.saturating_sub(1)].position;
            let after = samples[(index + 1).min(last)].position;
            let length = distance(before, after);
            let tangent = if length > 1e-9 {
                [
                    (after[0] - before[0]) / length,
                    (after[1] - before[1]) / length,
                ]
            } else {
                [1.0, 0.0]
            };
            let sample = &samples[index];
            classic::displace(
                sample.position,
                sample.pressure,
                tangent,
                sample.arc,
                amplitude,
                pen.seed,
                frame,
            )
        })
        .collect()
}

/// The stroke's outline on `frame`, to fill with the non-zero rule; `None`
/// without samples.
pub fn outline(samples: &[Sample], pen: &Pen, frame: u32) -> Option<BezPath> {
    if samples.is_empty() {
        return None;
    }
    let points = displaced(samples, pen, frame);
    let base = classic::width(f64::from(pen.width), pen.seed, frame, pen.wobble);
    let radius = |sample: &Sample| (base * pen.pressure_scale(sample.pressure)).max(0.5) * 0.5;
    let point = |[x, y]: [f64; 2]| kurbo::Point::new(x, y);

    let mut path = BezPath::new();
    for (sample, &center) in samples.iter().zip(&points) {
        path.extend(Circle::new(point(center), radius(sample)).path_elements(TOLERANCE));
    }
    let reverse = *BANDS_REVERSED;
    for (pair, centers) in samples.windows(2).zip(points.windows(2)) {
        let radii = [radius(&pair[0]), radius(&pair[1])];
        if let Some(mut corners) = band(centers[0], centers[1], radii) {
            if reverse {
                corners.reverse();
            }
            path.move_to(corners[0]);
            for &corner in &corners[1..] {
                path.line_to(corner);
            }
            path.close_path();
        }
    }
    Some(path)
}

/// Whether `band`'s corners run against kurbo's circles. Rotating, moving
/// and scaling keep a polygon's orientation, so one band decides for all.
static BANDS_REVERSED: std::sync::LazyLock<bool> = std::sync::LazyLock::new(|| {
    let circle = Circle::new((0.0, 0.0), 1.0).to_path(TOLERANCE).area();
    let corners = band([0.0, 0.0], [4.0, 0.0], [1.0, 2.0]).expect("apart");
    let mut quad = BezPath::new();
    quad.move_to(corners[0]);
    for &corner in &corners[1..] {
        quad.line_to(corner);
    }
    quad.close_path();
    quad.area().signum() != circle.signum()
});

/// The corners of the quadrilateral between the outer tangents of two
/// circles, always in the same orientation. `None` when one circle contains
/// the other.
fn band(a: [f64; 2], b: [f64; 2], [ra, rb]: [f64; 2]) -> Option<[kurbo::Point; 4]> {
    let length = distance(a, b);
    let k = (ra - rb) / length;
    if !(length > 0.0 && k.abs() < 1.0) {
        return None;
    }
    let along = [(b[0] - a[0]) / length, (b[1] - a[1]) / length];
    let across = [-along[1], along[0]];
    let side = (1.0 - k * k).sqrt();
    let touch = |center: [f64; 2], radius: f64, sign: f64| {
        kurbo::Point::new(
            center[0] + radius * (k * along[0] + sign * side * across[0]),
            center[1] + radius * (k * along[1] + sign * side * across[1]),
        )
    };
    Some([
        touch(a, ra, 1.0),
        touch(b, rb, 1.0),
        touch(b, rb, -1.0),
        touch(a, ra, -1.0),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use ugu_core::store::BrushEngine;
    use vello_cpu::kurbo::{Cap, Join, Stroke};
    use vello_cpu::{Pixmap, RasterizerSettings, RenderContext, RenderSettings, Resources};

    fn pen(size_dynamics: f32, wobble: f64) -> Pen {
        Pen {
            width: 12.0,
            brush: Brush {
                engine: BrushEngine::Line,
                opacity: 1.0,
                hardness: 1.0,
                antialias: true,
                size_dynamics,
                wobble_scale: 1.0,
            },
            seed: 0x5eed,
            wobble,
        }
    }

    fn wave(count: usize, pressure: impl Fn(usize) -> f32) -> Vec<Point> {
        (0..count)
            .map(|index| Point {
                x: 20.0 + index as f32 * 3.1,
                y: 64.0 + (index as f32 * 0.21).sin() * 30.0,
                pressure: pressure(index),
            })
            .collect()
    }

    /// Premultiplied RGBA8 of `path` in `color` on a transparent canvas,
    /// filled or, with a width, stroked.
    fn rasterize(path: &BezPath, stroke: Option<f64>, color: [u8; 4]) -> Vec<u8> {
        let mut context = RenderContext::new_with(256, 128, RenderSettings::default());
        let [r, g, b, a] = color;
        context.set_paint(vello_cpu::color::AlphaColor::from_rgba8(r, g, b, a));
        match stroke {
            Some(width) => {
                context.set_stroke(
                    Stroke::new(width)
                        .with_caps(Cap::Round)
                        .with_join(Join::Round),
                );
                context.stroke_path(path);
            }
            None => context.fill_path(path),
        }
        let mut pixmap = Pixmap::new(256, 128);
        context.flush();
        context.render_with(
            &mut pixmap,
            &mut Resources::new(),
            RasterizerSettings::default(),
        );
        pixmap.data_as_u8_slice().to_vec()
    }

    #[test]
    fn samples_are_evenly_spaced_and_end_at_the_last_point() {
        let points = wave(40, |_| 1.0);
        let samples = Resampler::whole(&points, 4.0);
        for pair in samples[..samples.len() - 1].windows(2) {
            assert!((distance(pair[0].position, pair[1].position) - 4.0).abs() < 0.5);
            assert!((pair[1].arc - pair[0].arc - 4.0).abs() < 1e-9);
        }
        let last = samples.last().unwrap();
        assert_eq!(last.position, position(points.last().unwrap()));
    }

    #[test]
    fn sampling_while_drawing_matches_sampling_afterwards() {
        let points = wave(60, |index| 0.3 + index as f32 / 100.0);
        let mut live = Resampler::new(spacing(12.0));
        for (index, point) in points.iter().enumerate() {
            live.push(*point);
            let settled = live.settled();
            let so_far = Resampler::whole(&points[..=index], spacing(12.0));
            assert_eq!(live.samples_with_tail(), so_far);
            assert!(settled <= so_far.len());
        }
    }

    #[test]
    fn a_long_stroke_is_sampled_more_sparsely() {
        let points = [
            Point {
                x: 0.0,
                y: 0.0,
                pressure: 1.0,
            },
            Point {
                x: 30_000.0,
                y: 30_000.0,
                pressure: 1.0,
            },
            Point {
                x: -30_000.0,
                y: 30_000.0,
                pressure: 1.0,
            },
            Point {
                x: 30_000.0,
                y: -30_000.0,
                pressure: 1.0,
            },
            Point {
                x: -30_000.0,
                y: -30_000.0,
                pressure: 1.0,
            },
            Point {
                x: 30_000.0,
                y: 30_000.0,
                pressure: 1.0,
            },
        ];
        assert!(Resampler::whole(&points, 2.0).len() <= MAX_SAMPLES);
    }

    #[test]
    fn every_frame_stays_inside_the_bounds() {
        let points = wave(80, |index| (index % 7) as f32 / 7.0);
        for wobble in [0.0, 1.6, 12.0] {
            let pen = pen(0.8, wobble);
            let [left, top, right, bottom] = bounds(&points, &pen);
            let samples = Resampler::whole(&points, spacing(pen.width));
            for frame in 0..30 {
                let shape = outline(&samples, &pen, frame).unwrap().bounding_box();
                assert!(
                    shape.x0 >= left && shape.y0 >= top,
                    "frame {frame}, wobble {wobble}"
                );
                assert!(
                    shape.x1 <= right && shape.y1 <= bottom,
                    "frame {frame}, wobble {wobble}"
                );
            }
        }
    }

    #[test]
    fn the_union_has_the_shape_of_a_round_stroke() {
        let points = wave(60, |_| 0.7);
        let pen = pen(0.8, 1.6);
        let samples = Resampler::whole(&points, spacing(pen.width));
        let union = rasterize(&outline(&samples, &pen, 3).unwrap(), None, [0, 0, 0, 255]);
        let mut line = BezPath::new();
        for (index, [x, y]) in displaced(&samples, &pen, 3).into_iter().enumerate() {
            let at = kurbo::Point::new(x, y);
            if index == 0 {
                line.move_to(at);
            } else {
                line.line_to(at);
            }
        }
        let width = classic::width(12.0, pen.seed, 3, pen.wobble) * pen.pressure_scale(0.7);
        let stroked = rasterize(&line, Some(width), [0, 0, 0, 255]);
        assert!(stroked.chunks(4).filter(|pixel| pixel[3] == 255).count() > 1000);

        // Both shapes overlap pieces of themselves (the union its circles and
        // bands, the stroker its joins), and Vello sums overlapping coverage,
        // so antialiased edge pixels differ: the union's are about 0.1 pixel
        // darker. The shape is the same: no holes inside, nothing outside.
        let (mut edges, mut darker) = (0, 0);
        for (index, (a, b)) in stroked.chunks(4).zip(union.chunks(4)).enumerate() {
            let (a, b) = (i32::from(a[3]), i32::from(b[3]));
            let at = (index % 256, index / 256);
            match a {
                255 => assert!(b >= 250, "hole at {at:?}: {b}"),
                0 => assert!(b < 64, "outside at {at:?}: {b}"),
                _ => {
                    edges += 1;
                    darker += b - a;
                }
            }
        }
        let mean = f64::from(darker) / f64::from(edges);
        assert!(
            (0.0..32.0).contains(&mean),
            "edges {mean} darker on average"
        );
    }

    #[test]
    fn a_translucent_pressure_stroke_does_not_darken_where_it_overlaps() {
        let points = wave(60, |index| 0.2 + (index % 10) as f32 / 12.0);
        let pen = pen(0.8, 1.6);
        let samples = Resampler::whole(&points, spacing(pen.width));
        let pixels = rasterize(&outline(&samples, &pen, 0).unwrap(), None, [0, 0, 0, 100]);
        let darkest = pixels.chunks(4).map(|pixel| pixel[3]).max().unwrap();
        assert_eq!(darkest, 100);
    }

    #[test]
    fn circles_and_bands_wind_the_same_way() {
        let points = wave(80, |index| 0.2 + (index % 9) as f32 / 10.0);
        let pen = pen(0.8, 6.0);
        let samples = Resampler::whole(&points, spacing(pen.width));
        let path = outline(&samples, &pen, 4).unwrap();
        let mut signs = Vec::new();
        let mut piece = BezPath::new();
        for element in path.elements() {
            if matches!(element, kurbo::PathEl::MoveTo(_)) && !piece.elements().is_empty() {
                signs.push(piece.area().signum());
                piece = BezPath::new();
            }
            piece.push(*element);
        }
        signs.push(piece.area().signum());
        assert!(signs.len() > samples.len());
        assert!(signs.iter().all(|&sign| sign == signs[0]));
    }

    #[test]
    fn a_single_point_is_a_dot_of_its_pressure_width() {
        let points = [Point {
            x: 50.0,
            y: 50.0,
            pressure: 0.5,
        }];
        let pen = pen(0.8, 0.0);
        let samples = Resampler::whole(&points, spacing(pen.width));
        let shape = outline(&samples, &pen, 0).unwrap().bounding_box();
        // 12 × (1 − 0.8 + 0.5 × 0.8) = 7.2
        assert!((shape.width() - 7.2).abs() < 0.01);
    }
}

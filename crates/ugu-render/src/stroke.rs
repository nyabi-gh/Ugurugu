// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! The shape of a pen stroke on one frame.
//!
//! Raw input points are resampled at a fixed spacing along the stroke, each
//! sample is moved by the frame's motion, and the moved samples become an
//! outline: the union of a circle per sample and the band joining each pair,
//! filled once, so a translucent stroke is equally translucent everywhere.
//! Most of each circle is inside the bands on either side, so only the part
//! outside both is drawn, and the whole is traced as one outline.
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
    (0..samples.len())
        .map(|index| displaced_at(samples, index, pen, frame))
        .collect()
}

/// Sample `index` moved by the motion of `frame`. It depends on the samples
/// on either side, which give its direction.
pub fn displaced_at(samples: &[Sample], index: usize, pen: &Pen, frame: u32) -> [f64; 2] {
    let sample = &samples[index];
    let amplitude = classic::amplitude(f64::from(pen.width), pen.wobble);
    if amplitude == 0.0 {
        return sample.position;
    }
    let before = samples[index.saturating_sub(1)].position;
    let after = samples[(index + 1).min(samples.len() - 1)].position;
    let length = distance(before, after);
    let tangent = if length > 1e-9 {
        [
            (after[0] - before[0]) / length,
            (after[1] - before[1]) / length,
        ]
    } else {
        [1.0, 0.0]
    };
    classic::displace(
        sample.position,
        sample.pressure,
        tangent,
        sample.arc,
        amplitude,
        pen.seed,
        frame,
    )
}

/// The stroke's outline on `frame`, to fill with the non-zero rule; `None`
/// without samples.
pub fn outline(samples: &[Sample], pen: &Pen, frame: u32) -> Option<BezPath> {
    if samples.is_empty() {
        return None;
    }
    let base = classic::width(f64::from(pen.width), pen.seed, frame, pen.wobble);
    let radii: Vec<f64> = samples
        .iter()
        .map(|sample| radius(base, pen, sample))
        .collect();
    let centers = displaced(samples, pen, frame);
    let side = |a: usize, b: usize| left_by_band(centers[a], centers[b], [radii[a], radii[b]]);
    let mut path = BezPath::new();
    let mut halves = Vec::new();
    let mut start = 0;
    while start < samples.len() {
        // A run of samples joined by bands; a circle inside its neighbour
        // ends one.
        let mut end = start;
        halves.clear();
        halves.push([None, None]);
        while end + 1 < samples.len()
            && let (Some(ahead), Some(behind)) = (side(end, end + 1), side(end + 1, end))
        {
            halves[end - start][1] = Some(ahead);
            halves.push([Some(behind), None]);
            end += 1;
        }
        run(
            &mut path,
            &centers[start..=end],
            &radii[start..=end],
            &halves,
        );
        start = end + 1;
    }
    Some(path)
}

/// How the edges meet at a circle between two bands, on the right (from
/// the previous band's right corner to the next band's) and on the left.
#[derive(Clone, Copy)]
enum Joint {
    /// Arcs on both sides.
    Both,
    /// The cap left by the previous band covers the next band's chord.
    Previous,
    /// The cap left by the next band covers the previous band's chord.
    Next,
    /// Nothing outside both bands; both chords stay.
    Neither,
    /// The edges cross at this corner on the right, an arc on the left.
    Right([f64; 2]),
    /// An arc on the right, the edges cross at this corner on the left.
    Left([f64; 2]),
}

/// Appends one closed outline for the circles at `centers` and the bands
/// joining them: the right edges forward, the last circle's cap, the left
/// edges back and the first circle's cap. It is the sum of the bands and
/// `cap`'s pieces less the chords they share in opposite directions, so
/// every point is wound as often as by the pieces and the fill is the same.
fn run(path: &mut BezPath, centers: &[[f64; 2]], radii: &[f64], halves: &[[Option<Half>; 2]]) {
    let last = centers.len() - 1;
    if last == 0 {
        let [x, y] = centers[0];
        path.extend(Circle::new((x, y), radii[0]).path_elements(TOLERANCE));
        return;
    }
    let at = |index: usize, [x, y]: [f64; 2]| {
        kurbo::Point::new(centers[index][0] + x, centers[index][1] + y)
    };
    // Each circle's chords relative to its center: to the previous band
    // right then left, to the next band left then right.
    let chords: Vec<[[f64; 2]; 4]> = (0..=last)
        .map(|index| {
            let radius = radii[index];
            let ends = |half: Option<Half>| {
                half.map_or([[0.0; 2]; 2], |([x, y], offset)| {
                    let along = (radius * radius - offset * offset).max(0.0).sqrt();
                    [
                        [offset * x + along * y, offset * y - along * x],
                        [offset * x - along * y, offset * y + along * x],
                    ]
                })
            };
            let [a0, a1] = ends(halves[index][0]);
            let [b0, b1] = ends(halves[index][1]);
            [a0, a1, b0, b1]
        })
        .collect();
    let joint = |index: usize| {
        let [Some(first), Some(second)] = halves[index] else {
            return Joint::Neither;
        };
        let [a0, a1, b0, b1] = chords[index];
        let inside = |([x, y], offset): Half, [px, py]: [f64; 2]| x * px + y * py >= offset;
        let corner = || {
            let (([ax, ay], ad), ([bx, by], bd)) = (first, second);
            let det = ax * by - ay * bx;
            [(ad * by - bd * ay) / det, (ax * bd - bx * ad) / det]
        };
        let both_b = inside(first, b0) && inside(first, b1);
        match (inside(second, a0), inside(second, a1)) {
            (true, true) if both_b => Joint::Both,
            (true, true) => Joint::Previous,
            (false, false) if both_b => Joint::Next,
            (false, false) => Joint::Neither,
            (false, true) => Joint::Right(corner()),
            (true, false) => Joint::Left(corner()),
        }
    };
    let joints: Vec<Joint> = (0..=last).map(joint).collect();
    let arc = |path: &mut BezPath, index: usize, from: [f64; 2], to: [f64; 2]| {
        arc_to(path, centers[index], radii[index], from, to);
    };

    let [_, _, b0, b1] = chords[0];
    path.move_to(at(0, b0));
    arc(path, 0, b0, b1);
    for index in 1..last {
        let [a0, a1, b0, b1] = chords[index];
        path.line_to(at(index, a0));
        match joints[index] {
            Joint::Both | Joint::Left(_) => arc(path, index, a0, b1),
            Joint::Previous => path.line_to(at(index, b1)),
            Joint::Next => {
                path.line_to(at(index, a1));
                path.line_to(at(index, b0));
                arc(path, index, b0, b1);
            }
            Joint::Neither => {
                path.line_to(at(index, a1));
                path.line_to(at(index, b0));
                path.line_to(at(index, b1));
            }
            Joint::Right(corner) => {
                path.line_to(at(index, corner));
                path.line_to(at(index, b1));
            }
        }
    }
    let [a0, a1, _, _] = chords[last];
    path.line_to(at(last, a0));
    arc(path, last, a0, a1);
    for index in (1..last).rev() {
        let [a0, a1, b0, b1] = chords[index];
        path.line_to(at(index, b0));
        match joints[index] {
            Joint::Both | Joint::Right(_) => arc(path, index, b0, a1),
            Joint::Previous => {
                path.line_to(at(index, b1));
                path.line_to(at(index, a0));
                arc(path, index, a0, a1);
            }
            Joint::Next | Joint::Neither => path.line_to(at(index, a1)),
            Joint::Left(corner) => {
                path.line_to(at(index, corner));
                path.line_to(at(index, a1));
            }
        }
    }
    path.close_path();
}

/// Part of an outline.
pub struct Pieces {
    pub path: BezPath,
    /// Left, top, right, bottom; empty (inverted) with no pieces.
    pub bounds: [f64; 4],
}

/// The circles of samples in `circles`, less what the bands on either side
/// cover, and the bands from each sample in `bands` to the next, so an
/// outline can be drawn a part at a time. A circle's part depends on the
/// samples on either side and their neighbours.
pub fn pieces(
    samples: &[Sample],
    pen: &Pen,
    frame: u32,
    circles: std::ops::Range<usize>,
    bands: std::ops::Range<usize>,
) -> Pieces {
    let base = classic::width(f64::from(pen.width), pen.seed, frame, pen.wobble);
    let radius = |sample: &Sample| radius(base, pen, sample);
    let first = circles.start.saturating_sub(1).min(bands.start);
    let last = (circles.end + 1).max(bands.end + 1).min(samples.len());
    let centers: Vec<[f64; 2]> = (first..last)
        .map(|index| displaced_at(samples, index, pen, frame))
        .collect();
    let center = |index: usize| centers[index - first];

    let mut path = BezPath::new();
    let mut bounds = [
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    ];
    let mut grow = |[x, y]: [f64; 2], radius: f64| {
        bounds = [
            bounds[0].min(x - radius),
            bounds[1].min(y - radius),
            bounds[2].max(x + radius),
            bounds[3].max(y + radius),
        ];
    };
    for index in circles {
        let radius = radius(&samples[index]);
        grow(center(index), radius);
        let side = |other: usize| {
            let radii = [radius, self::radius(base, pen, &samples[other])];
            left_by_band(center(index), center(other), radii)
        };
        let keep = [
            index.checked_sub(1).and_then(side),
            (index + 1 < samples.len())
                .then(|| side(index + 1))
                .flatten(),
        ];
        cap(&mut path, center(index), radius, keep);
    }
    let reverse = *BANDS_REVERSED;
    for index in bands {
        let radii = [radius(&samples[index]), radius(&samples[index + 1])];
        let ends = [center(index), center(index + 1)];
        if let Some(mut corners) = band(ends[0], ends[1], radii) {
            grow(ends[0], radii[0]);
            grow(ends[1], radii[1]);
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
    Pieces { path, bounds }
}

/// A half plane `normal · (p − center) ≥ offset` around a circle's center.
type Half = ([f64; 2], f64);

/// The part of the circle at `a` that the band to the circle at `b` does
/// not cover: the side of the chord between the band's corners away from
/// `b`. The circle and band together are the hull of the two circles, so
/// this is all of the circle the band leaves. `None` when one circle
/// contains the other and there is no band.
fn left_by_band(a: [f64; 2], b: [f64; 2], [ra, rb]: [f64; 2]) -> Option<Half> {
    let length = distance(a, b);
    let k = (ra - rb) / length;
    (length > 0.0 && k.abs() < 1.0)
        .then(|| ([(a[0] - b[0]) / length, (a[1] - b[1]) / length], -ra * k))
}

fn radius(base: f64, pen: &Pen, sample: &Sample) -> f64 {
    (base * pen.pressure_scale(sample.pressure)).max(0.5) * 0.5
}

/// Appends the part of the circle at `center` inside each of `keep`, wound
/// like kurbo's circles.
fn cap(path: &mut BezPath, center: [f64; 2], radius: f64, keep: [Option<Half>; 2]) {
    let at = |[x, y]: [f64; 2]| kurbo::Point::new(center[0] + x, center[1] + y);
    // Where the edge of `half` crosses the circle: the kept arc runs
    // counterclockwise from the first to the second.
    let ends = |([x, y], offset): Half| {
        let along = (radius * radius - offset * offset).max(0.0).sqrt();
        [
            [offset * x + along * y, offset * y - along * x],
            [offset * x - along * y, offset * y + along * x],
        ]
    };
    let inside = |([x, y], offset): Half, [px, py]: [f64; 2]| x * px + y * py >= offset;
    let arc = |path: &mut BezPath, from: [f64; 2], to: [f64; 2]| {
        arc_to(path, center, radius, from, to);
    };
    match keep {
        [None, None] => {
            path.extend(Circle::new(at([0.0, 0.0]), radius).path_elements(TOLERANCE));
        }
        [Some(half), None] | [None, Some(half)] => {
            let [from, to] = ends(half);
            path.move_to(at(from));
            arc(path, from, to);
            path.close_path();
        }
        [Some(first), Some(second)] => {
            let [a0, a1] = ends(first);
            let [b0, b1] = ends(second);
            let (a0_in, a1_in) = (inside(second, a0), inside(second, a1));
            let (b0_in, b1_in) = (inside(first, b0), inside(first, b1));
            // Where the two edges cross, if they cross inside the circle.
            let corner = || {
                let (([ax, ay], ad), ([bx, by], bd)) = (first, second);
                let det = ax * by - ay * bx;
                [(ad * by - bd * ay) / det, (ax * bd - bx * ad) / det]
            };
            match (a0_in, a1_in) {
                (true, true) if b0_in && b1_in => {
                    // A band across the circle between the two edges.
                    path.move_to(at(a0));
                    arc(path, a0, b1);
                    path.line_to(at(b0));
                    arc(path, b0, a1);
                    path.close_path();
                }
                (true, true) => {
                    path.move_to(at(a0));
                    arc(path, a0, a1);
                    path.close_path();
                }
                (false, false) if b0_in && b1_in => {
                    path.move_to(at(b0));
                    arc(path, b0, b1);
                    path.close_path();
                }
                (false, false) => {}
                (false, true) => {
                    path.move_to(at(b0));
                    arc(path, b0, a1);
                    path.line_to(at(corner()));
                    path.close_path();
                }
                (true, false) => {
                    path.move_to(at(a0));
                    arc(path, a0, b1);
                    path.line_to(at(corner()));
                    path.close_path();
                }
            }
        }
    }
}

/// Appends the counterclockwise arc of the circle at `center` from `from` to
/// `to`, both relative to `center` and on the circle.
fn arc_to(path: &mut BezPath, center: [f64; 2], radius: f64, from: [f64; 2], to: [f64; 2]) {
    let point = |[x, y]: [f64; 2]| kurbo::Point::new(center[0] + x, center[1] + y);
    let cross = from[0] * to[1] - from[1] * to[0];
    let dot = from[0] * to[0] + from[1] * to[1];
    // Up to a quarter turn, one cubic is within the tolerance for any radius
    // a pen can have (its error is 2.7e-4 of the radius).
    if cross >= 0.0 && dot >= 0.0 && radius * 2.7e-4 <= TOLERANCE {
        let squared = radius * radius;
        // tan(θ/2), then tan(θ/4) for the control points' distance.
        let half = cross / (squared + dot);
        let quarter = half / (1.0 + (1.0 + half * half).sqrt());
        let k = 4.0 / 3.0 * quarter;
        path.curve_to(
            point([from[0] - k * from[1], from[1] + k * from[0]]),
            point([to[0] + k * to[1], to[1] - k * to[0]]),
            point(to),
        );
        return;
    }
    let start = from[1].atan2(from[0]);
    let sweep = cross.atan2(dot).rem_euclid(std::f64::consts::TAU);
    let arc = kurbo::Arc::new((center[0], center[1]), (radius, radius), start, sweep, 0.0);
    path.extend(arc.append_iter(TOLERANCE));
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

        // Both shapes overlap themselves a little (the outline where bands
        // meet inside a turn, the stroker at its joins), and Vello sums
        // overlapping coverage, so antialiased edge pixels differ by a few
        // levels. The shape is the same: no holes inside, nothing outside.
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
        assert!((0.0..8.0).contains(&mean), "edges {mean} darker on average");
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
    fn the_pieces_wind_the_same_way() {
        let points = wave(80, |index| 0.2 + (index % 9) as f32 / 10.0);
        let pen = pen(0.8, 6.0);
        let samples = Resampler::whole(&points, spacing(pen.width));
        let all = 0..samples.len();
        let path = pieces(&samples, &pen, 4, all.clone(), 0..samples.len() - 1).path;
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
        let whole = outline(&samples, &pen, 4).unwrap();
        assert_eq!(whole.area().signum(), signs[0]);
    }

    /// The union as M2 drew it: a circle per sample and a band per pair.
    fn circles_and_bands(samples: &[Sample], pen: &Pen, frame: u32) -> BezPath {
        let base = classic::width(f64::from(pen.width), pen.seed, frame, pen.wobble);
        let radius = |sample: &Sample| (base * pen.pressure_scale(sample.pressure)).max(0.5) * 0.5;
        let centers = displaced(samples, pen, frame);
        let mut path = BezPath::new();
        for (sample, &[x, y]) in samples.iter().zip(&centers) {
            path.extend(Circle::new((x, y), radius(sample)).path_elements(TOLERANCE));
        }
        for index in 0..samples.len() - 1 {
            let radii = [radius(&samples[index]), radius(&samples[index + 1])];
            if let Some(mut corners) = band(centers[index], centers[index + 1], radii) {
                if *BANDS_REVERSED {
                    corners.reverse();
                }
                path.move_to(corners[0]);
                for &corner in &corners[1..] {
                    path.line_to(corner);
                }
                path.close_path();
            }
        }
        path
    }

    /// How far `point` is inside the union of the circles at `centers` and
    /// the bands joining them, negative outside.
    fn depth(centers: &[[f64; 2]], radii: &[f64], point: [f64; 2]) -> f64 {
        let circles = centers
            .iter()
            .zip(radii)
            .map(|(&center, radius)| radius - distance(center, point));
        let bands = (0..centers.len() - 1).filter_map(|index| {
            let corners = band(
                centers[index],
                centers[index + 1],
                [radii[index], radii[index + 1]],
            )?;
            let turn = (corners[1] - corners[0])
                .cross(corners[2] - corners[1])
                .signum();
            let point = kurbo::Point::new(point[0], point[1]);
            Some(
                (0..4)
                    .map(|edge| {
                        let (a, b) = (corners[edge], corners[(edge + 1) % 4]);
                        turn * (b - a).cross(point - a) / (b - a).hypot()
                    })
                    .fold(f64::INFINITY, f64::min),
            )
        });
        circles.chain(bands).fold(f64::NEG_INFINITY, f64::max)
    }

    #[test]
    fn the_outline_fills_what_the_circles_and_bands_fill() {
        let at = |x: f32, y: f32, pressure: f32| Point { x, y, pressure };
        let zigzag: Vec<Point> = (0..24)
            .map(|index| {
                let y = if index % 2 == 0 { 30.0 } else { 90.0 };
                at(20.0 + index as f32 * 9.0, y, 0.8)
            })
            .collect();
        let hairpin = vec![
            at(30.0, 60.0, 0.6),
            at(200.0, 62.0, 0.9),
            at(40.0, 66.0, 0.4),
        ];
        // Some circles contain their neighbours.
        let jumps = wave(70, |index| [0.05, 1.0, 0.3, 0.95, 0.1][index % 5]);
        let spiral: Vec<Point> = (0..120)
            .map(|index| {
                let turn = index as f32 * 0.25;
                let reach = 4.0 + index as f32 * 0.45;
                let pressure = 0.5 + (turn * 0.7).sin() * 0.4;
                at(
                    128.0 + turn.cos() * reach,
                    64.0 + turn.sin() * reach * 0.8,
                    pressure,
                )
            })
            .collect();
        for points in [zigzag, hairpin, jumps, spiral] {
            for (width, dynamics, wobble) in [(12.0, 0.8, 1.6), (1.0, 0.0, 0.0), (40.0, 1.0, 12.0)]
            {
                let pen = Pen {
                    width,
                    ..pen(dynamics, wobble)
                };
                let samples = Resampler::whole(&points, spacing(pen.width));
                let traced = outline(&samples, &pen, 3).unwrap();
                let base = classic::width(f64::from(pen.width), pen.seed, 3, pen.wobble);
                let centers = displaced(&samples, &pen, 3);
                let radii: Vec<f64> = samples
                    .iter()
                    .map(|sample| radius(base, &pen, sample))
                    .collect();
                let before = rasterize(&circles_and_bands(&samples, &pen, 3), None, [0, 0, 0, 255]);
                let after = rasterize(&traced, None, [0, 0, 0, 255]);
                // Vello adds up the coverage of overlapping pieces, so edge
                // pixels of the union come out darker than they are; the
                // outline overlaps itself far less.
                let (mut edges, mut error_before, mut error_after) = (0, 0.0, 0.0);
                for (index, (a, b)) in before.chunks(4).zip(after.chunks(4)).enumerate() {
                    let (a, b) = (a[3], b[3]);
                    if a == b && (a == 0 || a == 255) {
                        continue;
                    }
                    let [x, y] = [(index % 256) as f64, (index / 256) as f64];
                    // Away from the edge by more than curves are flattened,
                    // the outline is inside exactly where the union is.
                    let center = [x + 0.5, y + 0.5];
                    let inside = depth(&centers, &radii, center);
                    if inside.abs() > TOLERANCE {
                        let point = kurbo::Point::new(center[0], center[1]);
                        assert_eq!(
                            traced.winding(point) != 0,
                            inside > 0.0,
                            "at {center:?}, width {width}"
                        );
                    }
                    let covered = (0..16)
                        .filter(|sub| {
                            let [sx, sy] = [f64::from(sub % 4), f64::from(sub / 4)];
                            let point = [x + (sx + 0.5) / 4.0, y + (sy + 0.5) / 4.0];
                            depth(&centers, &radii, point) > 0.0
                        })
                        .count();
                    let truth = covered as f64 / 16.0 * 255.0;
                    edges += 1;
                    error_before += (f64::from(a) - truth).abs();
                    error_after += (f64::from(b) - truth).abs();
                }
                assert!(edges > 0);
                assert!(
                    error_after < error_before,
                    "width {width}: {error_after} against {error_before}"
                );
            }
        }
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

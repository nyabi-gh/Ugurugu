// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! Airbrush and spray strokes: a dab at every sample, or a few particles
//! around it, each laid over what is there, so where they overlap they add
//! up. The shapes, sizes, alphas and particle positions are 2.2.13's
//! (`drawAirbrushDab`, `drawSprayDab`); the samples move with the stroke's
//! motion like a pen's.
//!
//! A stroke's dabs are painted here into a buffer of its own rather than
//! drawn as Vello paths, which costs about half (docs/rust/m4-plan.md M4-6):
//! a path has a fixed cost that a one-pixel particle never pays back, and
//! Vello makes a gradient table for every soft dab. Aliased pixels are those
//! whose centres the shape covers, as Qt paints them for 2.2.13.

use crate::stroke::{self, Pen, Sample};
use ugu_core::motion::noise;
use ugu_core::store::{Brush, BrushEngine, Point, TipShape};

/// Samples of an airbrush or spray stroke, as in 2.2.13.
pub const MAX_DABS: usize = 50_000;
const MAX_PARTICLES: usize = 250_000;
/// Steps of the soft falloff, by squared distance from the middle.
const FALLOFF_STEPS: usize = 4096;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Dab {
    pub center: [f64; 2],
    pub diameter: f64,
    pub alpha: u8,
}

/// How all the dabs of a stroke are drawn.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Look {
    Solid {
        square: bool,
        antialias: bool,
    },
    /// Solid to `hardness` of the radius, then fading to nothing.
    Soft {
        hardness: f64,
    },
}

pub fn look(brush: &Brush) -> Look {
    let square = brush.tip == TipShape::Square;
    match brush.engine {
        BrushEngine::Airbrush if !square && brush.hardness < 0.995 => Look::Soft {
            hardness: f64::from(brush.hardness).clamp(0.0, 0.98),
        },
        BrushEngine::Spray => Look::Solid {
            square,
            antialias: false,
        },
        _ => Look::Solid {
            square,
            antialias: brush.antialias,
        },
    }
}

/// 2.2.13's `colorWithOpacity`: `alpha` scaled and rounded.
fn scaled(alpha: u8, factor: f64) -> u8 {
    (f64::from(alpha) * factor.clamp(0.0, 1.0)).round() as u8
}

/// A whole stroke's dabs on `frame` into `out`, for a stroke whose colour
/// has `alpha`, sampled as the renderer samples them.
pub fn stroke_dabs(points: &[Point], pen: &Pen, frame: u32, alpha: u8, out: &mut Vec<Dab>) {
    let samples = stroke::Resampler::at_most(points, pen.dab_spacing(frame), MAX_DABS);
    dabs_into(&samples, pen, frame, alpha, out);
}

/// The dabs of `samples` on `frame` into `out`, for a stroke whose colour
/// has `alpha`. Dabs that would paint nothing are left out.
pub fn dabs_into(samples: &[Sample], pen: &Pen, frame: u32, alpha: u8, out: &mut Vec<Dab>) {
    out.clear();
    dabs_of(samples, 0..samples.len(), pen, frame, alpha, out);
}

/// The dabs of the samples in `range` added to `out`. A sample's dabs
/// depend on the samples on either side of it, which give its direction.
pub fn dabs_of(
    samples: &[Sample],
    range: std::ops::Range<usize>,
    pen: &Pen,
    frame: u32,
    alpha: u8,
    out: &mut Vec<Dab>,
) {
    let brush = &pen.brush;
    let base = ugu_core::motion::classic::width(f64::from(pen.width), pen.seed, frame, pen.wobble);
    let alpha_at = |pressure: f64| {
        let fade = stroke::pressure_scale(brush.opacity_dynamics, pressure);
        scaled(alpha, f64::from(brush.opacity * brush.flow) * fade)
    };
    let center = |index: usize| stroke::displaced_at(samples, index, pen, frame);
    match brush.engine {
        BrushEngine::Airbrush => {
            out.extend(range.filter_map(|index| {
                let sample = &samples[index];
                let alpha = alpha_at(sample.pressure);
                (alpha > 0).then(|| Dab {
                    center: center(index),
                    diameter: (base * pen.pressure_scale(sample.pressure)).max(0.5),
                    alpha,
                })
            }));
        }
        BrushEngine::Spray => {
            let count = ((f64::from(brush.density) * 6.0).round() as usize).clamp(1, 24);
            let still = if brush.animated_jitter && brush.wobble_scale > 0.0 {
                frame
            } else {
                0
            };
            let scatter = f64::from(brush.scatter) * base * 0.5;
            let jitter = f64::from(brush.size_jitter) * 0.75;
            for index in range {
                let sample = &samples[index];
                let alpha = alpha_at(sample.pressure);
                if index * count >= MAX_PARTICLES {
                    return;
                }
                if alpha == 0 {
                    continue;
                }
                let [x, y] = center(index);
                let size =
                    base * f64::from(brush.particle_size) * pen.pressure_scale(sample.pressure);
                for particle in 0..count {
                    let emitted = index * count + particle;
                    if emitted >= MAX_PARTICLES {
                        return;
                    }
                    let at = emitted as i64;
                    let angle =
                        noise::unit(pen.seed, still, at, 0x36d1_a53b) * std::f64::consts::TAU;
                    let radius = noise::unit(pen.seed, still, at, 0x9c8e_31d7).sqrt() * scatter;
                    let scale = 1.0 + noise::signed(pen.seed, still, at, 0xa24b_aed4) * jitter;
                    out.push(Dab {
                        center: [x + angle.cos() * radius, y + angle.sin() * radius],
                        diameter: (size * scale.max(0.1)).max(0.5),
                        alpha,
                    });
                }
            }
        }
        BrushEngine::Line => {}
    }
}

/// Paints dabs over each other into a premultiplied RGBA buffer.
#[derive(Default)]
pub struct Painter {
    /// The soft falloff for `falloff_of`'s hardness: how much of the dab's
    /// alpha is left, out of 255, by squared distance.
    falloff: Vec<u8>,
    falloff_of: Option<u64>,
}

impl Painter {
    /// Paints `dabs`, given in the buffer's pixels, in `rgb` over what
    /// `buffer` holds; it is `width` pixels wide.
    pub fn paint(
        &mut self,
        buffer: &mut [u8],
        width: usize,
        dabs: &[Dab],
        look: Look,
        rgb: [u8; 3],
    ) {
        let height = buffer.len() / 4 / width.max(1);
        let size = [width, height];
        match look {
            Look::Solid {
                square: false,
                antialias: false,
            } => dabs.iter().for_each(|dab| round(buffer, size, dab, rgb)),
            Look::Solid {
                square: true,
                antialias: false,
            } => dabs.iter().for_each(|dab| square(buffer, size, dab, rgb)),
            Look::Solid {
                square: false,
                antialias: true,
            } => dabs
                .iter()
                .for_each(|dab| smooth_round(buffer, size, dab, rgb)),
            Look::Solid {
                square: true,
                antialias: true,
            } => dabs
                .iter()
                .for_each(|dab| smooth_square(buffer, size, dab, rgb)),
            Look::Soft { hardness } => {
                self.prepare(hardness);
                for dab in dabs {
                    soft(buffer, size, dab, rgb, &self.falloff);
                }
            }
        }
    }

    /// 2.2.13's radial gradient: the full alpha to `hardness` of the radius,
    /// then linearly to none at the edge.
    fn prepare(&mut self, hardness: f64) {
        if self.falloff_of == Some(hardness.to_bits()) {
            return;
        }
        self.falloff_of = Some(hardness.to_bits());
        self.falloff.clear();
        self.falloff.extend((0..FALLOFF_STEPS).map(|step| {
            let t = ((step as f64 + 0.5) / FALLOFF_STEPS as f64).sqrt();
            let left = if t <= hardness {
                1.0
            } else {
                1.0 - (t - hardness) / (1.0 - hardness)
            };
            (left * 255.0).round() as u8
        }));
    }
}

/// `a × b / 255`, rounded.
#[inline]
fn mul(a: u16, b: u16) -> u16 {
    let value = a * b + 128;
    (value + (value >> 8)) >> 8
}

/// `rgb` at `alpha`, premultiplied.
#[inline]
fn premultiplied([r, g, b]: [u8; 3], alpha: u16) -> [u16; 4] {
    [
        mul(r.into(), alpha),
        mul(g.into(), alpha),
        mul(b.into(), alpha),
        alpha,
    ]
}

/// `source` over every pixel of `run`, a byte at a time so that it
/// vectorises.
#[inline]
fn over_run(run: &mut [u8], source: [u16; 4]) {
    let left = 255 - source[3];
    let pattern: [u16; 16] = std::array::from_fn(|index| source[index & 3]);
    let (chunks, rest) = run.as_chunks_mut::<16>();
    for chunk in chunks {
        for (byte, add) in chunk.iter_mut().zip(pattern) {
            *byte = (add + mul((*byte).into(), left)) as u8;
        }
    }
    for (byte, add) in rest.iter_mut().zip(pattern) {
        *byte = (add + mul((*byte).into(), left)) as u8;
    }
}

/// `source` over one pixel.
#[inline]
fn over(pixel: &mut [u8], source: [u16; 4]) {
    let left = 255 - source[3];
    for (byte, add) in pixel.iter_mut().zip(source) {
        *byte = (add + mul((*byte).into(), left)) as u8;
    }
}

/// The rows from `top` to `bottom` that lie in `height`.
fn rows(top: f64, bottom: f64, height: usize) -> std::ops::Range<usize> {
    let first = top.floor().max(0.0) as usize;
    let last = bottom.ceil().clamp(0.0, height as f64) as usize;
    first..last.max(first)
}

/// The pixels whose centres are within `left..=right`, clamped to `width`.
fn centres(left: f64, right: f64, width: usize) -> std::ops::Range<usize> {
    let first = (left - 0.5).ceil().max(0.0);
    let last = ((right - 0.5).floor() + 1.0).min(width as f64);
    if last <= first {
        return 0..0;
    }
    first as usize..last as usize
}

fn row(buffer: &mut [u8], width: usize, y: usize) -> &mut [u8] {
    &mut buffer[y * width * 4..(y + 1) * width * 4]
}

fn round(buffer: &mut [u8], [width, height]: [usize; 2], dab: &Dab, rgb: [u8; 3]) {
    let radius = dab.diameter * 0.5;
    let [x, y] = dab.center;
    let source = premultiplied(rgb, dab.alpha.into());
    for line in rows(y - radius, y + radius, height) {
        let dy = line as f64 + 0.5 - y;
        let reach = radius * radius - dy * dy;
        if reach < 0.0 {
            continue;
        }
        let half = reach.sqrt();
        let span = centres(x - half, x + half, width);
        over_run(
            &mut row(buffer, width, line)[span.start * 4..span.end * 4],
            source,
        );
    }
}

fn square(buffer: &mut [u8], [width, height]: [usize; 2], dab: &Dab, rgb: [u8; 3]) {
    let half = dab.diameter * 0.5;
    let [x, y] = dab.center;
    let source = premultiplied(rgb, dab.alpha.into());
    let span = centres(x - half, x + half, width);
    for line in centres(y - half, y + half, height) {
        over_run(
            &mut row(buffer, width, line)[span.start * 4..span.end * 4],
            source,
        );
    }
}

/// A round dab whose edge pixels are covered by how far inside it their
/// centres are, up to half a pixel either way.
fn smooth_round(buffer: &mut [u8], [width, height]: [usize; 2], dab: &Dab, rgb: [u8; 3]) {
    let radius = dab.diameter * 0.5;
    let [x, y] = dab.center;
    let alpha = u16::from(dab.alpha);
    let full = premultiplied(rgb, alpha);
    // A dab under a pixel covers at most its own area.
    let most = (std::f64::consts::PI * radius * radius).min(1.0);
    let outer = radius + 0.5;
    let inner = radius - 0.5;
    for line in rows(y - outer, y + outer, height) {
        let dy = line as f64 + 0.5 - y;
        let reach = outer * outer - dy * dy;
        if reach <= 0.0 {
            continue;
        }
        let span = centres(x - reach.sqrt(), x + reach.sqrt(), width);
        let inside = if inner > 0.0 && inner * inner - dy * dy >= 0.0 && most >= 1.0 {
            let half = (inner * inner - dy * dy).sqrt();
            let inside = centres(x - half, x + half, width);
            if inside.is_empty() {
                span.start..span.start
            } else {
                inside
            }
        } else {
            span.start..span.start
        };
        let pixels = row(buffer, width, line);
        over_run(&mut pixels[inside.start * 4..inside.end * 4], full);
        for column in span.clone().filter(|column| !inside.contains(column)) {
            let dx = column as f64 + 0.5 - x;
            let covered = (outer - dx.hypot(dy)).clamp(0.0, most);
            let alpha = (f64::from(alpha) * covered).round() as u16;
            if alpha > 0 {
                over(
                    &mut pixels[column * 4..column * 4 + 4],
                    premultiplied(rgb, alpha),
                );
            }
        }
    }
}

/// How much of the pixel from `start` to `start + 1` lies within
/// `low..high`.
fn overlap(start: f64, low: f64, high: f64) -> f64 {
    ((start + 1.0).min(high) - start.max(low)).max(0.0)
}

/// A square dab whose edge pixels are covered by the area of them it covers.
fn smooth_square(buffer: &mut [u8], [width, height]: [usize; 2], dab: &Dab, rgb: [u8; 3]) {
    let half = dab.diameter * 0.5;
    let [x, y] = dab.center;
    let alpha = f64::from(dab.alpha);
    let [left, right, top, bottom] = [x - half, x + half, y - half, y + half];
    let columns = rows(left, right, width);
    for line in rows(top, bottom, height) {
        let tall = overlap(line as f64, top, bottom);
        let pixels = row(buffer, width, line);
        for column in columns.clone() {
            let covered = tall * overlap(column as f64, left, right);
            let alpha = (alpha * covered).round() as u16;
            if alpha > 0 {
                over(
                    &mut pixels[column * 4..column * 4 + 4],
                    premultiplied(rgb, alpha),
                );
            }
        }
    }
}

fn soft(buffer: &mut [u8], [width, height]: [usize; 2], dab: &Dab, rgb: [u8; 3], falloff: &[u8]) {
    let radius = dab.diameter * 0.5;
    let [x, y] = dab.center;
    let alpha = u16::from(dab.alpha);
    let step = FALLOFF_STEPS as f64 / (radius * radius);
    let columns = rows(x - radius, x + radius, width);
    for line in rows(y - radius, y + radius, height) {
        let dy = line as f64 + 0.5 - y;
        let dy2 = dy * dy;
        let pixels = row(buffer, width, line);
        for column in columns.clone() {
            let dx = column as f64 + 0.5 - x;
            let Some(&left) = falloff.get(((dx * dx + dy2) * step) as usize) else {
                continue;
            };
            let alpha = mul(alpha, left.into());
            if alpha > 0 {
                over(
                    &mut pixels[column * 4..column * 4 + 4],
                    premultiplied(rgb, alpha),
                );
            }
        }
    }
}

/// Left, top, right, bottom of `dabs`; empty (inverted) without any.
pub fn bounds(dabs: &[Dab]) -> [f64; 4] {
    dabs.iter().fold(
        [
            f64::INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NEG_INFINITY,
        ],
        |[left, top, right, bottom], dab| {
            // A square's corner at most, and the half pixel an antialiased
            // edge covers.
            let reach = dab.diameter * std::f64::consts::FRAC_1_SQRT_2 + 0.5;
            let [x, y] = dab.center;
            [
                left.min(x - reach),
                top.min(y - reach),
                right.max(x + reach),
                bottom.max(y + reach),
            ]
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use ugu_core::brush::find;
    use ugu_core::store::Point as Input;

    fn pen(preset: &str, wobble: f64) -> Pen {
        let preset = find(preset).unwrap();
        Pen {
            width: preset.size,
            brush: preset.brush,
            seed: 0x1234_5678_9abc_def0,
            wobble,
        }
    }

    fn line() -> Vec<Input> {
        [(24.0, 48.0, 0.45), (104.0, 48.0, 1.0)]
            .map(|(x, y, pressure)| Input { x, y, pressure })
            .to_vec()
    }

    fn dabs(pen: &Pen, frame: u32) -> Vec<Dab> {
        let samples = stroke::Resampler::at_most(&line(), pen.dab_spacing(frame), MAX_DABS);
        let mut out = Vec::new();
        dabs_into(&samples, pen, frame, 255, &mut out);
        out
    }

    #[test]
    fn airbrush_dabs_follow_2_2_13() {
        let pen = pen("soft-airbrush", 0.0);
        let dabs = dabs(&pen, 0);
        // 64 × 0.09 = 5.76 apart over 80 pixels, and the last point.
        assert_eq!(dabs.len(), 15);
        // Pressure 0.45: 64 × (0.85 + 0.45 × 0.15); alpha 255 × 0.9 × 0.11 ×
        // (0.25 + 0.45 × 0.75).
        assert!((dabs[0].diameter - 58.72).abs() < 1e-4);
        assert_eq!(dabs[0].alpha, 15);
        assert_eq!(dabs.last().unwrap().alpha, 25);
        assert_eq!(look(&pen.brush), Look::Soft { hardness: 0.0 });
    }

    #[test]
    fn spray_particles_stay_put_unless_animated() {
        let still = pen("pixel-spray", 0.0);
        assert!(dabs(&still, 0) == dabs(&still, 1));
        let animated = pen("wobble-spray", 0.0);
        assert!(dabs(&animated, 0) != dabs(&animated, 1));
        let mut no_motion = animated;
        no_motion.brush.wobble_scale = 0.0;
        assert!(dabs(&no_motion, 0) == dabs(&no_motion, 1));
        // round(1.2 × 6) particles per sample.
        assert_eq!(dabs(&animated, 0).len() % 7, 0);
    }

    /// `dab` alone on a clear buffer of `size`.
    fn painted(dab: Dab, look: Look, size: usize) -> Vec<u8> {
        let mut buffer = vec![0; size * size * 4];
        Painter::default().paint(&mut buffer, size, &[dab], look, [255, 255, 255]);
        buffer
    }

    fn alphas(buffer: &[u8]) -> Vec<u8> {
        buffer.chunks(4).map(|pixel| pixel[3]).collect()
    }

    #[test]
    fn aliased_dabs_paint_the_pixels_whose_centres_they_cover() {
        let dab = Dab {
            center: [9.3, 8.6],
            diameter: 11.0,
            alpha: 200,
        };
        for square in [false, true] {
            let look = Look::Solid {
                square,
                antialias: false,
            };
            let buffer = painted(dab, look, 20);
            for (index, alpha) in alphas(&buffer).into_iter().enumerate() {
                let [dx, dy] = [
                    (index % 20) as f64 + 0.5 - 9.3,
                    (index / 20) as f64 + 0.5 - 8.6,
                ];
                let inside = if square {
                    dx.abs() <= 5.5 && dy.abs() <= 5.5
                } else {
                    dx.hypot(dy) <= 5.5
                };
                assert_eq!(alpha, if inside { 200 } else { 0 }, "{index} {square}");
            }
        }
    }

    #[test]
    fn antialiased_dabs_cover_about_their_area() {
        for (square, area) in [(false, std::f64::consts::PI * 30.25), (true, 121.0)] {
            let dab = Dab {
                center: [10.25, 9.7],
                diameter: 11.0,
                alpha: 255,
            };
            let look = Look::Solid {
                square,
                antialias: true,
            };
            let alphas = alphas(&painted(dab, look, 22));
            let covered: f64 = alphas.iter().map(|&alpha| f64::from(alpha) / 255.0).sum();
            assert!(
                (covered - area).abs() < area * 0.01,
                "{square} {covered} {area}"
            );
            assert!(alphas.iter().any(|&alpha| alpha > 0 && alpha < 255));
        }
    }

    #[test]
    fn a_soft_dab_keeps_its_alpha_to_the_hardness_then_fades() {
        let dab = Dab {
            center: [16.0, 16.0],
            diameter: 30.0,
            alpha: 240,
        };
        let alphas = alphas(&painted(dab, Look::Soft { hardness: 0.4 }, 32));
        let row: Vec<u8> = alphas[16 * 32 + 16..16 * 32 + 32].to_vec();
        // Pixel centres 0.5 to 5.5 from the middle are within 0.4 × 15.
        assert!(row[..6].iter().all(|&alpha| alpha == 240), "{row:?}");
        assert!(
            row[6..].windows(2).all(|pair| pair[0] >= pair[1]),
            "{row:?}"
        );
        // Past the hardness, what is left falls linearly to the edge.
        let t = 11.5f64.hypot(0.5) / 15.0;
        let expected = (240.0 * (1.0 - (t - 0.4) / 0.6)).round() as u8;
        assert!(row[11].abs_diff(expected) <= 2, "{row:?} {expected}");
        assert_eq!(row[15], 0);
        assert_eq!(alphas[0], 0);
    }

    #[test]
    fn overlapping_dabs_add_up_like_source_over() {
        let dab = Dab {
            center: [4.0, 4.0],
            diameter: 6.0,
            alpha: 128,
        };
        let look = Look::Solid {
            square: false,
            antialias: false,
        };
        let mut buffer = vec![0; 8 * 8 * 4];
        Painter::default().paint(&mut buffer, 8, &[dab, dab], look, [255, 0, 0]);
        // 128 + 128 × 127 / 255.
        assert_eq!(
            &buffer[(4 * 8 + 4) * 4..(4 * 8 + 4) * 4 + 4],
            [192, 0, 0, 192]
        );
    }

    #[test]
    fn particles_land_within_the_scatter() {
        let pen = pen("rough-spray", 0.0);
        let reach = f64::from(pen.width) * 1.1 * 0.5;
        let samples = stroke::Resampler::at_most(&line(), pen.dab_spacing(0), MAX_DABS);
        let dabs = dabs(&pen, 0);
        for (index, dab) in dabs.iter().enumerate() {
            let [x, y] = samples[index / 6].position;
            let away = (dab.center[0] - x).hypot(dab.center[1] - y);
            assert!(away <= reach + 1e-9, "{away}");
        }
    }
}

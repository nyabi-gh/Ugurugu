// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! Airbrush and spray strokes: a dab at every sample, or a few particles
//! around it, each laid over what is there, so where they overlap they add
//! up. The shapes, sizes, alphas and particle positions are 2.2.13's
//! (`drawAirbrushDab`, `drawSprayDab`); the samples move with the stroke's
//! motion like a pen's.

use ugu_core::motion::noise;
use ugu_core::store::{Brush, BrushEngine, TipShape};
use vello_cpu::RenderContext;
use vello_cpu::color::AlphaColor;
use vello_cpu::kurbo::{BezPath, Circle, Point, Rect, Shape};
use vello_cpu::peniko::{ColorStop, Gradient};

use crate::stroke::{self, Pen, Sample};

/// Samples of an airbrush or spray stroke, as in 2.2.13.
pub const MAX_DABS: usize = 50_000;
const MAX_PARTICLES: usize = 250_000;
/// Curve flattening tolerance, in pixels.
const TOLERANCE: f64 = 0.05;
/// The coverage over which an aliased dab paints a pixel.
const ALIASING_THRESHOLD: u8 = 128;

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

/// The dabs of `samples` on `frame` into `out`, for a stroke whose colour
/// has `alpha`. Dabs that would paint nothing are left out.
pub fn dabs_into(samples: &[Sample], pen: &Pen, frame: u32, alpha: u8, out: &mut Vec<Dab>) {
    out.clear();
    if samples.is_empty() {
        return;
    }
    let brush = &pen.brush;
    let base = ugu_core::motion::classic::width(f64::from(pen.width), pen.seed, frame, pen.wobble);
    let centers = stroke::displaced(samples, pen, frame);
    let alpha_at = |pressure: f64| {
        let fade = stroke::pressure_scale(brush.opacity_dynamics, pressure);
        scaled(alpha, f64::from(brush.opacity * brush.flow) * fade)
    };
    match brush.engine {
        BrushEngine::Airbrush => {
            out.extend(
                samples
                    .iter()
                    .zip(&centers)
                    .filter_map(|(sample, &center)| {
                        let alpha = alpha_at(sample.pressure);
                        (alpha > 0).then(|| Dab {
                            center,
                            diameter: (base * pen.pressure_scale(sample.pressure)).max(0.5),
                            alpha,
                        })
                    }),
            );
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
            for (index, (sample, &[x, y])) in samples.iter().zip(&centers).enumerate() {
                let alpha = alpha_at(sample.pressure);
                let size =
                    base * f64::from(brush.particle_size) * pen.pressure_scale(sample.pressure);
                for particle in 0..count {
                    let emitted = index * count + particle;
                    if emitted >= MAX_PARTICLES {
                        return;
                    }
                    if alpha == 0 {
                        continue;
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

/// Draws `dabs` in `rgb`, one after another, in the current transform.
/// `scratch` is reused for their outlines.
pub fn draw(
    context: &mut RenderContext,
    dabs: &[Dab],
    look: Look,
    [r, g, b]: [u8; 3],
    scratch: &mut BezPath,
) {
    match look {
        Look::Solid { square, antialias } => {
            context.set_aliasing_threshold((!antialias).then_some(ALIASING_THRESHOLD));
            for dab in dabs {
                context.set_paint(AlphaColor::from_rgba8(r, g, b, dab.alpha));
                let [x, y] = dab.center;
                let half = dab.diameter * 0.5;
                if square {
                    context.fill_rect(&Rect::new(x - half, y - half, x + half, y + half));
                } else {
                    circle(scratch, dab);
                    context.fill_path(scratch);
                }
            }
            context.set_aliasing_threshold(None);
        }
        Look::Soft { hardness } => {
            for dab in dabs {
                let color = AlphaColor::from_rgba8(r, g, b, dab.alpha);
                let clear = AlphaColor::from_rgba8(r, g, b, 0);
                let [x, y] = dab.center;
                let mut stops = vec![ColorStop::from((0.0, color))];
                if hardness > 0.001 {
                    stops.push(ColorStop::from((hardness as f32, color)));
                }
                stops.push(ColorStop::from((1.0, clear)));
                let gradient = Gradient::new_radial(Point::new(x, y), (dab.diameter * 0.5) as f32)
                    .with_stops(stops.as_slice());
                context.set_paint(gradient);
                circle(scratch, dab);
                context.fill_path(scratch);
            }
        }
    }
}

/// `dab`'s circle into `path`, replacing what it held.
fn circle(path: &mut BezPath, dab: &Dab) {
    let [x, y] = dab.center;
    path.truncate(0);
    path.extend(Circle::new((x, y), dab.diameter * 0.5).path_elements(TOLERANCE));
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
            // A square's corner, at most.
            let reach = dab.diameter * std::f64::consts::FRAC_1_SQRT_2;
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

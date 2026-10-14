// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! The shape tool: a line, rectangle, ellipse, regular polygon or star
//! dragged out and drawn as one ordinary stroke along its outline, with the
//! brush, width and colour of the pen, so it wobbles and undoes as a stroke.

use std::f64::consts::{FRAC_PI_2, FRAC_PI_4, TAU};

use ugu_core::edit::{EditError, Outcome};
use ugu_core::store;

use crate::{InputPoint, Session, StrokeRefused, Tools, point};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Shape {
    Line,
    #[default]
    Rectangle,
    Ellipse,
    Polygon,
    Star,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShapeSettings {
    pub shape: Shape,
    /// A polygon's sides.
    pub sides: u32,
    /// A star's points.
    pub points: u32,
}

impl ShapeSettings {
    pub const DEFAULT: Self = Self {
        shape: Shape::Rectangle,
        sides: 3,
        points: 5,
    };
    pub const CORNERS: std::ops::RangeInclusive<u32> = 3..=12;
}

/// How a drag is read: Shift keeps a square box or a line at a multiple of
/// 45°, Alt grows the shape from where the drag began.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Drag {
    pub constrain: bool,
    pub centred: bool,
}

/// Outline points are at most this far apart, as a quick hand drawing them,
/// so that the wobble bends edges as it bends strokes.
const STEP: f64 = 2.0;
/// A star's inner corners, as a share of the outer ones' distance from the
/// centre.
const STAR_INNER: f64 = 0.5;

/// A drag in progress, kept with the stroke it draws.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Shaping {
    settings: ShapeSettings,
    start: [f64; 2],
    pixel: bool,
}

/// The outline of `settings`' shape dragged from `start` to `end`, as the
/// points of one stroke; closed shapes end where they start. Empty when the
/// drag is too small to draw anything.
pub fn outline(
    settings: ShapeSettings,
    start: [f64; 2],
    end: [f64; 2],
    drag: Drag,
) -> Vec<[f64; 2]> {
    let mut delta = [end[0] - start[0], end[1] - start[1]];
    if settings.shape == Shape::Line {
        if drag.constrain {
            // Along the nearest multiple of 45°, keeping the drag's reach
            // along it, so that a drag between pixel centres ends on one.
            let steps = (delta[1].atan2(delta[0]) / FRAC_PI_4).round() as i32;
            delta = match steps.rem_euclid(4) {
                0 => [delta[0], 0.0],
                2 => [0.0, delta[1]],
                _ => {
                    let reach = delta[0].abs().max(delta[1].abs());
                    [reach * delta[0].signum(), reach * delta[1].signum()]
                }
            };
        }
        let from = if drag.centred {
            [start[0] - delta[0], start[1] - delta[1]]
        } else {
            start
        };
        let to = [start[0] + delta[0], start[1] + delta[1]];
        if (to[0] - from[0]).hypot(to[1] - from[1]) < 1.0 {
            return Vec::new();
        }
        return dense(&[from, to]);
    }
    if drag.constrain {
        let side = delta[0].abs().max(delta[1].abs());
        delta = delta.map(|value| if value < 0.0 { -side } else { side });
    }
    let (low, high) = if drag.centred {
        (
            [start[0] - delta[0].abs(), start[1] - delta[1].abs()],
            [start[0] + delta[0].abs(), start[1] + delta[1].abs()],
        )
    } else {
        (
            [
                start[0].min(start[0] + delta[0]),
                start[1].min(start[1] + delta[1]),
            ],
            [
                start[0].max(start[0] + delta[0]),
                start[1].max(start[1] + delta[1]),
            ],
        )
    };
    if high[0] - low[0] < 1.0 && high[1] - low[1] < 1.0 {
        return Vec::new();
    }
    let corners: Vec<[f64; 2]> = match settings.shape {
        Shape::Line => unreachable!("handled above"),
        Shape::Rectangle => vec![low, [high[0], low[1]], high, [low[0], high[1]]],
        Shape::Ellipse => {
            let radius = [(high[0] - low[0]) / 2.0, (high[1] - low[1]) / 2.0];
            // Pieces no longer than `STEP` even round the larger axis, a
            // whole number to each quarter so the outline reaches the box.
            let around = TAU * radius[0].max(radius[1]);
            let count = ((around / STEP / 4.0).ceil() as usize).max(4) * 4;
            return close(
                (0..count)
                    .map(|index| {
                        let angle = TAU * index as f64 / count as f64;
                        [
                            low[0] + radius[0] * (1.0 + angle.cos()),
                            low[1] + radius[1] * (1.0 + angle.sin()),
                        ]
                    })
                    .collect(),
            );
        }
        Shape::Polygon => fitted(&regular(settings.sides, 1.0), low, high),
        Shape::Star => fitted(&regular(settings.points, STAR_INNER), low, high),
    };
    dense(&close(corners))
}

/// The corners of a regular shape of `corners` points, the first straight
/// up, round a unit circle; with `inner` below 1, a star whose inner
/// corners are that far out.
fn regular(corners: u32, inner: f64) -> Vec<[f64; 2]> {
    let corners = corners.clamp(
        *ShapeSettings::CORNERS.start(),
        *ShapeSettings::CORNERS.end(),
    );
    let star = inner < 1.0;
    let count = if star { corners * 2 } else { corners };
    (0..count)
        .map(|index| {
            let angle = -FRAC_PI_2 + TAU * f64::from(index) / f64::from(count);
            let reach = if star && index % 2 == 1 { inner } else { 1.0 };
            [reach * angle.cos(), reach * angle.sin()]
        })
        .collect()
}

/// `points` stretched to fill the box from `low` to `high`.
fn fitted(points: &[[f64; 2]], low: [f64; 2], high: [f64; 2]) -> Vec<[f64; 2]> {
    let mut least = [f64::INFINITY; 2];
    let mut most = [f64::NEG_INFINITY; 2];
    for point in points {
        for axis in 0..2 {
            least[axis] = least[axis].min(point[axis]);
            most[axis] = most[axis].max(point[axis]);
        }
    }
    points
        .iter()
        .map(|point| {
            std::array::from_fn(|axis| {
                low[axis]
                    + (point[axis] - least[axis]) / (most[axis] - least[axis])
                        * (high[axis] - low[axis])
            })
        })
        .collect()
}

fn close(mut points: Vec<[f64; 2]>) -> Vec<[f64; 2]> {
    points.push(points[0]);
    points
}

/// `corners` joined by straight pieces of points at most `STEP` apart.
fn dense(corners: &[[f64; 2]]) -> Vec<[f64; 2]> {
    let mut points = vec![corners[0]];
    for pair in corners.windows(2) {
        let [from, to] = [pair[0], pair[1]];
        let steps = ((to[0] - from[0]).hypot(to[1] - from[1]) / STEP)
            .ceil()
            .max(1.0) as usize;
        points.extend((1..=steps).map(|step| {
            let along = step as f64 / steps as f64;
            [
                from[0] + (to[0] - from[0]) * along,
                from[1] + (to[1] - from[1]) * along,
            ]
        }));
    }
    points
}

/// In pixel mode a drag goes from pixel centre to pixel centre, so that
/// straight edges run along one row or column of pixels.
fn snapped(position: [f64; 2], pixel: bool) -> [f64; 2] {
    if pixel {
        position.map(|value| value.floor() + 0.5)
    } else {
        position
    }
}

impl Session {
    /// Starts dragging out the shape `tools` choose at `position`, drawn
    /// with the pen as `begin_stroke` draws.
    pub fn begin_shape(&mut self, tools: &Tools, position: [f64; 2]) -> Result<(), StrokeRefused> {
        let pixel = self.pixel_mode();
        let start = snapped(position, pixel);
        self.begin_stroke(
            tools,
            InputPoint {
                position: start,
                pressure: None,
                time: 0.0,
            },
        )?;
        let live = self.live.as_mut().expect("just begun");
        live.shaping = Some(Shaping {
            settings: tools.shape,
            start,
            pixel,
        });
        Ok(())
    }

    /// Drags the shape's far corner, or the line's end, to `position`;
    /// returns whether the outline changed.
    pub fn drag_shape(&mut self, position: [f64; 2], drag: Drag) -> bool {
        let Some(live) = self.live.as_mut() else {
            return false;
        };
        let Some(shaping) = live.shaping else {
            return false;
        };
        let end = snapped(position, shaping.pixel);
        // Dragged out on screen, then taken into the layer's own pixels.
        let to_layer = live.to_layer;
        let mut points: Vec<_> = outline(shaping.settings, shaping.start, end, drag)
            .into_iter()
            .map(|position| point(crate::clamp_position(crate::into(to_layer, position)), 1.0))
            .collect();
        points.truncate(store::limits::POINTS_PER_STROKE);
        if points.is_empty() {
            points.push(point(crate::into(to_layer, shaping.start), 1.0));
        }
        if points == live.points {
            return false;
        }
        live.points = points;
        live.replaced_last = true;
        true
    }

    /// Ends the drag at `position` and commits the shape, unless it is too
    /// small to draw anything.
    pub fn end_shape(&mut self, position: [f64; 2], drag: Drag) -> Result<Outcome, EditError> {
        self.drag_shape(position, drag);
        let Some(live) = self.live.take() else {
            return Ok(Outcome::NoChange);
        };
        if live.points.len() < 2 {
            self.drop_cel();
            return Ok(Outcome::NoChange);
        }
        self.commit(live)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bounds(points: &[[f64; 2]]) -> [f64; 4] {
        points.iter().fold(
            [
                f64::INFINITY,
                f64::INFINITY,
                f64::NEG_INFINITY,
                f64::NEG_INFINITY,
            ],
            |[left, top, right, bottom], [x, y]| {
                [left.min(*x), top.min(*y), right.max(*x), bottom.max(*y)]
            },
        )
    }

    fn settings(shape: Shape) -> ShapeSettings {
        ShapeSettings {
            shape,
            ..ShapeSettings::DEFAULT
        }
    }

    #[test]
    fn every_shape_fills_the_dragged_box_and_closed_ones_close() {
        let plain = Drag::default();
        for shape in [
            Shape::Rectangle,
            Shape::Ellipse,
            Shape::Polygon,
            Shape::Star,
        ] {
            let points = outline(settings(shape), [40.0, 30.0], [10.0, 90.0], plain);
            let [left, top, right, bottom] = bounds(&points);
            assert!(
                (left - 10.0).abs() < 1e-9 && (right - 40.0).abs() < 1e-9,
                "{shape:?}: {left} {right}"
            );
            assert!(
                (top - 30.0).abs() < 1e-3 && (bottom - 90.0).abs() < 1e-3,
                "{shape:?}"
            );
            assert_eq!(points.first(), points.last(), "{shape:?}");
            assert!(
                points.windows(2).all(|pair| (pair[1][0] - pair[0][0])
                    .hypot(pair[1][1] - pair[0][1])
                    <= STEP + 1e-9),
                "{shape:?}"
            );
        }
        let line = outline(settings(Shape::Line), [40.0, 30.0], [10.0, 90.0], plain);
        assert_eq!(
            (line[0], *line.last().unwrap()),
            ([40.0, 30.0], [10.0, 90.0])
        );
    }

    #[test]
    fn shift_squares_the_box_and_turns_lines_by_45_degrees_and_alt_grows_from_the_start() {
        let shift = Drag {
            constrain: true,
            centred: false,
        };
        let square = outline(settings(Shape::Ellipse), [10.0, 10.0], [40.0, 20.0], shift);
        assert_eq!(bounds(&square).map(f64::round), [10.0, 10.0, 40.0, 40.0]);
        for (to, end) in [
            ([30.0, 4.0], [30.0, 0.0]),
            ([-3.0, -25.0], [0.0, -25.0]),
            ([20.0, -17.0], [20.0, -20.0]),
            ([-12.0, 14.0], [-14.0, 14.0]),
        ] {
            let line = outline(settings(Shape::Line), [0.0, 0.0], to, shift);
            assert_eq!(*line.last().unwrap(), end, "{to:?}");
        }
        let alt = Drag {
            constrain: false,
            centred: true,
        };
        let centred = outline(settings(Shape::Rectangle), [50.0, 50.0], [60.0, 45.0], alt);
        assert_eq!(bounds(&centred), [40.0, 45.0, 60.0, 55.0]);
        let both = Drag {
            constrain: true,
            centred: true,
        };
        let line = outline(settings(Shape::Line), [50.0, 50.0], [50.0, 60.0], both);
        assert_eq!(
            (line[0], *line.last().unwrap()),
            ([50.0, 40.0], [50.0, 60.0])
        );
    }

    #[test]
    fn polygons_and_stars_have_the_corners_asked_for() {
        // Corners are where the outline turns.
        let turns = |points: &[[f64; 2]]| {
            let mut turns = 0;
            let count = points.len() - 1;
            for index in 0..count {
                let before = points[(index + count - 1) % count];
                let at = points[index];
                let after = points[index + 1];
                let cross = (at[0] - before[0]) * (after[1] - at[1])
                    - (at[1] - before[1]) * (after[0] - at[0]);
                if cross.abs() > 1e-6 {
                    turns += 1;
                }
            }
            turns
        };
        for sides in [3, 4, 6, 12] {
            let shape = ShapeSettings {
                shape: Shape::Polygon,
                sides,
                ..ShapeSettings::DEFAULT
            };
            let points = outline(shape, [0.0, 0.0], [100.0, 100.0], Drag::default());
            assert_eq!(turns(&points), sides as usize);
        }
        let star = ShapeSettings {
            shape: Shape::Star,
            points: 5,
            ..ShapeSettings::DEFAULT
        };
        assert_eq!(
            turns(&outline(star, [0.0, 0.0], [100.0, 100.0], Drag::default())),
            10
        );
    }

    #[test]
    fn a_drag_too_small_draws_nothing() {
        for shape in [
            Shape::Line,
            Shape::Rectangle,
            Shape::Ellipse,
            Shape::Polygon,
            Shape::Star,
        ] {
            assert!(outline(settings(shape), [5.0, 5.0], [5.4, 5.6], Drag::default()).is_empty());
        }
        // A flat box is a line's worth.
        assert!(
            !outline(
                settings(Shape::Rectangle),
                [5.0, 5.0],
                [25.0, 5.0],
                Drag::default()
            )
            .is_empty()
        );
    }
}

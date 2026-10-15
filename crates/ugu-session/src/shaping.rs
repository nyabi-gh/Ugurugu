// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! The shape tool: a line or a closed shape dragged out and drawn as one
//! ordinary stroke along its outline, with the brush, width and colour of
//! the pen, so it wobbles and undoes as a stroke. A closed shape can also
//! be filled, as the bucket fills: in the pen's colour, without wobbling,
//! under the outline when it has one.

use std::f64::consts::{FRAC_PI_2, FRAC_PI_4, PI, TAU};

use ugu_core::command;
use ugu_core::edit::{EditError, Outcome};
use ugu_core::ops::Rgba8;
use ugu_core::selection::{self, Selection};
use ugu_core::store;

use crate::{InputPoint, LiveStroke, Session, StrokeRefused, Tools, point, selecting};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Shape {
    Line,
    #[default]
    Rectangle,
    Ellipse,
    Polygon,
    Star,
    Parallelogram,
    Trapezoid,
    Semicircle,
    Moon,
}

impl Shape {
    /// Whether the outline ends where it starts, so the shape can be filled.
    pub fn closed(self) -> bool {
        self != Self::Line
    }
}

/// What a closed shape draws: its outline, the area inside, or both.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ShapePaint {
    #[default]
    Outline,
    Fill,
    Both,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShapeSettings {
    pub shape: Shape,
    /// A polygon's sides.
    pub sides: u32,
    /// A star's points.
    pub points: u32,
    /// How far out a star's inner corners are, in percent of the outer
    /// ones' distance from the centre.
    pub inner: u32,
    pub paint: ShapePaint,
}

impl ShapeSettings {
    pub const DEFAULT: Self = Self {
        shape: Shape::Rectangle,
        sides: 3,
        points: 5,
        inner: 50,
        paint: ShapePaint::Outline,
    };
    pub const CORNERS: std::ops::RangeInclusive<u32> = 3..=12;
    pub const INNER: std::ops::RangeInclusive<u32> = 10..=90;

    /// Whether the shape is drawn with a stroke along its outline.
    pub fn outlined(self) -> bool {
        !self.shape.closed() || self.paint != ShapePaint::Fill
    }

    /// Whether the area inside the shape is filled.
    pub fn filled(self) -> bool {
        self.shape.closed() && self.paint != ShapePaint::Outline
    }
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
/// A parallelogram's slant and how much narrower a trapezoid's top is than
/// its base, each as a share of the box's width.
const SLANT: f64 = 0.25;
/// The moon is a circle with one of this radius cut from it, this far to
/// the right of its centre, both in the first circle's radius.
const MOON_BITE: (f64, f64) = (0.85, 0.5);

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
        Shape::Star => {
            let inner = settings
                .inner
                .clamp(*ShapeSettings::INNER.start(), *ShapeSettings::INNER.end());
            fitted(
                &regular(settings.points, f64::from(inner) / 100.0),
                low,
                high,
            )
        }
        Shape::Parallelogram => {
            let slant = (high[0] - low[0]) * SLANT;
            vec![
                [low[0] + slant, low[1]],
                [high[0], low[1]],
                [high[0] - slant, high[1]],
                [low[0], high[1]],
            ]
        }
        Shape::Trapezoid => {
            let inset = (high[0] - low[0]) * SLANT / 2.0;
            vec![
                [low[0] + inset, low[1]],
                [high[0] - inset, low[1]],
                high,
                [low[0], high[1]],
            ]
        }
        Shape::Semicircle => {
            // The upper half of a circle on the box's bottom edge.
            let around = PI * (high[0] - low[0]).max(2.0 * (high[1] - low[1])) / 2.0;
            // Closing it draws the base.
            let arc = arc([0.0, 0.0], 1.0, PI, TAU, pieces(around));
            return dense(&close(fitted(&arc, low, high)));
        }
        Shape::Moon => {
            let (radius, offset) = MOON_BITE;
            // Where the two circles cross.
            let x = (offset * offset + 1.0 - radius * radius) / (2.0 * offset);
            let y = (1.0 - x * x).sqrt();
            let outer = y.atan2(x);
            let inner = y.atan2(x - offset);
            let around = PI * (high[0] - low[0]).max(high[1] - low[1]);
            let count = pieces(around);
            let mut points = arc([0.0, 0.0], 1.0, outer, TAU - outer, count);
            points.extend(arc([offset, 0.0], radius, TAU - inner, inner, count));
            return dense(&close(fitted(&points, low, high)));
        }
    };
    dense(&close(corners))
}

/// Enough pieces of at most `STEP` for an outline `around` long.
fn pieces(around: f64) -> usize {
    ((around / STEP).ceil() as usize).max(8)
}

/// `count` pieces of the circle round `centre` from angle `from` to `to`,
/// clockwise on screen when `to` is the larger.
fn arc(centre: [f64; 2], radius: f64, from: f64, to: f64, count: usize) -> Vec<[f64; 2]> {
    (0..=count)
        .map(|index| {
            let angle = from + (to - from) * index as f64 / count as f64;
            [
                centre[0] + radius * angle.cos(),
                centre[1] + radius * angle.sin(),
            ]
        })
        .collect()
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

/// `outline` grown by a hair, so that pixels whose centres lie on it are
/// filled too: in pixel mode its corners and straight edges run through
/// pixel centres, and a fill of only the centres strictly inside would leave
/// the right and bottom edge out and keep the left and top one.
fn with_edges(outline: &[[f64; 2]]) -> Vec<[f64; 2]> {
    const HAIR: f64 = 1e-6;
    let count = outline.len();
    // Twice the signed area: which way round the outline goes, and so which
    // side of each edge is outside.
    let turn: f64 = (0..count)
        .map(|index| {
            let [a, b] = [outline[index], outline[(index + 1) % count]];
            a[0] * b[1] - b[0] * a[1]
        })
        .sum();
    let outward = |[x, y]: [f64; 2]| {
        let length = x.hypot(y);
        if length == 0.0 {
            return [0.0, 0.0];
        }
        let normal = if turn > 0.0 { [y, -x] } else { [-y, x] };
        normal.map(|value| value / length)
    };
    (0..count)
        .map(|index| {
            let point = outline[index];
            let before = outline[(index + count - 1) % count];
            let after = outline[(index + 1) % count];
            let [a, b] = [
                outward([point[0] - before[0], point[1] - before[1]]),
                outward([after[0] - point[0], after[1] - point[1]]),
            ];
            let normal = [a[0] + b[0], a[1] + b[1]];
            let length = normal[0].hypot(normal[1]);
            if length < 1e-9 {
                return point;
            }
            [
                point[0] + normal[0] / length * HAIR,
                point[1] + normal[1] / length * HAIR,
            ]
        })
        .collect()
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
        // The eraser's end erases along the outline only.
        live.outlined = tools.shape.outlined() || live.erase;
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
        live.fill = (shaping.settings.filled() && !live.erase && points.len() > 2).then(|| {
            points
                .iter()
                .map(|point| [f64::from(point.x), f64::from(point.y)])
                .collect()
        });
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
        let pixel = self.pixel_mode();
        let area = live.fill.as_ref().and_then(|outline| {
            let outline = if pixel {
                with_edges(outline)
            } else {
                outline.clone()
            };
            Selection::of_shape(&selection::Shape::Freehand(outline), self.document().canvas)
        });
        match area {
            Some(area) => self.commit_filled(live, &area),
            None if live.outlined => self.commit(live),
            None => {
                self.drop_cel();
                Ok(Outcome::NoChange)
            }
        }
    }

    /// Fills `area` in the stroke's colour and opacity and draws the stroke
    /// over it when the shape is outlined, as one step.
    fn commit_filled(&mut self, live: LiveStroke, area: &Selection) -> Result<Outcome, EditError> {
        let brush = &live.template.brush;
        let antialias = brush.antialias && !self.pixel_mode();
        let [red, green, blue, alpha] = live.template.color.0;
        let alpha = (f32::from(alpha) * brush.opacity.clamp(0.0, 1.0)).round() as u8;
        let color = Rgba8([red, green, blue, alpha]);
        let stroke = live.outlined.then(|| store::Stroke {
            points: std::sync::Arc::from(live.points.as_slice()),
            ..live.template.clone()
        });
        let outcome = self.with_cel(|session| {
            session.history_mut()?.group("Draw", |group| {
                let clip = match &live.clip {
                    Some(selection) => Some(selecting::stored_mask(group, selection)?),
                    None => None,
                };
                let coverage = selecting::stored_mask(group, area)?;
                group.apply(|document| {
                    command::fill(
                        document, live.layer, coverage, color, antialias, clip, false,
                    )
                })?;
                match stroke {
                    Some(stroke) => group
                        .apply(|document| command::draw(document, live.layer, stroke, false, clip)),
                    None => Ok(()),
                }
            })
        })?;
        if matches!(outcome, Outcome::Committed(_)) {
            self.used.push(live.template.color);
        }
        Ok(outcome)
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

    const CLOSED: [Shape; 8] = [
        Shape::Rectangle,
        Shape::Ellipse,
        Shape::Polygon,
        Shape::Star,
        Shape::Parallelogram,
        Shape::Trapezoid,
        Shape::Semicircle,
        Shape::Moon,
    ];

    #[test]
    fn every_shape_fills_the_dragged_box_and_closed_ones_close() {
        let plain = Drag::default();
        for shape in CLOSED {
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
        // The inner corners as far out as asked, from the box's centre.
        let reach = |inner: u32| {
            let star = ShapeSettings { inner, ..star };
            outline(star, [0.0, 0.0], [100.0, 100.0], Drag::default())
                .iter()
                .map(|point| (point[0] - 50.0).hypot(point[1] - 50.0))
                .fold(f64::INFINITY, f64::min)
        };
        assert!(reach(20) < reach(50) && reach(50) < reach(80));
        for shape in [Shape::Parallelogram, Shape::Trapezoid] {
            assert_eq!(
                turns(&outline(
                    settings(shape),
                    [0.0, 0.0],
                    [100.0, 60.0],
                    Drag::default()
                )),
                4,
                "{shape:?}"
            );
        }
    }

    #[test]
    fn the_moon_and_the_semicircle_are_cut_where_expected() {
        let inside = |points: &[[f64; 2]], at: [f64; 2]| {
            let mut crossings = 0;
            for pair in points.windows(2) {
                let [a, b] = [pair[0], pair[1]];
                if (a[1] > at[1]) != (b[1] > at[1])
                    && at[0] < a[0] + (at[1] - a[1]) / (b[1] - a[1]) * (b[0] - a[0])
                {
                    crossings += 1;
                }
            }
            crossings % 2 == 1
        };
        let box_ = ([0.0, 0.0], [100.0, 100.0]);
        let moon = outline(settings(Shape::Moon), box_.0, box_.1, Drag::default());
        // The thick part is on the left and the bite on the right.
        assert!(inside(&moon, [8.0, 50.0]));
        assert!(!inside(&moon, [60.0, 50.0]));
        let half = outline(settings(Shape::Semicircle), box_.0, box_.1, Drag::default());
        assert!(inside(&half, [50.0, 90.0]));
        assert!(!inside(&half, [3.0, 3.0]));
    }

    #[test]
    fn a_drag_too_small_draws_nothing() {
        for shape in std::iter::once(Shape::Line).chain(CLOSED) {
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

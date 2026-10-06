// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! A deterministic drawing document and the geometry both renderers draw.
//!
//! Geometry is built here once, so the renderers differ only in how they
//! rasterize and composite, not in how they build outlines.

pub type Point = [f32; 2];
/// Straight (not premultiplied) RGBA.
pub type Rgba = [u8; 4];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Blend {
    Normal,
    Multiply,
    Screen,
    Overlay,
}

/// Bottom to top. The last layer is clipped to the one below it, which
/// composites with the group as one unit, as in the current app.
pub const LAYER_BLENDS: [Blend; 5] = [
    Blend::Normal,
    Blend::Multiply,
    Blend::Screen,
    Blend::Overlay,
    Blend::Normal,
];
pub const CLIP_BASE: usize = 3;
pub const CLIPPED: usize = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    /// Constant width, stroked with round caps and joins.
    Pen,
    /// Width follows pressure; filled outline.
    Pressure,
    /// Soft round dabs along the path.
    Airbrush,
}

struct Stroke {
    kind: Kind,
    color: Rgba,
    width: f32,
    /// x, y, pressure.
    points: Vec<[f32; 3]>,
}

pub struct Document {
    pub size: u32,
    layers: Vec<Vec<Stroke>>,
}

pub enum Segment {
    Move(Point),
    Line(Point),
    Cubic(Point, Point, Point),
    Close,
}

pub enum Draw {
    /// Non-zero fill.
    Fill { path: Vec<Segment>, color: Rgba },
    /// Open polyline with round caps and joins.
    Stroke {
        points: Vec<Point>,
        width: f32,
        color: Rgba,
    },
    /// Solid to half the radius, then fading to transparent at the edge.
    Dab {
        center: Point,
        radius: f32,
        color: Rgba,
    },
}

struct Random(u64);

impl Random {
    fn unit(&mut self) -> f32 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((self.0 >> 40) as f32) / (1u64 << 24) as f32
    }

    fn range(&mut self, low: f32, high: f32) -> f32 {
        low + (high - low) * self.unit()
    }
}

/// Repeatable noise in -1..1 for one point of one frame.
fn noise(stroke: usize, point: usize, frame: usize, axis: u64) -> f32 {
    let mut value = (stroke as u64) << 40 ^ (point as u64) << 16 ^ (frame as u64) << 2 ^ axis;
    value ^= value >> 33;
    value = value.wrapping_mul(0xff51afd7ed558ccd);
    value ^= value >> 33;
    value = value.wrapping_mul(0xc4ceb9fe1a85ec53);
    value ^= value >> 33;
    (value >> 40) as f32 / (1u64 << 23) as f32 - 1.0
}

/// How far points move between animation frames, in pixels.
const WOBBLE: f32 = 2.0;

impl Document {
    /// `strokes` spread evenly over the layers, each with `points` samples:
    /// half pen, 30% pressure, 20% airbrush, all translucent.
    pub fn generate(size: u32, strokes: usize, points: usize) -> Self {
        let mut random = Random(0x5547_5552_5547_5530);
        let extent = size as f32;
        let per_layer = strokes / LAYER_BLENDS.len();
        let layers = (0..LAYER_BLENDS.len())
            .map(|_| {
                (0..per_layer)
                    .map(|index| {
                        let kind = match index % 10 {
                            0..5 => Kind::Pen,
                            5..8 => Kind::Pressure,
                            _ => Kind::Airbrush,
                        };
                        let color = [
                            (random.unit() * 255.0) as u8,
                            (random.unit() * 255.0) as u8,
                            (random.unit() * 255.0) as u8,
                            match kind {
                                Kind::Airbrush => 64,
                                _ => random.range(140.0, 230.0) as u8,
                            },
                        ];
                        let width = match kind {
                            Kind::Airbrush => random.range(16.0, 48.0),
                            _ => random.range(2.0, 24.0),
                        };
                        let mut position = [
                            random.range(0.05, 0.95) * extent,
                            random.range(0.05, 0.95) * extent,
                        ];
                        let mut heading = random.range(0.0, std::f32::consts::TAU);
                        let step = random.range(3.0, 8.0);
                        let points = (0..points)
                            .map(|index| {
                                heading += random.range(-0.25, 0.25);
                                position[0] =
                                    (position[0] + heading.cos() * step).clamp(0.0, extent);
                                position[1] =
                                    (position[1] + heading.sin() * step).clamp(0.0, extent);
                                let progress = index as f32 / points.max(2) as f32;
                                let pressure =
                                    0.25 + 0.75 * (progress * std::f32::consts::PI).sin();
                                [position[0], position[1], pressure]
                            })
                            .collect();
                        Stroke {
                            kind,
                            color,
                            width,
                            points,
                        }
                    })
                    .collect()
            })
            .collect();
        Self { size, layers }
    }

    pub fn point_count(&self) -> usize {
        self.layers
            .iter()
            .flatten()
            .map(|stroke| stroke.points.len())
            .sum()
    }

    pub fn stroke_count(&self) -> usize {
        self.layers.iter().map(Vec::len).sum()
    }

    /// The draws of one layer in one animation frame; frame 0 is unmoved.
    pub fn layer_draws(&self, layer: usize, frame: usize) -> Vec<Draw> {
        let mut draws = Vec::new();
        for (index, stroke) in self.layers[layer].iter().enumerate() {
            let id = layer << 20 | index;
            let moved: Vec<[f32; 3]> = stroke
                .points
                .iter()
                .enumerate()
                .map(|(point, &[x, y, pressure])| {
                    if frame == 0 {
                        return [x, y, pressure];
                    }
                    [
                        x + WOBBLE * noise(id, point, frame, 0),
                        y + WOBBLE * noise(id, point, frame, 1),
                        pressure,
                    ]
                })
                .collect();
            match stroke.kind {
                Kind::Pen => draws.push(Draw::Stroke {
                    points: moved.iter().map(|&[x, y, _]| [x, y]).collect(),
                    width: stroke.width,
                    color: stroke.color,
                }),
                Kind::Pressure => draws.push(Draw::Fill {
                    path: pressure_outline(&moved, stroke.width),
                    color: stroke.color,
                }),
                Kind::Airbrush => draws.extend(moved.iter().map(|&[x, y, pressure]| Draw::Dab {
                    center: [x, y],
                    radius: stroke.width * 0.5 * pressure,
                    color: stroke.color,
                })),
            }
        }
        draws
    }

    /// A long pen stroke split into its segments, as drawn one input at a time.
    pub fn live_segments(&self, count: usize) -> Vec<(Draw, [u32; 4])> {
        let center = self.size as f32 * 0.5;
        let width = 12.0;
        let point = |index: usize| {
            let angle = index as f32 * 0.02;
            let radius = center * (0.8 - 0.6 * index as f32 / count as f32);
            [center + radius * angle.cos(), center + radius * angle.sin()]
        };
        (0..count)
            .map(|index| {
                let (from, to) = (point(index), point(index + 1));
                let margin = width * 0.5 + 2.0;
                let bound = |value: f32| value.clamp(0.0, self.size as f32) as u32;
                let dirty = [
                    bound(from[0].min(to[0]) - margin),
                    bound(from[1].min(to[1]) - margin),
                    bound(from[0].max(to[0]) + margin + 1.0),
                    bound(from[1].max(to[1]) + margin + 1.0),
                ];
                let draw = Draw::Stroke {
                    points: vec![from, to],
                    width,
                    color: [30, 30, 30, 200],
                };
                (draw, dirty)
            })
            .collect()
    }
}

fn signed_area(points: &[Point]) -> f32 {
    let mut area = 0.0;
    for (index, a) in points.iter().enumerate() {
        let b = points[(index + 1) % points.len()];
        area += a[0] * b[1] - b[0] * a[1];
    }
    area
}

/// Kappa for a quarter circle drawn as one cubic.
const KAPPA: f32 = 0.552_284_8;

/// A circle with positive signed area, like every quad in an outline, so
/// that their non-zero union has no holes.
fn push_circle(path: &mut Vec<Segment>, [x, y]: Point, r: f32) {
    let k = r * KAPPA;
    path.push(Segment::Move([x + r, y]));
    path.push(Segment::Cubic([x + r, y + k], [x + k, y + r], [x, y + r]));
    path.push(Segment::Cubic([x - k, y + r], [x - r, y + k], [x - r, y]));
    path.push(Segment::Cubic([x - r, y - k], [x - k, y - r], [x, y - r]));
    path.push(Segment::Cubic([x + k, y - r], [x + r, y - k], [x + r, y]));
    path.push(Segment::Close);
}

pub fn circle(center: Point, r: f32) -> Vec<Segment> {
    let mut path = Vec::with_capacity(6);
    push_circle(&mut path, center, r);
    path
}

/// The union of a disc at every sample and a quad between neighbours.
fn pressure_outline(points: &[[f32; 3]], width: f32) -> Vec<Segment> {
    let mut path = Vec::with_capacity(points.len() * 11);
    let radius = |pressure: f32| (width * 0.5 * pressure).max(0.25);
    for &[x, y, pressure] in points {
        push_circle(&mut path, [x, y], radius(pressure));
    }
    for pair in points.windows(2) {
        let ([x0, y0, p0], [x1, y1, p1]) = (pair[0], pair[1]);
        let (dx, dy) = (x1 - x0, y1 - y0);
        let length = (dx * dx + dy * dy).sqrt();
        if length < 1e-3 {
            continue;
        }
        let normal = [-dy / length, dx / length];
        let (r0, r1) = (radius(p0), radius(p1));
        let mut quad = [
            [x0 + normal[0] * r0, y0 + normal[1] * r0],
            [x1 + normal[0] * r1, y1 + normal[1] * r1],
            [x1 - normal[0] * r1, y1 - normal[1] * r1],
            [x0 - normal[0] * r0, y0 - normal[1] * r0],
        ];
        if signed_area(&quad) < 0.0 {
            quad.reverse();
        }
        path.push(Segment::Move(quad[0]));
        for corner in &quad[1..] {
            path.push(Segment::Line(*corner));
        }
        path.push(Segment::Close);
    }
    path
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn circles_and_quads_wind_the_same_way() {
        let path = circle([0.0, 0.0], 1.0);
        let anchors: Vec<Point> = path
            .iter()
            .filter_map(|segment| match segment {
                Segment::Move(point) | Segment::Cubic(_, _, point) => Some(*point),
                _ => None,
            })
            .collect();
        assert!(signed_area(&anchors[..4]) > 0.0);
    }

    #[test]
    fn the_document_has_the_requested_size() {
        let document = Document::generate(512, 100, 50);
        assert_eq!(document.stroke_count(), 100);
        assert_eq!(document.point_count(), 5000);
    }

    #[test]
    fn frame_zero_is_unmoved_and_later_frames_move() {
        let document = Document::generate(512, 10, 10);
        let first = |draws: Vec<Draw>| match &draws[0] {
            Draw::Stroke { points, .. } => points[1],
            _ => unreachable!("the first stroke of a layer is a pen stroke"),
        };
        let still = first(document.layer_draws(0, 0));
        assert_eq!(still, first(document.layer_draws(0, 0)));
        assert_ne!(still, first(document.layer_draws(0, 1)));
    }
}

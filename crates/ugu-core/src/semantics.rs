// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! The meaning of layer operations, pinned by a reference evaluator.
//!
//! Strokes are rectangles and motion moves a stroke right by its wobble
//! amount on odd frames. That is enough to tell an operation that acts on each
//! frame's result from one that acts on a frozen raster. The real renderer
//! must agree with these results; it is not this code.

use crate::ops::*;
use std::collections::HashMap;

const DOCUMENT_WOBBLE: Wobble = Wobble::classic(1.0);
const STILL: Wobble = Wobble::classic(0.0);
const RED: [f32; 4] = [1.0, 0.0, 0.0, 1.0];
const BLUE: [f32; 4] = [0.0, 0.0, 1.0, 1.0];
const GREEN: [f32; 4] = [0.0, 1.0, 0.0, 1.0];
const CLEAR: [f32; 4] = [0.0; 4];

/// Premultiplied pixels, row-major.
#[derive(Clone, Debug, PartialEq)]
struct Surface {
    size: [u32; 2],
    pixels: Vec<[f32; 4]>,
}

impl Surface {
    fn new(size: [u32; 2]) -> Self {
        Self {
            size,
            pixels: vec![CLEAR; (size[0] * size[1]) as usize],
        }
    }

    fn index(&self, x: i32, y: i32) -> Option<usize> {
        let inside = x >= 0 && y >= 0 && (x as u32) < self.size[0] && (y as u32) < self.size[1];
        inside.then(|| (y as u32 * self.size[0] + x as u32) as usize)
    }

    fn at(&self, x: i32, y: i32) -> [f32; 4] {
        self.index(x, y).map_or(CLEAR, |index| self.pixels[index])
    }

    fn over(&mut self, x: i32, y: i32, source: [f32; 4]) {
        if let Some(index) = self.index(x, y) {
            let below = self.pixels[index];
            self.pixels[index] = std::array::from_fn(|c| source[c] + below[c] * (1.0 - source[3]));
        }
    }

    fn set(&mut self, x: i32, y: i32, value: [f32; 4]) {
        if let Some(index) = self.index(x, y) {
            self.pixels[index] = value;
        }
    }

    fn draw(&mut self, other: &Surface, opacity: f32) {
        for y in 0..self.size[1] as i32 {
            for x in 0..self.size[0] as i32 {
                let source = other.at(x, y);
                self.over(x, y, source.map(|c| c * opacity));
            }
        }
    }

    fn assert_near(&self, other: &Surface) {
        assert_eq!(self.size, other.size);
        for (index, (a, b)) in self.pixels.iter().zip(&other.pixels).enumerate() {
            let close = a.iter().zip(b).all(|(a, b)| (a - b).abs() < 1e-5);
            assert!(close, "pixel {index}: {a:?} != {b:?}");
        }
    }
}

/// Covers x0..x1, y0..y1.
struct Rect([i32; 4]);

impl Rect {
    fn cells(&self) -> impl Iterator<Item = (i32, i32)> + '_ {
        let [x0, y0, x1, y1] = self.0;
        (y0..y1).flat_map(move |y| (x0..x1).map(move |x| (x, y)))
    }
}

#[derive(Default)]
struct World {
    strokes: HashMap<StrokeId, (Rect, [f32; 4])>,
    masks: HashMap<MaskId, Rect>,
}

impl World {
    fn stroke(&mut self, rect: [i32; 4], color: [f32; 4]) -> StrokeId {
        let id = StrokeId(self.strokes.len() as u32);
        self.strokes.insert(id, (Rect(rect), color));
        id
    }

    fn mask(&mut self, rect: [i32; 4]) -> MaskId {
        let id = MaskId(self.masks.len() as u32);
        self.masks.insert(id, Rect(rect));
        id
    }

    fn masked(&self, clip: Option<MaskId>, x: i32, y: i32) -> bool {
        clip.is_none_or(|mask| self.masks[&mask].cells().any(|cell| cell == (x, y)))
    }

    fn apply(&self, ops: &[Op], surface: &mut Surface, frame: u32, wobble: Wobble) {
        for op in ops {
            match op {
                Op::Paint { stroke, clip } | Op::Erase { stroke, clip } => {
                    let (rect, color) = &self.strokes[stroke];
                    let shift = if frame % 2 == 1 {
                        wobble.amount as i32
                    } else {
                        0
                    };
                    for (x, y) in rect.cells().map(|(x, y)| (x + shift, y)) {
                        if !self.masked(*clip, x, y) {
                            continue;
                        }
                        if matches!(op, Op::Paint { .. }) {
                            surface.over(x, y, *color);
                        } else {
                            surface.set(x, y, CLEAR);
                        }
                    }
                }
                Op::Fill {
                    coverage,
                    color,
                    clip,
                    ..
                } => {
                    let [r, g, b, a] = color.0.map(|c| f32::from(c) / 255.0);
                    for (x, y) in self.masks[coverage].cells() {
                        if self.masked(*clip, x, y) {
                            surface.over(x, y, [r * a, g * a, b * a, a]);
                        }
                    }
                }
                Op::PlaceImage { .. } => unreachable!("not used by these tests"),
                Op::TransformSelection {
                    mask,
                    transform,
                    keep_source,
                    ..
                } => {
                    let [1.0, 0.0, dx, 0.0, 1.0, dy] = transform.0 else {
                        unreachable!("these tests only translate by whole pixels")
                    };
                    let cut: Vec<_> = self.masks[mask]
                        .cells()
                        .map(|(x, y)| (x, y, surface.at(x, y)))
                        .collect();
                    if !keep_source {
                        cut.iter().for_each(|&(x, y, _)| surface.set(x, y, CLEAR));
                    }
                    for (x, y, pixel) in cut {
                        surface.over(x + dx as i32, y + dy as i32, pixel);
                    }
                }
                Op::ClearSelection { mask } => {
                    self.masks[mask]
                        .cells()
                        .for_each(|(x, y)| surface.set(x, y, CLEAR));
                }
                Op::Crop { offset, size } => {
                    let mut cropped = Surface::new(*size);
                    for y in 0..surface.size[1] as i32 {
                        for x in 0..surface.size[0] as i32 {
                            cropped.set(x + offset[0], y + offset[1], surface.at(x, y));
                        }
                    }
                    *surface = cropped;
                }
                Op::Resample { size, .. } => {
                    let mut resampled = Surface::new(*size);
                    for y in 0..size[1] {
                        for x in 0..size[0] {
                            let from_x = x * surface.size[0] / size[0];
                            let from_y = y * surface.size[1] / size[1];
                            resampled.set(
                                x as i32,
                                y as i32,
                                surface.at(from_x as i32, from_y as i32),
                            );
                        }
                    }
                    *surface = resampled;
                }
                Op::Isolated(section) => {
                    let mut isolated = Surface::new(surface.size);
                    self.apply(
                        &section.ops,
                        &mut isolated,
                        frame,
                        section.wobble.unwrap_or(DOCUMENT_WOBBLE),
                    );
                    assert_eq!(isolated.size, surface.size, "a section changed the canvas");
                    surface.draw(&isolated, section.opacity);
                }
            }
        }
    }

    fn layer(&self, layer: &PaintLayer, frame: u32) -> Surface {
        let mut surface = Surface::new(layer.initial_size);
        self.apply(
            &layer.ops,
            &mut surface,
            frame,
            layer.wobble.unwrap_or(DOCUMENT_WOBBLE),
        );
        surface
    }

    /// Normal layers only, bottom first.
    fn document(&self, layers: &[&PaintLayer], frame: u32) -> Surface {
        let mut document = Surface::new(layers[0].final_size());
        for layer in layers {
            assert_eq!(layer.blend, Blend::Normal);
            document.draw(&self.layer(layer, frame), layer.opacity);
        }
        document
    }
}

fn layer(ops: Vec<Op>, size: [u32; 2]) -> PaintLayer {
    PaintLayer {
        ops,
        opacity: 1.0,
        blend: Blend::Normal,
        clip_to_below: false,
        wobble: Some(STILL),
        initial_size: size,
    }
}

fn paint(stroke: StrokeId) -> Op {
    Op::Paint { stroke, clip: None }
}

fn erase(stroke: StrokeId) -> Op {
    Op::Erase { stroke, clip: None }
}

#[test]
fn an_eraser_reaches_only_what_came_before_it() {
    let mut world = World::default();
    let before = world.stroke([0, 0, 4, 1], RED);
    let eraser = world.stroke([0, 0, 4, 1], CLEAR);
    let after = world.stroke([2, 0, 4, 1], BLUE);
    let result = world.layer(
        &layer(vec![paint(before), erase(eraser), paint(after)], [4, 1]),
        0,
    );
    assert_eq!(result.at(0, 0), CLEAR);
    assert_eq!(result.at(2, 0), BLUE);
}

#[test]
fn merging_keeps_every_frame_and_each_eraser_in_its_own_layer() {
    let mut world = World::default();
    let lower_line = world.stroke([0, 0, 6, 2], RED);
    let lower_eraser = world.stroke([0, 0, 1, 2], CLEAR);
    let upper_line = world.stroke([2, 1, 5, 3], BLUE);
    // Without isolation this would also erase the lower layer's red.
    let upper_eraser = world.stroke([3, 0, 5, 3], CLEAR);
    let mut below = layer(vec![paint(lower_line), erase(lower_eraser)], [6, 3]);
    below.opacity = 0.5;
    below.wobble = None;
    let mut above = layer(vec![paint(upper_line), erase(upper_eraser)], [6, 3]);
    above.opacity = 0.75;
    above.wobble = Some(Wobble::classic(2.0));

    let merged = merge_down(&below, &above).unwrap();
    for frame in 0..4 {
        world
            .document(&[&merged], frame)
            .assert_near(&world.document(&[&below, &above], frame));
    }
    assert_eq!(world.layer(&merged, 0).at(3, 0), RED.map(|c| c * 0.5));
}

#[test]
fn an_eraser_added_after_a_merge_reaches_both_layers() {
    let mut world = World::default();
    let lower_line = world.stroke([0, 0, 4, 1], RED);
    let upper_line = world.stroke([0, 1, 4, 2], BLUE);
    let eraser = world.stroke([0, 0, 2, 2], CLEAR);
    let below = layer(vec![paint(lower_line)], [4, 2]);
    let above = layer(vec![paint(upper_line)], [4, 2]);
    let mut merged = merge_down(&below, &above).unwrap();
    merged.ops.push(erase(eraser));
    let result = world.layer(&merged, 0);
    assert_eq!([result.at(0, 0), result.at(0, 1)], [CLEAR, CLEAR]);
    assert_eq!([result.at(3, 0), result.at(3, 1)], [RED, BLUE]);
}

#[test]
fn merging_refuses_what_it_cannot_keep() {
    let below = layer(vec![], [4, 4]);
    let mut multiply = below.clone();
    multiply.blend = Blend::Multiply;
    assert_eq!(merge_down(&below, &multiply), Err(MergeRefusal::Blend));
    let mut clipped = below.clone();
    clipped.clip_to_below = true;
    assert_eq!(merge_down(&below, &clipped), Err(MergeRefusal::Clipping));
    let other_canvas = layer(vec![], [8, 8]);
    assert_eq!(
        merge_down(&below, &other_canvas),
        Err(MergeRefusal::CanvasEpoch)
    );
    let cropping = layer(
        vec![Op::Crop {
            offset: [0, 0],
            size: [4, 4],
        }],
        [4, 4],
    );
    assert_eq!(
        merge_down(&below, &cropping),
        Err(MergeRefusal::CanvasChange)
    );
    let mut translucent_cropping = cropping.clone();
    translucent_cropping.opacity = 0.5;
    assert_eq!(
        merge_down(&translucent_cropping, &below),
        Err(MergeRefusal::CanvasChange)
    );
    assert!(merge_down(&cropping, &below).is_ok());
}

#[test]
fn a_selection_transform_moves_each_frames_own_result() {
    let mut world = World::default();
    let moving = world.stroke([1, 0, 3, 1], RED);
    let later = world.stroke([0, 0, 1, 1], BLUE);
    let row = world.mask([0, 0, 6, 1]);
    let mut moved = layer(
        vec![
            paint(moving),
            Op::TransformSelection {
                mask: row,
                transform: Affine::translation(0.0, 2.0),
                sampling: Sampling::Nearest,
                keep_source: false,
            },
            paint(later),
        ],
        [6, 3],
    );
    moved.wobble = Some(Wobble::classic(1.0));
    let even = world.layer(&moved, 0);
    let odd = world.layer(&moved, 1);
    // Frame 1's stroke sits one pixel right, and the moved copy follows it
    // rather than repeating frame 0's raster.
    assert_eq!(
        [even.at(1, 2), even.at(2, 2), even.at(3, 2)],
        [RED, RED, CLEAR]
    );
    assert_eq!(
        [odd.at(1, 2), odd.at(2, 2), odd.at(3, 2)],
        [CLEAR, RED, RED]
    );
    // A stroke after the transform is not moved by it.
    assert_eq!(even.at(0, 0), BLUE);
    assert_eq!(even.at(1, 0), CLEAR);
}

#[test]
fn a_fill_keeps_its_coverage_while_lines_move() {
    let mut world = World::default();
    let outline = world.stroke([0, 0, 1, 3], BLUE);
    let inside = world.mask([1, 0, 3, 3]);
    let fill = Op::Fill {
        coverage: inside,
        color: Rgba8([0, 255, 0, 255]),
        antialias: false,
        clip: None,
    };
    let mut filled = layer(vec![fill, paint(outline)], [4, 3]);
    filled.wobble = Some(Wobble::classic(1.0));
    let even = world.layer(&filled, 0);
    let odd = world.layer(&filled, 1);
    assert_eq!(
        [even.at(0, 1), even.at(1, 1), even.at(2, 1)],
        [BLUE, GREEN, GREEN]
    );
    // The outline moved over the fill; the fill did not move or refill.
    assert_eq!(
        [odd.at(0, 1), odd.at(1, 1), odd.at(2, 1), odd.at(3, 1)],
        [CLEAR, BLUE, GREEN, CLEAR]
    );
}

#[test]
fn a_crop_moves_the_canvas_and_a_resample_scales_what_came_before() {
    let mut world = World::default();
    let first = world.stroke([0, 0, 2, 2], RED);
    let after_crop = world.stroke([0, 0, 1, 1], BLUE);
    let after_resample = world.stroke([2, 2, 3, 3], GREEN);
    let ops = vec![
        paint(first),
        Op::Crop {
            offset: [2, 2],
            size: [6, 6],
        },
        paint(after_crop),
        Op::Resample {
            size: [3, 3],
            sampling: Sampling::Nearest,
        },
        paint(after_resample),
    ];
    let edited = layer(ops, [4, 4]);
    assert_eq!(edited.final_size(), [3, 3]);
    let result = world.layer(&edited, 0);
    // Red was at 2..4 after the crop and lands on pixel 1 at half size; blue
    // was drawn in the cropped canvas's coordinates; green after the resample.
    assert_eq!(
        [result.at(0, 0), result.at(1, 1), result.at(2, 2)],
        [BLUE, RED, GREEN]
    );
}

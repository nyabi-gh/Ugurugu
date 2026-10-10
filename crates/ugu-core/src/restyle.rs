// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! Changing the colour and width of what the selection touches on a layer,
//! 2.2.13's "Edit stroke properties".
//!
//! A stroke touches the selection when a selected pixel's centre is within
//! its reach of the line through its points, before motion moves it; a fill
//! when a selected pixel is in its coverage. Both count only inside their
//! own clip. What was drawn before a crop or resample is looked for where
//! that change put it.
//!
//! A changed stroke is stored under a new id, so nothing keyed by a stroke
//! id sees different content under the same one.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::command::next_stroke_id;
use crate::document::{Document, Layer, LayerId, LayerKind};
use crate::edit::{Change, EditError};
use crate::ops::{Affine, MaskId, Op, Rgba8, StrokeId};
use crate::selection::{Selection, copy_bits};
use crate::store::{BrushEngine, Mask, Store, Stroke};

/// What to change; `None` leaves it as it is.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Style {
    pub color: Option<Rgba8>,
    pub width: Option<f32>,
}

/// What the selection touches on a layer.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Touched {
    /// Drawn strokes and fills, which have a colour.
    pub colored: usize,
    /// Drawn and erasing strokes, which have a width.
    pub sized: usize,
    /// The colour all of `colored` share, if they do.
    pub color: Option<Rgba8>,
    /// The width all of `sized` share, if they do.
    pub width: Option<f32>,
}

/// What the selection touches on `layer`; nothing when it is not a paint
/// layer.
pub fn touched(document: &Document, layer: LayerId, selection: &Selection) -> Touched {
    let mut touched = Touched::default();
    let mut colors = HashSet::new();
    let mut widths = HashSet::new();
    let Some(paint) = paint(document, layer) else {
        return touched;
    };
    let mut finder = Finder::new(&document.store, paint, selection);
    let mut ops = paint.ops.clone();
    finder.visit(&mut ops, &mut |op, store| match op {
        Op::Paint { stroke, .. } => {
            let stroke = &store.strokes[stroke];
            touched.colored += 1;
            touched.sized += 1;
            colors.insert(stroke.color.0);
            widths.insert(stroke.width.to_bits());
        }
        Op::Erase { stroke, .. } => {
            touched.sized += 1;
            widths.insert(store.strokes[stroke].width.to_bits());
        }
        Op::Fill { color, .. } => {
            touched.colored += 1;
            colors.insert(color.0);
        }
        _ => {}
    });
    if let [color] = colors.into_iter().collect::<Vec<_>>()[..] {
        touched.color = Some(Rgba8(color));
    }
    if let [width] = widths.into_iter().collect::<Vec<_>>()[..] {
        touched.width = Some(f32::from_bits(width));
    }
    touched
}

/// Gives what the selection touches on `layer` `style`: a colour to drawn
/// strokes and fills, a width to drawn and erasing strokes. Empty when that
/// changes nothing.
pub fn restyle(
    document: &Document,
    layer: LayerId,
    selection: &Selection,
    style: Style,
) -> Result<Vec<Change>, EditError> {
    let Some(source) = document.layer(layer) else {
        return Err(EditError::NoSuchLayer(layer));
    };
    let LayerKind::Paint(paint) = &source.kind else {
        return Err(EditError::NotPaintLayer(layer));
    };
    let mut ops = paint.ops.clone();
    let mut next = next_stroke_id(document);
    let mut renamed: HashMap<StrokeId, StrokeId> = HashMap::new();
    let mut inserted = Vec::new();
    let mut changed = false;
    let mut finder = Finder::new(&document.store, paint, selection);
    finder.visit(&mut ops, &mut |op, store| {
        let erase = matches!(op, Op::Erase { .. });
        match op {
            Op::Paint { stroke: id, .. } | Op::Erase { stroke: id, .. } => {
                let stroke = &store.strokes[id];
                let color = style.color.filter(|_| !erase).unwrap_or(stroke.color);
                let width = style.width.unwrap_or(stroke.width);
                if color == stroke.color && width == stroke.width {
                    return;
                }
                *id = *renamed.entry(*id).or_insert_with(|| {
                    let new = next;
                    next = StrokeId(next.0 + 1);
                    inserted.push(Change::InsertStroke(
                        new,
                        Stroke {
                            color,
                            width,
                            ..stroke.clone()
                        },
                    ));
                    new
                });
                changed = true;
            }
            Op::Fill { color, .. } => {
                if let Some(next) = style.color.filter(|next| next != color) {
                    *color = next;
                    changed = true;
                }
            }
            _ => {}
        }
    });
    if !changed {
        return Ok(Vec::new());
    }
    let mut replaced = source.clone();
    if let LayerKind::Paint(paint) = &mut replaced.kind {
        paint.ops = ops;
    }
    // Strokes still drawn by another layer stay stored.
    let mut kept = HashSet::new();
    for other in &document.layers {
        strokes_of(other, layer, &mut kept);
    }
    let mut changes = inserted;
    changes.push(Change::ReplaceLayer(replaced));
    changes.extend(
        renamed
            .into_keys()
            .filter(|old| !kept.contains(old))
            .map(Change::RemoveStroke),
    );
    Ok(changes)
}

fn paint(document: &Document, layer: LayerId) -> Option<&crate::ops::PaintLayer> {
    match &document.layer(layer)?.kind {
        LayerKind::Paint(paint) => Some(paint),
        LayerKind::Group(_) => None,
    }
}

/// The strokes drawn in `layer` and the layers in it, but not in `except`.
fn strokes_of(layer: &Layer, except: LayerId, out: &mut HashSet<StrokeId>) {
    fn ops_strokes(ops: &[Op], out: &mut HashSet<StrokeId>) {
        for op in ops {
            match op {
                Op::Paint { stroke, .. } | Op::Erase { stroke, .. } => {
                    out.insert(*stroke);
                }
                Op::Isolated(section) => ops_strokes(&section.ops, out),
                _ => {}
            }
        }
    }
    if layer.id == except {
        return;
    }
    match &layer.kind {
        LayerKind::Paint(paint) => ops_strokes(&paint.ops, out),
        LayerKind::Group(group) => {
            for child in &group.children {
                strokes_of(child, except, out);
            }
        }
    }
}

/// Finds the operations the selection touches, on each canvas the layer had.
struct Finder<'a> {
    store: &'a Store,
    selection: &'a Selection,
    /// Each canvas, the last being the one the selection is on, and what
    /// moves it onto that one.
    canvases: Vec<([u32; 2], Affine)>,
    /// The selection on a canvas cut to a clip, as asked for.
    areas: HashMap<(usize, Option<MaskId>), Option<Arc<Area>>>,
}

impl<'a> Finder<'a> {
    fn new(store: &'a Store, paint: &crate::ops::PaintLayer, selection: &'a Selection) -> Self {
        let mut canvases = vec![(paint.initial_size, Affine::IDENTITY)];
        let mut size = paint.initial_size;
        for op in &paint.ops {
            let (step, next) = match *op {
                Op::Crop { offset, size } => (
                    Affine::translation(f64::from(offset[0]), f64::from(offset[1])),
                    size,
                ),
                Op::Resample { size: next, .. } => {
                    let scale = |axis: usize| f64::from(next[axis]) / f64::from(size[axis]);
                    (
                        Affine::scaling_about([scale(0), scale(1)], [0.0, 0.0]),
                        next,
                    )
                }
                _ => continue,
            };
            canvases.last_mut().expect("one at least").1 = step;
            canvases.push((next, Affine::IDENTITY));
            size = next;
        }
        // Each step moves its canvas onto the next; chained, onto the last.
        for index in (0..canvases.len().saturating_sub(1)).rev() {
            canvases[index].1 = canvases[index].1.then(canvases[index + 1].1);
        }
        Self {
            store,
            selection,
            canvases,
            areas: HashMap::new(),
        }
    }

    /// Calls `found` with each operation in `ops` the selection touches,
    /// sections included.
    fn visit(&mut self, ops: &mut [Op], found: &mut impl FnMut(&mut Op, &Store)) {
        let mut canvas = 0;
        for op in ops {
            self.visit_op(op, canvas, found);
            if matches!(op, Op::Crop { .. } | Op::Resample { .. }) {
                canvas += 1;
            }
        }
    }

    fn visit_op(&mut self, op: &mut Op, canvas: usize, found: &mut impl FnMut(&mut Op, &Store)) {
        let hit = match op {
            Op::Paint { stroke, clip } | Op::Erase { stroke, clip } => self
                .area(canvas, *clip)
                .is_some_and(|area| stroke_touches(&self.store.strokes[stroke], &area)),
            Op::Fill { coverage, clip, .. } => self.area(canvas, *clip).is_some_and(|area| {
                intersection(&self.store.masks[coverage], &area.mask).is_some()
            }),
            Op::Isolated(section) => {
                // A section never changes the canvas.
                for inner in &mut section.ops {
                    self.visit_op(inner, canvas, found);
                }
                false
            }
            _ => false,
        };
        if hit {
            found(op, self.store);
        }
    }

    /// The selection on canvas `index` cut to `clip`; `None` when nothing is
    /// left.
    fn area(&mut self, index: usize, clip: Option<MaskId>) -> Option<Arc<Area>> {
        if let Some(area) = self.areas.get(&(index, clip)) {
            return area.clone();
        }
        let mask = match clip {
            None if index + 1 == self.canvases.len() => Some(self.selection.mask().clone()),
            None => {
                let (canvas, to_last) = self.canvases[index];
                self.selection
                    .before(canvas, to_last)
                    .map(|selection| selection.mask().clone())
            }
            Some(clip) => self
                .area(index, None)
                .and_then(|area| intersection(&area.mask, &self.store.masks[&clip])),
        };
        let area = mask.map(|mask| Arc::new(Area::new(mask)));
        self.areas.insert((index, clip), area.clone());
        area
    }
}

/// Tiles of this many pixels square say whether they hold a set pixel.
const TILE: i32 = 16;

/// A mask, and a count of its tiles holding a set pixel above and left of
/// each tile corner, so that a rectangle with none is found at once.
struct Area {
    mask: Mask,
    columns: usize,
    sums: Vec<u32>,
}

impl Area {
    fn new(mask: Mask) -> Self {
        let [_, _, width, height] = mask.bounds;
        let columns = (width as usize).div_ceil(TILE as usize);
        let rows = (height as usize).div_ceil(TILE as usize);
        let row_bytes = Mask::row_bytes(width);
        let mut held = vec![false; columns * rows];
        for (y, bits) in mask.bits.chunks_exact(row_bytes).enumerate() {
            let tiles = &mut held[y / TILE as usize * columns..][..columns];
            for (index, &byte) in bits.iter().enumerate() {
                if byte != 0 {
                    tiles[index * 8 / TILE as usize] = true;
                }
            }
        }
        let mut sums = vec![0u32; (columns + 1) * (rows + 1)];
        for row in 0..rows {
            for column in 0..columns {
                let at = (row + 1) * (columns + 1) + column + 1;
                sums[at] =
                    u32::from(held[row * columns + column]) + sums[at - 1] + sums[at - columns - 1]
                        - sums[at - columns - 2];
            }
        }
        Self {
            mask,
            columns,
            sums,
        }
    }

    /// Whether a set pixel may lie from `low` to `high`; false only when
    /// none does.
    fn may_hold(&self, low: [f64; 2], high: [f64; 2]) -> bool {
        let [left, top, width, height] = self.mask.bounds;
        let tile = |value: f64, origin: i32, edge: i32| {
            ((value.floor() as i64 - i64::from(origin)).clamp(0, i64::from(edge)) / i64::from(TILE))
                as usize
        };
        if high[0] < f64::from(left)
            || high[1] < f64::from(top)
            || low[0] >= f64::from(left + width)
            || low[1] >= f64::from(top + height)
        {
            return false;
        }
        let [x0, y0] = [tile(low[0], left, width - 1), tile(low[1], top, height - 1)];
        let [x1, y1] = [
            tile(high[0], left, width - 1) + 1,
            tile(high[1], top, height - 1) + 1,
        ];
        let at = |x: usize, y: usize| self.sums[y * (self.columns + 1) + x];
        at(x1, y1) + at(x0, y0) > at(x0, y1) + at(x1, y0)
    }
}

fn row(mask: &Mask, y: i32) -> &[u8] {
    let row_bytes = Mask::row_bytes(mask.bounds[2]);
    let start = (y - mask.bounds[1]) as usize * row_bytes;
    &mask.bits[start..start + row_bytes]
}

/// The pixels set in both; `None` when there are none.
fn intersection(a: &Mask, b: &Mask) -> Option<Mask> {
    let left = a.bounds[0].max(b.bounds[0]);
    let top = a.bounds[1].max(b.bounds[1]);
    let right = (a.bounds[0] + a.bounds[2]).min(b.bounds[0] + b.bounds[2]);
    let bottom = (a.bounds[1] + a.bounds[3]).min(b.bounds[1] + b.bounds[3]);
    if left >= right || top >= bottom {
        return None;
    }
    let width = right - left;
    let row_bytes = Mask::row_bytes(width);
    let mut bits = vec![0u8; row_bytes * (bottom - top) as usize];
    let mut other = vec![0u8; row_bytes];
    let mut any = false;
    for (y, out) in (top..bottom).zip(bits.chunks_exact_mut(row_bytes)) {
        copy_bits(
            row(a, y),
            (left - a.bounds[0]) as usize,
            width as usize,
            out,
        );
        copy_bits(
            row(b, y),
            (left - b.bounds[0]) as usize,
            width as usize,
            &mut other,
        );
        for (byte, other) in out.iter_mut().zip(&other) {
            *byte &= other;
            any |= *byte != 0;
        }
    }
    any.then(|| Mask {
        bounds: [left, top, width, bottom - top],
        bits: Arc::from(bits),
    })
}

/// Whether a pixel from `from` to `to` (exclusive) on row `y` is set.
fn any_in_row(mask: &Mask, y: i32, from: i32, to: i32) -> bool {
    let [left, top, width, height] = mask.bounds;
    if y < top || y >= top + height {
        return false;
    }
    let (from, to) = ((from - left).max(0), (to - left).min(width));
    if from >= to {
        return false;
    }
    let bits = row(mask, y);
    let (first, last) = (from as usize / 8, (to - 1) as usize / 8);
    (first..=last).any(|index| {
        let mut byte = bits[index];
        if index == first {
            byte &= 0xff >> (from % 8);
        }
        if index == last {
            byte &= 0xff << (7 - (to - 1) % 8);
        }
        byte != 0
    })
}

/// How far from its line a stroke reaches at `pressure`: half its width,
/// and for a spray as far as its particles land.
fn reach(stroke: &Stroke, pressure: f32) -> f64 {
    let brush = &stroke.brush;
    let dynamics = f64::from(brush.size_dynamics);
    let scale = 1.0 - dynamics + f64::from(pressure.clamp(0.0, 1.0)) * dynamics;
    let width = f64::from(stroke.width);
    let line = (width * scale).max(0.5) * 0.5;
    match brush.engine {
        BrushEngine::Line | BrushEngine::Airbrush => line,
        // A block of whole pixels from the point's pixel, at any pressure.
        BrushEngine::Pixel => width.round().max(1.0) * std::f64::consts::FRAC_1_SQRT_2 + 1.0,
        BrushEngine::Spray => {
            let particle =
                f64::from(brush.particle_size) * (1.0 + 0.75 * f64::from(brush.size_jitter));
            line.max(width * 0.5 * (f64::from(brush.scatter) + particle))
        }
    }
}

fn stroke_touches(stroke: &Stroke, area: &Area) -> bool {
    let points: Vec<([f64; 2], f64)> = stroke
        .points
        .iter()
        .map(|point| {
            (
                [f64::from(point.x), f64::from(point.y)],
                reach(stroke, point.pressure),
            )
        })
        .collect();
    let farthest = points.iter().map(|(_, reach)| *reach).fold(0.0, f64::max);
    let (mut low, mut high) = ([f64::MAX; 2], [f64::MIN; 2]);
    for ([x, y], _) in &points {
        low = [low[0].min(*x), low[1].min(*y)];
        high = [high[0].max(*x), high[1].max(*y)];
    }
    if !area.may_hold(
        low.map(|value| value - farthest),
        high.map(|value| value + farthest),
    ) {
        return false;
    }
    if let [(point, reach)] = points[..] {
        return capsule_touches(point, point, reach, area);
    }
    points
        .windows(2)
        .any(|pair| capsule_touches(pair[0].0, pair[1].0, pair[0].1.max(pair[1].1), area))
}

/// Whether a pixel of `area` has its centre within `reach` of the segment
/// from `a` to `b`.
fn capsule_touches(a: [f64; 2], b: [f64; 2], reach: f64, area: &Area) -> bool {
    let low = [a[0].min(b[0]) - reach, a[1].min(b[1]) - reach];
    let high = [a[0].max(b[0]) + reach, a[1].max(b[1]) + reach];
    if !area.may_hold(low, high) {
        return false;
    }
    let [_, top, _, height] = area.mask.bounds;
    let first = ((a[1].min(b[1]) - reach - 0.5).ceil() as i32).max(top);
    let last = ((a[1].max(b[1]) + reach - 0.5).floor() as i32).min(top + height - 1);
    (first..=last).any(|y| {
        let centre = f64::from(y) + 0.5;
        capsule_span(a, b, reach, centre).is_some_and(|[from, to]| {
            area.may_hold([from, centre], [to, centre]) && {
                let from = (from - 0.5).ceil() as i32;
                let to = (to - 0.5).floor() as i32 + 1;
                any_in_row(&area.mask, y, from, to)
            }
        })
    })
}

/// Where the line across at `y` is within `reach` of the segment from `a`
/// to `b`: the least and greatest x.
fn capsule_span(a: [f64; 2], b: [f64; 2], reach: f64, y: f64) -> Option<[f64; 2]> {
    let mut span: Option<[f64; 2]> = None;
    let mut add = |from: f64, to: f64| {
        if from <= to {
            span = Some(span.map_or([from, to], |[low, high]| [low.min(from), high.max(to)]));
        }
    };
    for [x, centre] in [a, b] {
        let across = reach * reach - (y - centre) * (y - centre);
        if across >= 0.0 {
            let half = across.sqrt();
            add(x - half, x + half);
        }
    }
    let [dx, dy] = [b[0] - a[0], b[1] - a[1]];
    let length2 = dx * dx + dy * dy;
    if length2 > 0.0 {
        // Along the segment within its ends, and across it within reach;
        // each a bound on k·x + m.
        let (mut from, mut to) = (f64::MIN, f64::MAX);
        let mut bound = |k: f64, m: f64, low: f64, high: f64| {
            if k == 0.0 {
                if m < low || m > high {
                    (from, to) = (f64::MAX, f64::MIN);
                }
            } else {
                let (p, q) = ((low - m) / k, (high - m) / k);
                from = from.max(p.min(q));
                to = to.min(p.max(q));
            }
        };
        let rise = y - a[1];
        bound(dx, -a[0] * dx + rise * dy, 0.0, length2);
        let side = reach * length2.sqrt();
        bound(dy, -a[0] * dy - rise * dx, -side, side);
        add(from, to);
    }
    span
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edit::{Outcome, commit};
    use crate::ops::{Op, Sampling};
    use crate::selection::Shape;
    use crate::store::{Brush, Point};

    fn stroke(points: &[[f32; 2]], width: f32, color: [u8; 4]) -> Stroke {
        Stroke {
            points: points
                .iter()
                .map(|&[x, y]| Point {
                    x,
                    y,
                    pressure: 1.0,
                })
                .collect(),
            color: Rgba8(color),
            width,
            brush: Brush::DEFAULT,
            seed: 7,
        }
    }

    fn rectangle(from: [f64; 2], to: [f64; 2], canvas: [u32; 2]) -> Selection {
        Selection::of_shape(&Shape::Rectangle(from, to), canvas).expect("covers pixels")
    }

    /// A 100² document with one layer holding `ops`, its strokes as
    /// `strokes` and masks as `masks` say.
    fn document(strokes: Vec<Stroke>, masks: Vec<Mask>, ops: Vec<Op>) -> (Document, LayerId) {
        let mut document = Document::new([100, 100]);
        let layer = document.layers[0].id;
        for (index, stroke) in strokes.into_iter().enumerate() {
            document
                .store
                .strokes
                .insert(StrokeId(index as u32), stroke);
        }
        for (index, mask) in masks.into_iter().enumerate() {
            document.store.masks.insert(MaskId(index as u32), mask);
        }
        let LayerKind::Paint(paint) = &mut document.layers[0].kind else {
            unreachable!("a new document starts with a paint layer");
        };
        paint.ops = ops;
        document.canvas = paint.final_size();
        document.check_structure().expect("valid");
        (document, layer)
    }

    fn paint(id: u32) -> Op {
        Op::Paint {
            stroke: StrokeId(id),
            clip: None,
        }
    }

    #[test]
    fn a_stroke_touches_where_its_width_reaches() {
        let black = [0, 0, 0, 255];
        // A line along y = 50 from x = 10 to 40, 10 wide: reaching y 45 to 55.
        let line = stroke(&[[10.0, 50.0], [40.0, 50.0]], 10.0, black);
        let (document, layer) = document(vec![line], vec![], vec![paint(0)]);
        let count = |from, to| touched(&document, layer, &rectangle(from, to, [100, 100])).colored;
        assert_eq!(
            count([20.0, 54.0], [30.0, 60.0]),
            1,
            "the edge, not the centre line"
        );
        assert_eq!(count([20.0, 56.0], [30.0, 60.0]), 0, "below the reach");
        assert_eq!(
            count([44.0, 50.0], [46.0, 51.0]),
            1,
            "past the end, inside the round cap"
        );
        assert_eq!(
            count([44.0, 44.0], [46.0, 46.0]),
            0,
            "outside the cap's corner"
        );
        assert_eq!(count([0.0, 0.0], [100.0, 10.0]), 0);
    }

    #[test]
    fn a_dot_and_a_steep_line_are_found() {
        let dot = stroke(&[[50.0, 50.0]], 6.0, [0, 0, 0, 255]);
        let steep = stroke(&[[10.0, 10.0], [12.0, 90.0]], 2.0, [0, 0, 0, 255]);
        let (document, layer) = document(vec![dot, steep], vec![], vec![paint(0), paint(1)]);
        let count = |from, to| touched(&document, layer, &rectangle(from, to, [100, 100])).colored;
        assert_eq!(count([51.0, 51.0], [52.0, 52.0]), 1);
        assert_eq!(
            count([52.0, 52.0], [53.0, 53.0]),
            0,
            "the centre is 3.5 away"
        );
        assert_eq!(count([10.0, 49.0], [12.0, 50.0]), 1);
        assert_eq!(count([14.0, 49.0], [20.0, 50.0]), 0);
    }

    #[test]
    fn a_clip_and_a_fill_coverage_limit_what_is_touched() {
        let left = rectangle([0.0, 0.0], [50.0, 100.0], [100, 100]);
        let line = stroke(&[[10.0, 50.0], [90.0, 50.0]], 4.0, [0, 0, 0, 255]);
        let ops = vec![
            Op::Paint {
                stroke: StrokeId(0),
                clip: Some(MaskId(0)),
            },
            Op::Fill {
                coverage: MaskId(0),
                color: Rgba8([255, 0, 0, 255]),
                antialias: false,
                clip: None,
            },
        ];
        let (document, layer) = document(vec![line], vec![left.mask().clone()], ops);
        let right = rectangle([60.0, 0.0], [100.0, 100.0], [100, 100]);
        assert_eq!(touched(&document, layer, &right), Touched::default());
        let middle = rectangle([40.0, 40.0], [60.0, 60.0], [100, 100]);
        let found = touched(&document, layer, &middle);
        assert_eq!((found.colored, found.sized), (2, 1));
        assert_eq!(found.color, None, "black and red");
        assert_eq!(found.width, Some(4.0));
    }

    #[test]
    fn strokes_before_a_crop_or_resample_are_found_where_they_ended_up() {
        let line = stroke(&[[10.0, 10.0], [20.0, 10.0]], 2.0, [0, 0, 0, 255]);
        let ops = vec![
            paint(0),
            Op::Crop {
                offset: [30, 30],
                size: [100, 100],
            },
            Op::Resample {
                size: [50, 50],
                sampling: Sampling::Smooth,
            },
        ];
        let (document, layer) = document(vec![line], vec![], ops);
        // At 40..50, 40 after the crop, and 20..25, 20 after halving.
        let count = |from, to| touched(&document, layer, &rectangle(from, to, [50, 50])).colored;
        assert_eq!(count([21.0, 19.0], [23.0, 21.0]), 1);
        assert_eq!(count([5.0, 4.0], [10.0, 6.0]), 0, "where it was drawn");
    }

    #[test]
    fn restyling_renames_changed_strokes_and_undoes_in_one_commit() {
        let red = Rgba8([255, 0, 0, 255]);
        let black = [0, 0, 0, 255];
        let a = stroke(&[[10.0, 10.0], [20.0, 10.0]], 3.0, black);
        let b = stroke(&[[10.0, 80.0], [20.0, 80.0]], 3.0, black);
        let eraser = stroke(&[[10.0, 12.0], [20.0, 12.0]], 5.0, black);
        let ops = vec![
            paint(0),
            paint(1),
            Op::Erase {
                stroke: StrokeId(2),
                clip: None,
            },
        ];
        let (mut document, layer) = document(vec![a, b, eraser], vec![], ops);
        let before = document.clone();
        let top = rectangle([0.0, 0.0], [100.0, 30.0], [100, 100]);
        let style = Style {
            color: Some(red),
            width: Some(8.0),
        };
        let changes = restyle(&document, layer, &top, style).expect("a paint layer");
        let Ok(Outcome::Committed(undo)) = commit(&mut document, changes) else {
            panic!("committed");
        };
        let LayerKind::Paint(paint) = &document.layer(layer).expect("kept").kind else {
            unreachable!();
        };
        let [
            Op::Paint { stroke: a, .. },
            Op::Paint { stroke: b, .. },
            Op::Erase { stroke: e, .. },
        ] = paint.ops[..]
        else {
            panic!("the same operations");
        };
        assert_eq!(b, StrokeId(1), "out of the selection");
        assert!(a.0 >= 3 && e.0 >= 3 && a != e, "new ids");
        let store = &document.store.strokes;
        assert_eq!((store[&a].color, store[&a].width), (red, 8.0));
        assert_eq!(
            (store[&e].color.0, store[&e].width),
            (black, 8.0),
            "an eraser has no colour"
        );
        assert_eq!(store[&a].seed, 7, "it moves as before");
        assert!(!store.contains_key(&StrokeId(0)) && !store.contains_key(&StrokeId(2)));
        assert_eq!(store.len(), 3);

        let again = restyle(&document, layer, &top, style).expect("a paint layer");
        assert!(again.is_empty(), "nothing left to change");
        commit(&mut document, undo).expect("undoes");
        assert_eq!(document, before);
    }

    #[test]
    fn a_stroke_another_layer_draws_stays_stored() {
        let line = stroke(&[[10.0, 10.0], [20.0, 10.0]], 3.0, [0, 0, 0, 255]);
        let (mut document, layer) = document(vec![line], vec![], vec![paint(0)]);
        let mut copy = document.layers[0].clone();
        copy.id = LayerId(layer.0 + 1);
        document.layers.push(copy);
        let all = Selection::all([100, 100]).expect("a canvas");
        let style = Style {
            color: None,
            width: Some(limits_width()),
        };
        let changes = restyle(&document, layer, &all, style).expect("a paint layer");
        commit(&mut document, changes).expect("valid");
        assert!(document.store.strokes.contains_key(&StrokeId(0)));
        assert_eq!(document.store.strokes.len(), 2);
    }

    #[test]
    fn strokes_in_a_merged_section_are_found_and_changed() {
        let line = stroke(&[[10.0, 10.0], [20.0, 10.0]], 3.0, [0, 0, 0, 255]);
        let section = Op::Isolated(Box::new(crate::ops::Section {
            ops: vec![paint(0)],
            opacity: 0.5,
            wobble: None,
        }));
        let (mut document, layer) = document(vec![line], vec![], vec![section]);
        let all = Selection::all([100, 100]).expect("a canvas");
        assert_eq!(touched(&document, layer, &all).colored, 1);
        let style = Style {
            color: Some(Rgba8([9, 9, 9, 255])),
            width: None,
        };
        let changes = restyle(&document, layer, &all, style).expect("a paint layer");
        commit(&mut document, changes).expect("valid");
        let LayerKind::Paint(paint) = &document.layer(layer).expect("kept").kind else {
            unreachable!();
        };
        let [Op::Isolated(section)] = &paint.ops[..] else {
            panic!("still one section");
        };
        let [Op::Paint { stroke, .. }] = section.ops[..] else {
            panic!("still one stroke");
        };
        assert_eq!(document.store.strokes[&stroke].color, Rgba8([9, 9, 9, 255]));
    }

    fn limits_width() -> f32 {
        *crate::store::limits::STROKE_WIDTH.end()
    }

    #[test]
    fn a_width_out_of_range_is_refused() {
        let line = stroke(&[[10.0, 10.0], [20.0, 10.0]], 3.0, [0, 0, 0, 255]);
        let (mut document, layer) = document(vec![line], vec![], vec![paint(0)]);
        let before = document.clone();
        let all = Selection::all([100, 100]).expect("a canvas");
        let style = Style {
            color: None,
            width: Some(limits_width() * 2.0),
        };
        let changes = restyle(&document, layer, &all, style).expect("a paint layer");
        assert!(commit(&mut document, changes).is_err());
        assert_eq!(document, before);
    }
}

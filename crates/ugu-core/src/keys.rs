// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! Layer keyframes and animation folders.
//!
//! A layer or group with keys is moved, turned, scaled and flipped on each
//! frame by the pose its keys give that frame; between keys the pose is
//! eased from one key to the next, and after the last key it eases towards
//! the first one a cycle later, so the animation loops. A group's pose
//! applies to every layer in it, after their own.
//!
//! A group with exposures is an animation folder: on each frame only the
//! child exposed then is shown. An exposure holds until the next one, and
//! the frames before the first exposure hold the last one.

use crate::document::{Document, Group, Layer, LayerId, LayerKind};
use crate::ops::Affine;

pub mod limits {
    /// Pixels a key may move a layer, and where its pivot may be.
    pub const OFFSET: f64 = 32_767.0;
    pub const SCALE: std::ops::RangeInclusive<f64> = 0.05..=20.0;
    /// Degrees either way.
    pub const ROTATION: f64 = 3_600.0;
}

/// How a pose eases from a key to the next.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Ease {
    #[default]
    Linear,
    /// Slow at both ends (smoothstep).
    Smooth,
    /// Stays until the next key.
    Hold,
}

/// Where a layer is on a frame, relative to where it is drawn.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pose {
    /// Document pixels.
    pub offset: [f64; 2],
    /// Degrees, clockwise on screen.
    pub rotation: f64,
    /// The same both ways; 1 is the drawn size.
    pub scale: f64,
    /// Mirrored left to right.
    pub flip: bool,
    /// Multiplies the layer's own opacity; 1 shows it as it is.
    pub opacity: f64,
}

impl Pose {
    pub const REST: Self = Self {
        offset: [0.0, 0.0],
        rotation: 0.0,
        scale: 1.0,
        flip: false,
        opacity: 1.0,
    };

    /// Scales and flips about `pivot`, turns about it, then moves; `None`
    /// when that leaves every pixel where it is.
    pub fn transform(&self, pivot: [f64; 2]) -> Option<Affine> {
        let geometry = Self {
            opacity: 1.0,
            ..*self
        };
        if geometry == Self::REST {
            return None;
        }
        let horizontal = if self.flip { -self.scale } else { self.scale };
        let scaled = Affine::scaling_about([horizontal, self.scale], pivot);
        let [x, y] = pivot;
        let (sin, cos) = turn(self.rotation);
        let turned = Affine([
            cos,
            -sin,
            x - cos * x + sin * y,
            sin,
            cos,
            y - sin * x - cos * y,
        ]);
        Some(
            scaled
                .then(turned)
                .then(Affine::translation(self.offset[0], self.offset[1])),
        )
    }

    fn valid(&self) -> bool {
        self.offset
            .iter()
            .all(|value| value.is_finite() && value.abs() <= limits::OFFSET)
            && self.rotation.is_finite()
            && self.rotation.abs() <= limits::ROTATION
            && limits::SCALE.contains(&self.scale)
            && (0.0..=1.0).contains(&self.opacity)
    }
}

/// The sine and cosine of `degrees`, exact on quarter turns so that they
/// move whole pixels to whole pixels.
fn turn(degrees: f64) -> (f64, f64) {
    let quarter = degrees / 90.0;
    if quarter.fract() == 0.0 {
        return match quarter.rem_euclid(4.0) as u8 {
            0 => (0.0, 1.0),
            1 => (1.0, 0.0),
            2 => (0.0, -1.0),
            _ => (-1.0, 0.0),
        };
    }
    degrees.to_radians().sin_cos()
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Key {
    pub frame: u32,
    pub pose: Pose,
    /// From this key to the next.
    pub ease: Ease,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Keys {
    /// What the layer turns, scales and flips about, in document pixels.
    pub pivot: [f64; 2],
    /// By frame, at most one per frame.
    pub keys: Vec<Key>,
}

impl Keys {
    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }

    /// The key on `frame`.
    pub fn key(&self, frame: u32) -> Option<&Key> {
        self.keys.iter().find(|key| key.frame == frame)
    }

    /// The pose on `frame` of a loop of `frames`; `None` without keys.
    pub fn pose(&self, frame: u32, frames: u32) -> Option<Pose> {
        let first = self.keys.first()?;
        let last = self.keys.last().expect("not empty");
        let (from, to, at) = match self.keys.iter().rposition(|key| key.frame <= frame) {
            Some(index) => match self.keys.get(index + 1) {
                Some(next) => (&self.keys[index], next.frame, frame),
                None => (last, first.frame + frames, frame),
            },
            None => (last, first.frame + frames, frame + frames),
        };
        let next = self
            .keys
            .iter()
            .find(|key| key.frame == to % frames.max(1))
            .unwrap_or(first);
        if std::ptr::eq(from, next) || from.ease == Ease::Hold {
            return Some(from.pose);
        }
        let t = f64::from(at - from.frame) / f64::from(to - from.frame);
        let t = match from.ease {
            Ease::Smooth => t * t * (3.0 - 2.0 * t),
            _ => t,
        };
        let mix = |a: f64, b: f64| a + (b - a) * t;
        let (a, b) = (from.pose, next.pose);
        Some(Pose {
            offset: [mix(a.offset[0], b.offset[0]), mix(a.offset[1], b.offset[1])],
            rotation: mix(a.rotation, b.rotation),
            scale: mix(a.scale, b.scale),
            flip: a.flip,
            opacity: mix(a.opacity, b.opacity),
        })
    }

    /// How much the keys fade the layer on `frame`: 1 without keys.
    pub fn opacity(&self, frame: u32, frames: u32) -> f64 {
        self.pose(frame, frames).map_or(1.0, |pose| pose.opacity)
    }

    /// Whether any key fades the layer.
    pub fn fades(&self) -> bool {
        self.keys.iter().any(|key| key.pose.opacity != 1.0)
    }

    /// Where the layer's pixels go on `frame`; `None` where they stay.
    pub fn at(&self, frame: u32, frames: u32) -> Option<Affine> {
        self.pose(frame, frames)?.transform(self.pivot)
    }

    /// Puts `key` on its frame, in place of one there.
    pub fn set(&mut self, key: Key) {
        match self
            .keys
            .binary_search_by_key(&key.frame, |each| each.frame)
        {
            Ok(index) => self.keys[index] = key,
            Err(index) => self.keys.insert(index, key),
        }
    }

    /// Takes away the key on `frame`; `false` when there is none.
    pub fn remove(&mut self, frame: u32) -> bool {
        let before = self.keys.len();
        self.keys.retain(|key| key.frame != frame);
        self.keys.len() != before
    }

    /// The frame of the key before (`-1`) or after (`1`) `frame`, going
    /// round the loop.
    pub fn neighbour(&self, frame: u32, direction: i32) -> Option<u32> {
        let mut frames = self.keys.iter().map(|key| key.frame);
        if direction < 0 {
            let last = self.keys.last().map(|key| key.frame);
            frames.rev().find(|each| *each < frame).or(last)
        } else {
            let first = self.keys.first().map(|key| key.frame);
            frames.find(|each| *each > frame).or(first)
        }
    }

    fn check(&self, frames: u32) -> bool {
        self.pivot
            .iter()
            .all(|value| value.is_finite() && value.abs() <= limits::OFFSET)
            && self
                .keys
                .iter()
                .all(|key| key.frame < frames && key.pose.valid())
            && self
                .keys
                .windows(2)
                .all(|pair| pair[0].frame < pair[1].frame)
    }

    /// Drops the keys from `frames` on; `true` when it dropped any.
    fn trim(&mut self, frames: u32) -> bool {
        let before = self.keys.len();
        self.keys.retain(|key| key.frame < frames);
        self.keys.len() != before
    }
}

/// From `frame` until the next exposure, `cel` is the child shown; `None`
/// shows nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Exposure {
    pub frame: u32,
    pub cel: Option<LayerId>,
}

/// The child of an animation folder shown on `frame`, given its exposures
/// in frame order.
pub fn exposed(exposures: &[Exposure], frame: u32) -> Option<LayerId> {
    exposures
        .iter()
        .rev()
        .find(|exposure| exposure.frame <= frame)
        .or(exposures.last())
        .and_then(|exposure| exposure.cel)
}

/// Puts `cel` on `frame`, in place of an exposure there.
pub fn expose(exposures: &mut Vec<Exposure>, frame: u32, cel: Option<LayerId>) {
    let exposure = Exposure { frame, cel };
    match exposures.binary_search_by_key(&frame, |each| each.frame) {
        Ok(index) => exposures[index] = exposure,
        Err(index) => exposures.insert(index, exposure),
    }
}

impl Group {
    /// Whether this is an animation folder.
    pub fn is_folder(&self) -> bool {
        self.cels.is_some()
    }

    /// Whether `child` is shown on `frame`: always in a plain group.
    pub fn shows(&self, child: LayerId, frame: u32) -> bool {
        self.cels
            .as_ref()
            .is_none_or(|cels| exposed(cels, frame) == Some(child))
    }
}

impl Layer {
    pub fn keys(&self) -> &Keys {
        match &self.kind {
            LayerKind::Paint(paint) => &paint.keys,
            LayerKind::Group(group) => &group.keys,
        }
    }

    pub fn keys_mut(&mut self) -> &mut Keys {
        match &mut self.kind {
            LayerKind::Paint(paint) => &mut paint.keys,
            LayerKind::Group(group) => &mut group.keys,
        }
    }

    /// The animation folder's exposures; `None` for anything else.
    pub fn cels(&self) -> Option<&[Exposure]> {
        match &self.kind {
            LayerKind::Group(group) => group.cels.as_deref(),
            LayerKind::Paint(_) => None,
        }
    }
}

impl Document {
    /// The layers holding `id`, outermost first; `None` when it is missing.
    pub fn ancestors(&self, id: LayerId) -> Option<Vec<&Layer>> {
        fn walk<'a>(layers: &'a [Layer], id: LayerId, path: &mut Vec<&'a Layer>) -> bool {
            for layer in layers {
                if layer.id == id {
                    return true;
                }
                if let LayerKind::Group(group) = &layer.kind {
                    path.push(layer);
                    if walk(&group.children, id, path) {
                        return true;
                    }
                    path.pop();
                }
            }
            false
        }
        let mut path = Vec::new();
        walk(&self.layers, id, &mut path).then_some(path)
    }

    /// Where `id`'s own pixels go on `frame`: its pose, then each group's
    /// holding it, innermost first. `None` where they stay.
    pub fn transform(&self, id: LayerId, frame: u32) -> Option<Affine> {
        let layer = self.layer(id)?;
        let mut path = self.ancestors(id)?;
        path.push(layer);
        path.iter()
            .rev()
            .filter_map(|layer| layer.keys().at(frame, self.frames))
            .reduce(|inner, outer| inner.then(outer))
    }

    /// Whether any layer has keys or is an animation folder.
    pub fn is_animated(&self) -> (bool, bool) {
        fn walk(layers: &[Layer], found: &mut (bool, bool)) {
            for layer in layers {
                found.0 |= !layer.keys().is_empty();
                if let LayerKind::Group(group) = &layer.kind {
                    found.1 |= group.is_folder();
                    walk(&group.children, found);
                }
            }
        }
        let mut found = (false, false);
        walk(&self.layers, &mut found);
        found
    }

    /// Whether any key fades its layer.
    pub fn fades(&self) -> bool {
        fn walk(layers: &[Layer]) -> bool {
            layers.iter().any(|layer| {
                layer.keys().fades()
                    || matches!(&layer.kind, LayerKind::Group(group) if walk(&group.children))
            })
        }
        walk(&self.layers)
    }
}

/// Whether `layer`'s keys and exposures fit a loop of `frames`.
pub(crate) fn check(layer: &Layer, frames: u32) -> bool {
    if !layer.keys().check(frames) {
        return false;
    }
    let Some(cels) = layer.cels() else {
        return true;
    };
    let LayerKind::Group(group) = &layer.kind else {
        unreachable!("only groups have exposures");
    };
    cels.iter().all(|exposure| {
        exposure.frame < frames
            && exposure
                .cel
                .is_none_or(|cel| group.children.iter().any(|child| child.id == cel))
    }) && cels.windows(2).all(|pair| pair[0].frame < pair[1].frame)
}

/// `layer` with the keys and exposures from `frames` on dropped, children
/// included; `None` when nothing is dropped.
pub(crate) fn trimmed(layer: &Layer, frames: u32) -> Option<Layer> {
    let mut copy = layer.clone();
    let changed = trim(&mut copy, frames);
    changed.then_some(copy)
}

fn trim(layer: &mut Layer, frames: u32) -> bool {
    let mut changed = layer.keys_mut().trim(frames);
    if let LayerKind::Group(group) = &mut layer.kind {
        if let Some(cels) = &mut group.cels {
            let before = cels.len();
            cels.retain(|exposure| exposure.frame < frames);
            changed |= cels.len() != before;
        }
        for child in &mut group.children {
            changed |= trim(child, frames);
        }
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(frame: u32, x: f64, ease: Ease) -> Key {
        Key {
            frame,
            pose: Pose {
                offset: [x, 0.0],
                ..Pose::REST
            },
            ease,
        }
    }

    fn x(keys: &Keys, frame: u32) -> f64 {
        keys.pose(frame, 30).unwrap().offset[0]
    }

    #[test]
    fn no_keys_leave_the_layer_where_it_is_and_one_key_holds_everywhere() {
        let mut keys = Keys::default();
        assert_eq!(keys.pose(3, 30), None);
        assert_eq!(keys.at(3, 30), None);
        keys.set(key(10, 4.0, Ease::Linear));
        for frame in [0, 10, 29] {
            assert_eq!(x(&keys, frame), 4.0);
        }
    }

    #[test]
    fn two_keys_go_there_and_back_round_the_loop() {
        let mut keys = Keys::default();
        keys.set(key(0, 0.0, Ease::Linear));
        keys.set(key(15, 30.0, Ease::Linear));
        assert_eq!(x(&keys, 0), 0.0);
        assert_eq!(x(&keys, 5), 10.0);
        assert_eq!(x(&keys, 15), 30.0);
        assert_eq!(x(&keys, 20), 20.0);
        assert_eq!(x(&keys, 29), 2.0);
        // Before the first key, the last one eases towards it.
        let mut late = Keys::default();
        late.set(key(10, 0.0, Ease::Linear));
        late.set(key(20, 20.0, Ease::Linear));
        assert_eq!(x(&late, 25), 15.0);
        assert_eq!(x(&late, 5), 5.0);
    }

    #[test]
    fn eases_shape_the_way_between_keys() {
        let mut keys = Keys::default();
        keys.set(key(0, 0.0, Ease::Smooth));
        keys.set(key(10, 100.0, Ease::Hold));
        assert_eq!(x(&keys, 5), 50.0);
        assert!(x(&keys, 2) < 20.0);
        assert!(x(&keys, 8) > 80.0);
        for frame in [10, 20, 29] {
            assert_eq!(x(&keys, frame), 100.0);
        }
    }

    #[test]
    fn a_flip_holds_until_the_next_key() {
        let mut keys = Keys::default();
        keys.set(key(0, 0.0, Ease::Linear));
        let mut flipped = key(10, 0.0, Ease::Linear);
        flipped.pose.flip = true;
        keys.set(flipped);
        assert!(!keys.pose(9, 30).unwrap().flip);
        assert!(keys.pose(10, 30).unwrap().flip);
        assert!(keys.pose(29, 30).unwrap().flip);
    }

    #[test]
    fn quarter_turns_and_flips_map_whole_pixels_to_whole_pixels() {
        let pose = Pose {
            rotation: 90.0,
            flip: true,
            ..Pose::REST
        };
        let transform = pose.transform([10.0, 10.0]).unwrap();
        assert_eq!(transform.apply([12.0, 10.0]), [10.0, 8.0]);
        assert_eq!(Pose::REST.transform([3.0, 4.0]), None);
        let pose = Pose {
            rotation: -270.0,
            ..Pose::REST
        };
        assert_eq!(pose.transform([0.0, 0.0]).unwrap().0[0], 0.0);
    }

    #[test]
    fn keys_step_to_neighbours_round_the_loop() {
        let mut keys = Keys::default();
        for frame in [3, 9] {
            keys.set(key(frame, 0.0, Ease::Linear));
        }
        assert_eq!(keys.neighbour(5, 1), Some(9));
        assert_eq!(keys.neighbour(9, 1), Some(3));
        assert_eq!(keys.neighbour(5, -1), Some(3));
        assert_eq!(keys.neighbour(3, -1), Some(9));
        assert!(keys.remove(3));
        assert!(!keys.remove(3));
    }

    #[test]
    fn exposures_hold_until_the_next_and_loop_before_the_first() {
        let cel = |id| Some(LayerId(id));
        let mut exposures = Vec::new();
        assert_eq!(exposed(&exposures, 0), None);
        expose(&mut exposures, 4, cel(1));
        expose(&mut exposures, 8, cel(2));
        expose(&mut exposures, 12, None);
        assert_eq!(exposed(&exposures, 4), cel(1));
        assert_eq!(exposed(&exposures, 7), cel(1));
        assert_eq!(exposed(&exposures, 9), cel(2));
        assert_eq!(exposed(&exposures, 12), None);
        expose(&mut exposures, 12, cel(1));
        assert_eq!(exposed(&exposures, 2), cel(1));
        assert_eq!(exposures.len(), 3);
    }
}

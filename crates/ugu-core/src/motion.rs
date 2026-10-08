// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! Where each stroke point sits on each frame (render revision 1).
//!
//! Every value is a pure function of the stroke's seed, the frame and the
//! position along the stroke, so a preview, an export, a redraw after undo
//! and a reopened file all move the same way. The formulas and constants of
//! Classic, Smooth, Stepped and broken lines are those of 2.2.13, checked
//! against values computed by its C++ code; pixels are not promised to
//! match, only the motion.

use crate::ops::{Motion, MotionStyle};

/// Hashes that stand in for a random generator with no state.
pub mod noise {
    /// SplitMix64's finaliser.
    fn mix(mut value: u64) -> u64 {
        value = value.wrapping_add(0x9e37_79b9_7f4a_7c15);
        value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        value ^ (value >> 31)
    }

    /// From -1 (inclusive) to 1 (exclusive).
    pub fn signed(seed: u64, frame: u32, index: i64, channel: u64) -> f64 {
        let mut value = seed;
        value ^= mix((u64::from(frame) + 1).wrapping_mul(0x517c_c1b7_2722_0a95));
        value ^= mix((index as u64)
            .wrapping_add(4099)
            .wrapping_mul(0x6eed_0e9d_a4d9_4a4f));
        value ^= channel;
        let unit = (mix(value) >> 11) as f64 / (1u64 << 53) as f64;
        unit * 2.0 - 1.0
    }

    /// `signed` at whole coordinates, joined by smoothstep in between.
    pub fn smooth(seed: u64, frame: u32, coordinate: f64, channel: u64) -> f64 {
        Smooth::new(seed, frame, channel).at(coordinate)
    }

    /// `smooth` of one seed, frame and channel at coordinates that mostly
    /// rise a little at a time, as along a stroke: it keeps the last whole
    /// coordinate's two values instead of hashing them again.
    #[derive(Clone, Copy, Debug)]
    pub struct Smooth {
        seed: u64,
        frame: u32,
        channel: u64,
        /// The last left coordinate and `signed` there and one to the right.
        cell: Option<(i64, f64, f64)>,
    }

    impl Smooth {
        pub fn new(seed: u64, frame: u32, channel: u64) -> Self {
            Self {
                seed,
                frame,
                channel,
                cell: None,
            }
        }

        pub fn at(&mut self, coordinate: f64) -> f64 {
            let left = coordinate.floor();
            // Also false for NaN and infinities.
            if !(left >= i64::MIN as f64 && left < i64::MAX as f64) {
                return 0.0;
            }
            let fraction = coordinate - left;
            let blend = fraction * fraction * (3.0 - 2.0 * fraction);
            let left = left as i64;
            let signed = |index: i64| signed(self.seed, self.frame, index, self.channel);
            let (a, b) = match self.cell {
                Some((cell, a, b)) if cell == left => (a, b),
                Some((cell, _, b)) if cell.checked_add(1) == Some(left) => {
                    (b, signed(left.wrapping_add(1)))
                }
                _ => (signed(left), signed(left.wrapping_add(1))),
            };
            self.cell = Some((left, a, b));
            a + (b - a) * blend
        }
    }

    /// From 0 (inclusive) to 1 (exclusive).
    pub fn unit(seed: u64, frame: u32, index: i64, channel: u64) -> f64 {
        (signed(seed, frame, index, channel) + 1.0) * 0.5
    }
}

/// The frame of a cycle of `frames` that `frame` falls on, also for frames
/// before the first.
pub fn frame_in_cycle(frame: i64, frames: u32) -> u32 {
    frame.rem_euclid(i64::from(frames.max(1))) as u32
}

pub mod classic {
    use super::{Displacer, noise};

    pub(super) const SWAY_WAVELENGTH: f64 = 26.0;
    pub(super) const DETAIL_WAVELENGTH: f64 = 9.0;
    const WIDTH_CHANNEL: u64 = 0x1a67_d3c4;
    pub(super) const SWAY_CHANNEL: u64 = 0xb529_7a4d;
    pub(super) const DETAIL_CHANNEL: u64 = 0x1b56_c4e9;
    pub(super) const TANGENT_CHANNEL: u64 = 0x68e3_1da4;

    /// How far points move, in document pixels, for a stroke of `width` at
    /// `wobble` (the layer's amount times the brush's wobble scale).
    pub fn amplitude(width: f64, wobble: f64) -> f64 {
        wobble.max(0.0) * (0.82 + width.min(40.0) * 0.018)
    }

    /// No point moves further than this from where it was drawn.
    pub fn max_displacement(width: f64, wobble: f64) -> f64 {
        amplitude(width, wobble) * 1.3
    }

    /// The stroke's width on `frame`, which varies by up to 2.5%.
    pub fn width(width: f64, seed: u64, frame: u32, wobble: f64) -> f64 {
        let scale = (wobble / 1.6).clamp(0.0, 1.0);
        let noise = scale * noise::signed(seed, frame, 0, WIDTH_CHANNEL);
        (width * (1.0 + noise * 0.025)).max(0.5)
    }

    /// Moves a point `arc` pixels along its stroke, where the stroke runs in
    /// the unit direction `tangent`. The order of operations is part of the
    /// rule: it fixes the rounding.
    pub fn displace(
        position: [f64; 2],
        pressure: f64,
        tangent: [f64; 2],
        arc: f64,
        amplitude: f64,
        seed: u64,
        frame: u32,
    ) -> [f64; 2] {
        Displacer::classic(seed, frame, amplitude).displace(position, pressure, tangent, arc, 0)
    }
}

/// The poses a frame shows: `blend` of the way from `first` to `second`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pose {
    pub first: u32,
    pub second: u32,
    pub blend: f64,
}

/// The Smooth pose at `position` frames into a loop of `frames` frames and
/// `poses` poses; `None` unless 1 ≤ `poses` ≤ `frames` and `position` is
/// finite.
pub fn smooth_pose(position: f64, frames: u32, poses: u32) -> Option<Pose> {
    if !position.is_finite() || poses == 0 || poses > frames {
        return None;
    }
    if poses == 1 {
        return Some(Pose {
            first: 0,
            second: 0,
            blend: 0.0,
        });
    }
    let mut position = position % f64::from(frames);
    if position < 0.0 {
        position += f64::from(frames);
    }
    let phase = position * f64::from(poses) / f64::from(frames);
    let first = (phase.floor() as i64).clamp(0, i64::from(poses) - 1) as u32;
    let fraction = (phase - f64::from(first)).clamp(0.0, 1.0);
    Some(Pose {
        first,
        second: (first + 1) % poses,
        blend: fraction * fraction * (3.0 - 2.0 * fraction),
    })
}

/// The Stepped pose of `frame`; `None` unless 1 ≤ `poses` ≤ `frames`.
pub fn stepped_pose(frame: i64, frames: u32, poses: u32) -> Option<u32> {
    if poses == 0 || poses > frames {
        return None;
    }
    let frame = frame_in_cycle(frame, frames);
    Some((u64::from(frame) * u64::from(poses) / u64::from(frames)) as u32)
}

/// How strokes move under a `Motion` in a loop of some frames.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mover {
    motion: Motion,
    frames: u32,
}

impl Mover {
    /// Smooth and Stepped use at most one pose per frame of `frames`.
    pub fn new(motion: Motion, frames: u32) -> Self {
        let frames = frames.max(1);
        let poses = motion.poses.clamp(1, frames);
        Self {
            motion: Motion { poses, ..motion },
            frames,
        }
    }

    pub fn motion(&self) -> &Motion {
        &self.motion
    }

    /// The poses `frame` shows; Classic frames are their own poses.
    pub fn pose(&self, frame: u32) -> Pose {
        let frame = frame_in_cycle(i64::from(frame), self.frames);
        let (frames, poses) = (self.frames, self.motion.poses);
        let pose = match self.motion.style {
            MotionStyle::Classic => None,
            MotionStyle::Smooth => smooth_pose(f64::from(frame), frames, poses),
            MotionStyle::Stepped => {
                stepped_pose(i64::from(frame), frames, poses).map(|pose| Pose {
                    first: pose,
                    second: pose,
                    blend: 0.0,
                })
            }
        };
        pose.unwrap_or(Pose {
            first: frame,
            second: frame,
            blend: 0.0,
        })
    }

    /// The pose that decides which pieces of a broken line show on `frame`.
    pub fn visibility_pose(&self, frame: u32) -> u32 {
        self.pose(frame).first
    }

    /// The pieces a stroke of `seed` shows on `frame`; `None` when it shows
    /// whole.
    pub fn breaks(&self, seed: u64, frame: u32) -> Option<Breaks> {
        let motion = &self.motion;
        if !motion.broken {
            return None;
        }
        Breaks::new(
            seed,
            self.visibility_pose(frame),
            f64::from(motion.break_amount),
            f64::from(motion.break_range),
        )
    }

    /// The width of a stroke drawn `width` wide on `frame`, which varies by
    /// up to 2.5%.
    pub fn width(&self, width: f64, seed: u64, frame: u32, wobble: f64) -> f64 {
        let frame = frame_in_cycle(i64::from(frame), self.frames);
        if self.motion.style == MotionStyle::Classic {
            return classic::width(width, seed, frame, wobble);
        }
        let pose = self.pose(frame);
        let noise = self.random(seed, pose, 0, posed::WIDTH_CHANNEL);
        let scale = (wobble / 1.6).clamp(0.0, 1.0);
        (width * (1.0 + noise * scale * 0.025)).max(0.5)
    }

    /// Moves sample `index`, `arc` pixels along its stroke, as
    /// `classic::displace` does; `Displacer` moves a stroke's samples.
    #[expect(clippy::too_many_arguments)]
    pub fn displace(
        &self,
        position: [f64; 2],
        pressure: f64,
        tangent: [f64; 2],
        arc: f64,
        index: usize,
        amplitude: f64,
        seed: u64,
        frame: u32,
    ) -> [f64; 2] {
        self.displacer(seed, frame, amplitude)
            .displace(position, pressure, tangent, arc, index)
    }

    /// Moves the samples of a stroke of `seed` on `frame` by `amplitude`.
    pub fn displacer(&self, seed: u64, frame: u32, amplitude: f64) -> Displacer {
        let frame = frame_in_cycle(i64::from(frame), self.frames);
        if self.motion.style == MotionStyle::Classic {
            return Displacer::classic(seed, frame, amplitude);
        }
        let pose = self.pose(frame);
        let channels = [
            posed::SWAY_CHANNEL,
            posed::DETAIL_CHANNEL,
            posed::TANGENT_CHANNEL,
        ];
        let detail = f64::from(self.motion.detail - 1) / 23.0;
        Displacer {
            posed: Some(Posed {
                seed,
                pose,
                linked: f64::from(self.motion.linked),
                randomness: f64::from(self.motion.randomness),
                detail_wavelength: lerp(32.0, 5.0, detail),
            }),
            amplitude,
            noise: channels.map(|channel| {
                [
                    noise::Smooth::new(seed, pose.first, channel),
                    noise::Smooth::new(seed, pose.second, channel),
                    noise::Smooth::new(posed::LINKED_SEED, pose.first, channel),
                    noise::Smooth::new(posed::LINKED_SEED, pose.second, channel),
                ]
            }),
        }
    }

    /// `noise::signed` blended as `Displacer` blends smooth noise.
    fn random(&self, seed: u64, pose: Pose, index: i64, channel: u64) -> f64 {
        random(seed, pose, f64::from(self.motion.linked), index, channel)
    }
}

/// Moves the samples of one stroke on one frame. Taken in order along the
/// stroke, neighbouring samples share smooth noise, hashed once.
#[derive(Clone, Debug)]
pub struct Displacer {
    /// `None` for Classic.
    posed: Option<Posed>,
    amplitude: f64,
    /// Sway, detail and along the stroke; for Classic only the first of
    /// each is used, for Smooth and Stepped the stroke's own then the
    /// shared noise, each on the first then the second pose.
    noise: [[noise::Smooth; 4]; 3],
}

#[derive(Clone, Copy, Debug)]
struct Posed {
    seed: u64,
    pose: Pose,
    linked: f64,
    randomness: f64,
    detail_wavelength: f64,
}

impl Displacer {
    fn classic(seed: u64, frame: u32, amplitude: f64) -> Self {
        let channels = [
            classic::SWAY_CHANNEL,
            classic::DETAIL_CHANNEL,
            classic::TANGENT_CHANNEL,
        ];
        Self {
            posed: None,
            amplitude,
            noise: channels.map(|channel| [noise::Smooth::new(seed, frame, channel); 4]),
        }
    }

    /// Moves sample `index`, `arc` pixels along its stroke, where the
    /// stroke runs in the unit direction `tangent`. The order of operations
    /// is part of the rule: it fixes the rounding.
    pub fn displace(
        &mut self,
        position: [f64; 2],
        pressure: f64,
        tangent: [f64; 2],
        arc: f64,
        index: usize,
    ) -> [f64; 2] {
        let (across, along) = match self.posed {
            None => {
                let [sway, detail, along] = &mut self.noise;
                let sway = sway[0].at(arc / classic::SWAY_WAVELENGTH);
                let detail = detail[0].at(arc / classic::DETAIL_WAVELENGTH + 31.0);
                let along = along[0].at(arc / classic::SWAY_WAVELENGTH + 57.0);
                (sway * 0.72 + detail * 0.28, along)
            }
            Some(posed) => {
                let [sway, detail, along] = &mut self.noise;
                let sway = posed.smooth(sway, arc / 30.0);
                let detail = posed.smooth(detail, arc / posed.detail_wavelength + 31.0);
                let along = posed.smooth(along, arc / 30.0 + 57.0);
                let index = index.min(i32::MAX as usize) as i64;
                (
                    posed.toward_random(
                        sway * 0.68 + detail * 0.32,
                        index,
                        posed::RANDOM_NORMAL_CHANNEL,
                    ),
                    posed.toward_random(along, index, posed::RANDOM_TANGENT_CHANNEL),
                )
            }
        };
        let amplitude = self.amplitude;
        let normal = [-tangent[1], tangent[0]];
        let pressure_factor = 0.8 + pressure * 0.2;
        std::array::from_fn(|axis| {
            position[axis]
                + (normal[axis] * across * amplitude * pressure_factor
                    + tangent[axis] * along * amplitude * 0.3)
        })
    }
}

impl Posed {
    /// Smooth noise blended between the poses and, by `linked`, toward the
    /// noise every stroke shares. Each blend by 0 is skipped: x + (y − x)·0
    /// is x exactly.
    fn smooth(&self, noise: &mut [noise::Smooth; 4], coordinate: f64) -> f64 {
        let blend = self.pose.blend;
        let [own_first, own_second, shared_first, shared_second] = noise;
        let posed = |first: &mut noise::Smooth, second: &mut noise::Smooth| {
            let value = first.at(coordinate);
            if blend == 0.0 {
                return value;
            }
            lerp(value, second.at(coordinate), blend)
        };
        let own = posed(own_first, own_second);
        if self.linked == 0.0 {
            return own;
        }
        lerp(own, posed(shared_first, shared_second), self.linked)
    }

    fn toward_random(&self, base: f64, index: i64, channel: u64) -> f64 {
        if self.randomness == 0.0 {
            return base;
        }
        let random = random(self.seed, self.pose, self.linked, index, channel);
        lerp(base, random, self.randomness)
    }
}

/// `noise::signed` blended as `Posed::smooth` blends smooth noise.
fn random(seed: u64, pose: Pose, linked: f64, index: i64, channel: u64) -> f64 {
    let posed = |seed| {
        let first = noise::signed(seed, pose.first, index, channel);
        if pose.blend == 0.0 {
            return first;
        }
        lerp(
            first,
            noise::signed(seed, pose.second, index, channel),
            pose.blend,
        )
    };
    let own = posed(seed);
    if linked == 0.0 {
        return own;
    }
    lerp(own, posed(posed::LINKED_SEED), linked)
}

fn lerp(from: f64, to: f64, by: f64) -> f64 {
    from + (to - from) * by
}

/// Smooth and Stepped constants, 2.2.13's.
mod posed {
    /// The seed of the motion all strokes share.
    pub const LINKED_SEED: u64 = 0x94d0_49bb_1331_11eb;
    pub const WIDTH_CHANNEL: u64 = 0x739d_61a2;
    pub const SWAY_CHANNEL: u64 = 0xc45a_9137;
    pub const DETAIL_CHANNEL: u64 = 0x82f1_7b63;
    pub const TANGENT_CHANNEL: u64 = 0x5e2a_7d91;
    pub const RANDOM_NORMAL_CHANNEL: u64 = 0xb37c_4e25;
    pub const RANDOM_TANGENT_CHANNEL: u64 = 0x21ad_8f79;
}

const VISIBILITY_CHANNEL: u64 = 0xd2b7_4407;

/// Which pieces of a broken line show on one pose. A stroke is cut into
/// pieces of `range` pixels along its moved samples, and a segment between
/// two samples shows if its middle falls in a shown piece.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Breaks {
    seed: u64,
    pose: u32,
    amount: f64,
    range: f64,
}

/// How far `Breaks::step` has walked along a stroke.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Walk {
    arc: f64,
    last: Option<[f64; 2]>,
    /// The stroke grew too long to measure; 2.2.13 then shows all of it.
    lost: bool,
}

impl Breaks {
    /// `None`, when everything shows, for an `amount` outside 0 to 1 or a
    /// `range` not above 0.
    pub fn new(seed: u64, pose: u32, amount: f64, range: f64) -> Option<Self> {
        ((0.0..=1.0).contains(&amount) && range > 0.0 && range.is_finite()).then_some(Self {
            seed,
            pose,
            amount,
            range,
        })
    }

    /// Walks on to `point` and returns whether the segment to it shows;
    /// `None` at the first point.
    pub fn step(&self, walk: &mut Walk, point: [f64; 2]) -> Option<bool> {
        let last = walk.last.replace(point)?;
        let length = (point[0] - last[0]).hypot(point[1] - last[1]);
        let middle = walk.arc + length * 0.5;
        let cell = (middle / self.range).floor();
        walk.arc += length;
        if !length.is_finite()
            || !(cell >= i64::MIN as f64 && cell < i64::MAX as f64)
            || !walk.arc.is_finite()
        {
            walk.lost = true;
        }
        Some(
            walk.lost
                || noise::unit(self.seed, self.pose, cell as i64, VISIBILITY_CHANNEL)
                    >= self.amount,
        )
    }
}

impl Breaks {
    /// Which segments show: segment `i` joins `points[i]` and
    /// `points[i + 1]` of the moved samples. `None`, when everything shows,
    /// for fewer than two points or a stroke too long to measure.
    pub fn segments(&self, points: &[[f64; 2]]) -> Option<Vec<bool>> {
        let mut walk = Walk::default();
        let shown: Vec<bool> = points
            .iter()
            .filter_map(|&point| self.step(&mut walk, point))
            .collect();
        (!shown.is_empty() && !walk.lost).then_some(shown)
    }
}

// Golden values printed by 2.2.13's DeterministicNoise.cpp and
// ClassicStrokeMotion.cpp, built with MSVC /fp:precise, kept digit for digit.
#[cfg(test)]
#[expect(clippy::excessive_precision, clippy::approx_constant)]
mod tests {
    use super::*;

    #[test]
    fn signed_noise_matches_the_cpp_values() {
        let cases: [(u64, u32, i64, f64); 9] = [
            (0, 0, -3, 0.92166662265471477),
            (0, 7, 0, -0.068281764861532146),
            (0, 29, 12345, -0.40541741325522462),
            (1, 7, -3, -0.4789409280158019),
            (1, 29, 0, -0.53653289975729135),
            (0x0123456789abcdef, 0, 12345, -0.088385684832536171),
            (0x0123456789abcdef, 29, -3, 0.054609167651152202),
            (u64::MAX, 0, 0, 0.35017490274497742),
            (u64::MAX, 7, 12345, 0.75819199001770632),
        ];
        for (seed, frame, index, expected) in cases {
            assert_eq!(noise::signed(seed, frame, index, 0x1a67d3c4), expected);
        }
    }

    #[test]
    fn smooth_noise_matches_the_cpp_values() {
        let cases: [(u64, f64, f64); 6] = [
            (0, -2.75, 0.38127561858163306),
            (0, 3.25, 0.64704622181279681),
            (1, 0.5, -0.060371578595054487),
            (0x0123456789abcdef, 0.0, -0.25185275502242943),
            (u64::MAX, -2.75, 0.35966169809234461),
            (u64::MAX, 1000.125, 0.38001642416522263),
        ];
        for (seed, coordinate, expected) in cases {
            assert_eq!(noise::smooth(seed, 3, coordinate, 0xb5297a4d), expected);
        }
        assert_eq!(noise::smooth(0, 3, f64::NAN, 0xb5297a4d), 0.0);
        assert_eq!(noise::smooth(0, 3, 1e300, 0xb5297a4d), 0.0);
    }

    #[test]
    fn classic_width_matches_the_cpp_values() {
        let cases: [(f64, f64, f64); 6] = [
            (0.6, 0.0, 0.6),
            (0.6, 1.6, 0.61461285891543804),
            (6.0, 0.8, 6.0730642945771898),
            (6.0, 12.0, 6.1461285891543804),
            (120.0, 1.6, 122.92257178308761),
            (120.0, 12.0, 122.92257178308761),
        ];
        for (width, wobble, expected) in cases {
            assert_eq!(
                classic::width(width, 0x0123456789abcdef, 5, wobble),
                expected
            );
        }
    }

    #[test]
    fn classic_displacement_matches_the_cpp_values() {
        let diagonal = [-0.70710678118654757, 0.70710678118654757];
        let cases: [(f64, [f64; 2], f64, [f64; 2]); 8] = [
            (
                1.0,
                [1.0, 0.0],
                0.0,
                [100.58999358065419, -39.790447569288702],
            ),
            (
                1.0,
                [1.0, 0.0],
                260.25,
                [99.895915538218858, -40.767923057595141],
            ),
            (
                1.0,
                [0.6, 0.8],
                13.5,
                [99.356312242127558, -39.827517422098367],
            ),
            (
                1.0,
                diagonal,
                0.0,
                [99.507858898196162, -40.761317568922877],
            ),
            (1.0, diagonal, 260.25, [100.6898257349, -40.560925313176483]),
            (
                0.05,
                [1.0, 0.0],
                0.0,
                [100.58999358065419, -39.925262531123849],
            ),
            (
                0.05,
                [0.6, 0.8],
                260.25,
                [100.21116346425296, -40.913478175416152],
            ),
            (
                0.05,
                diagonal,
                13.5,
                [99.608152381146581, -41.139339641776928],
            ),
        ];
        let amplitude = classic::amplitude(6.0, 1.6);
        assert_eq!(amplitude, 1.4847999999999999);
        for (pressure, tangent, arc, expected) in cases {
            let moved = classic::displace(
                [100.25, -40.5],
                pressure,
                tangent,
                arc,
                amplitude,
                0x0123456789abcdef,
                11,
            );
            assert_eq!(
                moved, expected,
                "pressure {pressure}, tangent {tangent:?}, arc {arc}"
            );
        }
    }

    #[test]
    fn no_point_moves_beyond_the_maximum() {
        let (width, wobble) = (6.0, 12.0);
        let amplitude = classic::amplitude(width, wobble);
        let limit = classic::max_displacement(width, wobble);
        for seed in 0..64u64 {
            for frame in 0..30 {
                for step in 0..200 {
                    let arc = f64::from(step) * 1.7;
                    let moved =
                        classic::displace([0.0, 0.0], 1.0, [0.6, 0.8], arc, amplitude, seed, frame);
                    assert!(moved[0].hypot(moved[1]) <= limit);
                }
            }
        }
    }

    #[test]
    fn frames_wrap_around_the_cycle() {
        assert_eq!(frame_in_cycle(0, 30), 0);
        assert_eq!(frame_in_cycle(31, 30), 1);
        assert_eq!(frame_in_cycle(-1, 30), 29);
        assert_eq!(frame_in_cycle(5, 0), 0);
    }

    const SEED: u64 = 0x0123456789abcdef;

    /// The rows of one section of `testdata/motion.txt`, split into fields.
    fn golden(section: &str) -> Vec<Vec<&'static str>> {
        let text = include_str!("../testdata/motion.txt");
        let mut rows = Vec::new();
        let mut inside = false;
        for line in text.lines() {
            if let Some(title) = line.strip_prefix("// ") {
                inside = title.starts_with(&format!("{section}("));
            } else if inside {
                let row = line.trim_start_matches('(').trim_end_matches("),");
                rows.push(
                    row.split(", ")
                        .map(|field| field.trim_matches(['[', ']', '"']))
                        .collect(),
                );
            }
        }
        assert!(!rows.is_empty(), "no rows for {section}");
        rows
    }

    fn number<T: std::str::FromStr<Err: std::fmt::Debug>>(field: &str) -> T {
        field.parse().unwrap()
    }

    /// The motion of a row's first five fields.
    fn motion(row: &[&str]) -> Motion {
        let style = match row[0] {
            "Classic" => MotionStyle::Classic,
            "Smooth" => MotionStyle::Smooth,
            "Stepped" => MotionStyle::Stepped,
            other => panic!("style {other}"),
        };
        Motion {
            style,
            poses: number(row[1]),
            detail: row.get(2).map_or(12, |field| number(field)),
            linked: row.get(3).map_or(1.0, |field| number(field)),
            randomness: row.get(4).map_or(0.0, |field| number(field)),
            ..Motion::DEFAULT
        }
    }

    #[test]
    fn smooth_and_stepped_poses_match_the_cpp_values() {
        for row in golden("smooth_sample") {
            let pose = smooth_pose(number(row[0]), number(row[1]), number(row[2])).unwrap();
            let expected = Pose {
                first: number(row[3]),
                second: number(row[4]),
                blend: number(row[5]),
            };
            assert_eq!(pose, expected, "{row:?}");
        }
        for row in golden("stepped_pose") {
            let pose = stepped_pose(number(row[0]), number(row[1]), number(row[2]));
            assert_eq!(pose, Some(number(row[3])), "{row:?}");
        }
        assert_eq!(smooth_pose(0.0, 4, 5), None);
        assert_eq!(smooth_pose(f64::NAN, 30, 8), None);
        assert_eq!(stepped_pose(0, 30, 0), None);
    }

    #[test]
    fn posed_width_matches_the_cpp_values() {
        for row in golden("width") {
            let mover = Mover::new(motion(&row), 30);
            let width = mover.width(number(row[5]), SEED, number(row[6]), number(row[7]));
            assert_eq!(width, number::<f64>(row[8]), "{row:?}");
        }
    }

    #[test]
    fn posed_displacement_matches_the_cpp_values() {
        let amplitude = classic::amplitude(6.0, 1.6);
        for row in golden("displace") {
            let mover = Mover::new(motion(&row), 30);
            let moved = mover.displace(
                [100.25, -40.5],
                number(row[5]),
                [number(row[6]), number(row[7])],
                number(row[8]),
                number(row[9]),
                amplitude,
                SEED,
                number(row[10]),
            );
            let expected: [f64; 2] = [number(row[11]), number(row[12])];
            assert_eq!(moved, expected, "{row:?}");
        }
    }

    #[test]
    fn visibility_poses_match_the_cpp_values() {
        for row in golden("visibility_pose") {
            let mover = Mover::new(motion(&row[..2]), number(row[3]));
            let frame = frame_in_cycle(number(row[2]), number(row[3]));
            assert_eq!(
                mover.visibility_pose(frame),
                number::<u32>(row[4]),
                "{row:?}"
            );
        }
    }

    #[test]
    fn visible_segments_match_the_cpp_values() {
        let points: Vec<[f64; 2]> = (0..60)
            .map(|index| {
                let angle = f64::from(index) * 0.35;
                let radius = 10.0 + f64::from(index) * 1.7;
                [200.0 + radius * angle.cos(), 150.0 + radius * angle.sin()]
            })
            .collect();
        for row in golden("visible_segments") {
            let breaks = Breaks::new(SEED, number(row[0]), number(row[1]), number(row[2]));
            let shown = breaks.unwrap().segments(&points).unwrap();
            let expected: Vec<bool> = row[3].chars().map(|bit| bit == '1').collect();
            assert_eq!(shown, expected, "{row:?}");
        }
        let breaks = Breaks::new(SEED, 0, 0.5, 24.0).unwrap();
        assert_eq!(breaks.segments(&points[..1]), None);
        assert_eq!(breaks.segments(&[[0.0, 0.0], [f64::INFINITY, 0.0]]), None);
        assert_eq!(Breaks::new(SEED, 0, 1.5, 24.0), None);
        assert_eq!(Breaks::new(SEED, 0, 0.5, 0.0), None);
    }

    #[test]
    fn smooth_joins_the_last_pose_to_the_first_and_stepped_holds() {
        let frames = 30;
        let smooth = Mover::new(
            Motion {
                style: MotionStyle::Smooth,
                ..Motion::DEFAULT
            },
            frames,
        );
        let stepped = Mover::new(
            Motion {
                style: MotionStyle::Stepped,
                ..Motion::DEFAULT
            },
            frames,
        );
        let amplitude = classic::amplitude(6.0, 1.6);
        let at = |mover: &Mover, frame: u32| {
            mover.displace([0.0, 0.0], 1.0, [1.0, 0.0], 40.0, 7, amplitude, SEED, frame)
        };
        // Smooth moves a little each frame, also from the last into the first.
        let step = |a: [f64; 2], b: [f64; 2]| (a[0] - b[0]).hypot(a[1] - b[1]);
        let steps: Vec<f64> = (0..frames)
            .map(|frame| step(at(&smooth, frame), at(&smooth, (frame + 1) % frames)))
            .collect();
        let largest = steps.iter().copied().fold(0.0, f64::max);
        assert!(steps[29] < largest, "{steps:?}");
        assert!(steps[29] < amplitude * 0.5);
        // Stepped holds each of its 8 poses, then jumps.
        let held = (0..frames)
            .filter(|&frame| at(&stepped, frame) == at(&stepped, (frame + 1) % frames))
            .count();
        assert_eq!(held, 30 - 8);
    }

    #[test]
    fn a_displacer_moves_samples_as_one_at_a_time() {
        let amplitude = classic::amplitude(6.0, 1.6);
        // Small steps within a cell, a step to the next, jumps and a step back.
        let mut arc = 0.0;
        let arcs: Vec<f64> = (0..200)
            .map(|index| {
                arc += match index % 23 {
                    7 => 95.0,
                    13 => -40.0,
                    _ => 1.7,
                };
                arc
            })
            .collect();
        let motions = [
            Motion::DEFAULT,
            Motion {
                style: MotionStyle::Smooth,
                linked: 0.375,
                randomness: 0.5,
                detail: 24,
                ..Motion::DEFAULT
            },
            Motion {
                style: MotionStyle::Stepped,
                linked: 0.0,
                ..Motion::DEFAULT
            },
        ];
        for motion in motions {
            let mover = Mover::new(motion, 30);
            for frame in [0, 3, 29] {
                let mut displacer = mover.displacer(SEED, frame, amplitude);
                for (index, &arc) in arcs.iter().enumerate() {
                    let position = [10.0, -4.0];
                    let tangent = [0.6, 0.8];
                    let one =
                        mover.displace(position, 0.7, tangent, arc, index, amplitude, SEED, frame);
                    let walked = displacer.displace(position, 0.7, tangent, arc, index);
                    assert_eq!(walked, one, "{motion:?} frame {frame} arc {arc}");
                }
            }
        }
    }

    #[test]
    fn poses_are_at_most_the_frames() {
        let motion = Motion {
            style: MotionStyle::Stepped,
            poses: 60,
            ..Motion::DEFAULT
        };
        let mover = Mover::new(motion, 12);
        assert_eq!(mover.motion().poses, 12);
        let poses: Vec<u32> = (0..12).map(|frame| mover.pose(frame).first).collect();
        assert_eq!(poses, (0..12).collect::<Vec<_>>());
    }

    #[test]
    fn no_posed_point_moves_beyond_the_maximum() {
        let (width, wobble) = (6.0, 12.0);
        let amplitude = classic::amplitude(width, wobble);
        let limit = classic::max_displacement(width, wobble);
        for style in [MotionStyle::Smooth, MotionStyle::Stepped] {
            for (linked, randomness) in [(0.0, 0.0), (0.5, 1.0), (1.0, 0.5)] {
                let mover = Mover::new(
                    Motion {
                        style,
                        detail: 24,
                        linked,
                        randomness,
                        ..Motion::DEFAULT
                    },
                    30,
                );
                for seed in 0..16u64 {
                    for frame in 0..30 {
                        for step in 0..100 {
                            let arc = f64::from(step) * 1.7;
                            let index = step as usize;
                            let moved = mover.displace(
                                [0.0, 0.0],
                                1.0,
                                [0.6, 0.8],
                                arc,
                                index,
                                amplitude,
                                seed,
                                frame,
                            );
                            assert!(moved[0].hypot(moved[1]) <= limit);
                        }
                    }
                }
            }
        }
    }
}

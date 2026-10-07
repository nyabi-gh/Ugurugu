// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! Where each stroke point sits on each frame (render revision 1).
//!
//! Every value is a pure function of the stroke's seed, the frame and the
//! position along the stroke, so a preview, an export, a redraw after undo
//! and a reopened file all move the same way. The Classic formulas and
//! constants are those of 2.2.13, checked against values computed by its C++
//! code; pixels are not promised to match, only the motion.

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
        let left = coordinate.floor();
        // Also false for NaN and infinities.
        if !(left >= i64::MIN as f64 && left < i64::MAX as f64) {
            return 0.0;
        }
        let fraction = coordinate - left;
        let blend = fraction * fraction * (3.0 - 2.0 * fraction);
        let a = signed(seed, frame, left as i64, channel);
        let b = signed(seed, frame, (left as i64).wrapping_add(1), channel);
        a + (b - a) * blend
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
    use super::noise;

    const SWAY_WAVELENGTH: f64 = 26.0;
    const DETAIL_WAVELENGTH: f64 = 9.0;
    const WIDTH_CHANNEL: u64 = 0x1a67_d3c4;
    const SWAY_CHANNEL: u64 = 0xb529_7a4d;
    const DETAIL_CHANNEL: u64 = 0x1b56_c4e9;
    const TANGENT_CHANNEL: u64 = 0x68e3_1da4;

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
        let normal = [-tangent[1], tangent[0]];
        let sway = noise::smooth(seed, frame, arc / SWAY_WAVELENGTH, SWAY_CHANNEL);
        let detail = noise::smooth(seed, frame, arc / DETAIL_WAVELENGTH + 31.0, DETAIL_CHANNEL);
        let across = sway * 0.72 + detail * 0.28;
        let along = noise::smooth(seed, frame, arc / SWAY_WAVELENGTH + 57.0, TANGENT_CHANNEL);
        let pressure_factor = 0.8 + pressure * 0.2;
        std::array::from_fn(|axis| {
            position[axis]
                + (normal[axis] * across * amplitude * pressure_factor
                    + tangent[axis] * along * amplitude * 0.3)
        })
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
}

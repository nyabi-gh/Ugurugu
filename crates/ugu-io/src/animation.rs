// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! What animated exports share.

/// Frame delays in `per_second` units adding up to whole frames at `fps`,
/// none shorter than one unit, as 2.2.13's `frameDurations`.
pub fn delays(frames: u32, fps: f64, per_second: u32) -> Vec<u32> {
    let mut emitted: i64 = 0;
    (1..=frames)
        .map(|frame| {
            // qRound64: halves away from zero.
            let target = (f64::from(frame) * f64::from(per_second) / fps).round() as i64;
            let delay = (target - emitted).max(1);
            emitted += delay;
            delay as u32
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delays_add_up_to_whole_frames() {
        assert_eq!(delays(4, 25.0, 100), [4, 4, 4, 4]);
        // 1/3 s each: 33, 34, 33 adds up to a second.
        assert_eq!(delays(3, 3.0, 100), [33, 34, 33]);
        assert_eq!(delays(3, 3.0, 100).iter().sum::<u32>(), 100);
        assert_eq!(delays(2, 1000.0, 100), [1, 1]);
        assert_eq!(delays(3, 24.0, 1000), [42, 41, 42]);
    }
}

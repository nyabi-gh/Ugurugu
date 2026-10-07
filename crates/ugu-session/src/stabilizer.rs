// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! Hand-shake smoothing: the 1 Euro filter (Casiez, Roussel and Vogel, CHI
//! 2012), a low-pass whose cutoff rises with speed, so slow movement is
//! smoothed heavily and fast movement keeps little lag. One strength from 0
//! to 1 moves its minimum cutoff and speed coefficient between the weakest
//! and strongest ends together. The constants are 2.2.13's.

const DEFAULT_INTERVAL: f64 = 1.0 / 120.0;
const MIN_INTERVAL: f64 = 1.0 / 1000.0;
const MAX_INTERVAL: f64 = 0.1;
const DERIVATIVE_CUTOFF: f64 = 1.0;
const WEAKEST_MIN_CUTOFF: f64 = 18.0;
const STRONGEST_MIN_CUTOFF: f64 = 0.75;
const WEAKEST_SPEED_COEFFICIENT: f64 = 0.08;
const STRONGEST_SPEED_COEFFICIENT: f64 = 0.008;

fn low_pass_alpha(cutoff: f64, interval: f64) -> f64 {
    let time_constant = 1.0 / (2.0 * std::f64::consts::PI * cutoff);
    1.0 / (1.0 + time_constant / interval)
}

fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

#[derive(Clone, Debug)]
pub struct Stabilizer {
    strength: f64,
    raw: [f64; 2],
    filtered: [f64; 2],
    velocity: [f64; 2],
    /// Milliseconds.
    time: f64,
}

impl Stabilizer {
    /// Starts a stroke at `position`.
    pub fn new(strength: f32, position: [f64; 2], time: f64) -> Self {
        Self {
            strength: f64::from(strength).clamp(0.0, 1.0),
            raw: position,
            filtered: position,
            velocity: [0.0; 2],
            time,
        }
    }

    /// The smoothed position for the next raw one, at `time` milliseconds.
    pub fn update(&mut self, position: [f64; 2], time: f64) -> [f64; 2] {
        let interval = if time > self.time {
            ((time - self.time) / 1000.0).clamp(MIN_INTERVAL, MAX_INTERVAL)
        } else {
            DEFAULT_INTERVAL
        };
        let velocity_alpha = low_pass_alpha(DERIVATIVE_CUTOFF, interval);
        self.velocity = std::array::from_fn(|axis| {
            let raw_velocity = (position[axis] - self.raw[axis]) / interval;
            self.velocity[axis] + (raw_velocity - self.velocity[axis]) * velocity_alpha
        });
        self.raw = position;
        self.time = time;
        if self.strength == 0.0 {
            self.filtered = position;
            return position;
        }
        let min_cutoff = lerp(WEAKEST_MIN_CUTOFF, STRONGEST_MIN_CUTOFF, self.strength);
        let coefficient = lerp(
            WEAKEST_SPEED_COEFFICIENT,
            STRONGEST_SPEED_COEFFICIENT,
            self.strength,
        );
        let speed = self.velocity[0].hypot(self.velocity[1]);
        let adaptive = low_pass_alpha(min_cutoff + coefficient * speed, interval);
        let alpha = lerp(1.0, adaptive, self.strength);
        self.filtered = std::array::from_fn(|axis| {
            self.filtered[axis] + (position[axis] - self.filtered[axis]) * alpha
        });
        self.filtered
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strength_zero_passes_input_through() {
        let mut stabilizer = Stabilizer::new(0.0, [0.0, 0.0], 0.0);
        for step in 1..20 {
            let position = [f64::from(step) * 3.0, (f64::from(step) * 0.7).sin() * 10.0];
            assert_eq!(stabilizer.update(position, f64::from(step) * 8.0), position);
        }
    }

    #[test]
    fn stronger_smoothing_removes_more_jitter() {
        let jitter = |strength: f32| {
            let mut stabilizer = Stabilizer::new(strength, [0.0, 0.0], 0.0);
            let mut total = 0.0;
            for step in 1..200 {
                let wobble = if step % 2 == 0 { 2.0 } else { -2.0 };
                let out = stabilizer.update([f64::from(step), wobble], f64::from(step) * 8.0);
                total += out[1].abs();
            }
            total
        };
        assert!(jitter(1.0) < jitter(0.5));
        assert!(jitter(0.5) < jitter(0.0));
    }

    #[test]
    fn a_stalled_clock_uses_the_default_interval() {
        let mut stabilizer = Stabilizer::new(0.5, [0.0, 0.0], 100.0);
        let out = stabilizer.update([10.0, 0.0], 100.0);
        assert!(out[0] > 0.0 && out[0] < 10.0);
    }
}

// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! The symmetry ruler: a stroke drawn by hand is drawn again turned about a
//! centre, and mirrored across lines through it, as Clip Studio Paint's
//! symmetry ruler draws.

use ugu_core::ops::Affine;

/// Where and how strokes are repeated, in document pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Symmetry {
    pub on: bool,
    pub centre: [f64; 2],
    /// The first line's turn from upright, clockwise on screen, in degrees.
    pub angle: f64,
    /// How many lines meet at the centre, or with `mirror` off, how many
    /// times a stroke is repeated around it.
    pub lines: u32,
    /// Whether each stroke is mirrored across the lines.
    pub mirror: bool,
}

impl Symmetry {
    pub const LINES: std::ops::RangeInclusive<u32> = 1..=12;

    /// Off, one upright line through the middle of a `canvas`: a mirror
    /// from left to right once turned on.
    pub fn new(canvas: [u32; 2]) -> Self {
        Self {
            on: false,
            centre: canvas.map(|edge| f64::from(edge) / 2.0),
            angle: 0.0,
            lines: 1,
            mirror: true,
        }
    }

    /// The moves taking a stroke to each of its copies, the stroke itself
    /// left out: turns by whole sectors, and with `mirror` each of them
    /// after mirroring across the first line. Empty when off.
    pub fn copies(&self) -> Vec<Affine> {
        if !self.on {
            return Vec::new();
        }
        let lines = self.lines.clamp(*Self::LINES.start(), *Self::LINES.end());
        let mirror = self.mirror.then(|| self.mirror_across_first());
        let mut copies = Vec::new();
        for sector in 0..lines {
            let turn = self.turn(f64::from(sector) * 360.0 / f64::from(lines));
            if sector != 0 {
                copies.push(turn);
            }
            if let Some(mirror) = mirror {
                copies.push(mirror.then(turn));
            }
        }
        copies
    }

    /// The directions the ruler's lines leave the centre in, in degrees
    /// clockwise from up: both ways along each mirror line, the lines
    /// meeting at equal angles, or one way for each repeat when nothing is
    /// mirrored.
    pub fn rays(&self) -> Vec<f64> {
        let lines = self.lines.clamp(*Self::LINES.start(), *Self::LINES.end());
        if !self.mirror {
            let step = 360.0 / f64::from(lines);
            return (0..lines)
                .map(|line| self.angle + f64::from(line) * step)
                .collect();
        }
        let step = 180.0 / f64::from(lines);
        (0..lines)
            .flat_map(|line| {
                let angle = self.angle + f64::from(line) * step;
                [angle, angle + 180.0]
            })
            .collect()
    }

    /// A turn by `degrees` about the centre, exact for quarter turns so that
    /// mirrored and turned copies land on the same pixels as the stroke.
    fn turn(&self, degrees: f64) -> Affine {
        let (sin, cos) = sin_cos(degrees);
        let [x, y] = self.centre;
        Affine([
            cos,
            -sin,
            x - cos * x + sin * y,
            sin,
            cos,
            y - sin * x - cos * y,
        ])
    }

    /// The mirror across the line through the centre at `angle`.
    fn mirror_across_first(&self) -> Affine {
        // The line runs along (sin, -cos): up, turned by `angle`.
        let (sin, cos) = sin_cos(self.angle);
        let [ux, uy] = [sin, -cos];
        let [a, b, d, e] = [
            2.0 * ux * ux - 1.0,
            2.0 * ux * uy,
            2.0 * ux * uy,
            2.0 * uy * uy - 1.0,
        ];
        let [x, y] = self.centre;
        Affine([a, b, x - a * x - b * y, d, e, y - d * x - e * y])
    }
}

/// `degrees.to_radians().sin_cos()`, exact at multiples of 90°.
fn sin_cos(degrees: f64) -> (f64, f64) {
    let quarters = degrees / 90.0;
    if quarters == quarters.round() {
        match quarters.rem_euclid(4.0) as u8 {
            0 => (0.0, 1.0),
            1 => (1.0, 0.0),
            2 => (0.0, -1.0),
            _ => (-1.0, 0.0),
        }
    } else {
        degrees.to_radians().sin_cos()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ruler(lines: u32, mirror: bool) -> Symmetry {
        Symmetry {
            on: true,
            lines,
            mirror,
            ..Symmetry::new([200, 100])
        }
    }

    #[test]
    fn off_or_a_single_unmirrored_line_has_no_copies() {
        assert!(Symmetry::new([200, 100]).copies().is_empty());
        assert!(ruler(1, false).copies().is_empty());
    }

    #[test]
    fn one_line_mirrors_left_to_right_exactly() {
        let copies = ruler(1, true).copies();
        assert_eq!(copies.len(), 1);
        assert_eq!(copies[0].apply([10.25, 30.5]), [189.75, 30.5]);
    }

    #[test]
    fn a_turned_line_mirrors_across_itself() {
        // A level line through the middle mirrors top to bottom.
        let level = Symmetry {
            angle: 90.0,
            ..ruler(1, true)
        };
        assert_eq!(level.copies()[0].apply([10.0, 20.0]), [10.0, 80.0]);
        // A diagonal swaps the axes about the centre.
        let diagonal = Symmetry {
            angle: 45.0,
            centre: [0.0, 0.0],
            ..ruler(1, true)
        };
        let [x, y] = diagonal.copies()[0].apply([3.0, 0.0]);
        assert!(
            (x - 0.0).abs() < 1e-12 && (y + 3.0).abs() < 1e-12,
            "{x} {y}"
        );
    }

    #[test]
    fn lines_repeat_around_the_centre_and_mirror_each_repeat() {
        // Four turns of a point to the right of the centre land on the four
        // sides, exactly.
        let turns = ruler(4, false).copies();
        let spots: Vec<_> = turns.iter().map(|copy| copy.apply([110.0, 50.0])).collect();
        assert_eq!(spots, [[100.0, 60.0], [90.0, 50.0], [100.0, 40.0]]);
        // Mirrored, every turn comes twice: eight strokes in all.
        let mirrored = ruler(4, true).copies();
        assert_eq!(mirrored.len(), 7);
        let mut spots: Vec<_> = mirrored
            .iter()
            .map(|copy| copy.apply([110.0, 45.0]))
            .collect();
        spots.push([110.0, 45.0]);
        spots.sort_by(|a, b| a.partial_cmp(b).unwrap());
        spots.dedup();
        assert_eq!(spots.len(), 8, "{spots:?}");
    }

    #[test]
    fn rays_run_both_ways_along_mirror_lines() {
        assert_eq!(ruler(1, true).rays(), [0.0, 180.0]);
        assert_eq!(ruler(3, false).rays(), [0.0, 120.0, 240.0]);
        assert_eq!(ruler(2, true).rays(), [0.0, 180.0, 90.0, 270.0]);
    }
}

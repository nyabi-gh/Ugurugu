// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! The symmetry ruler of the document shown, and the lines that show it.

use ugu_session::Symmetry;

use super::Canvas;

impl Canvas {
    pub fn symmetry(&self) -> Symmetry {
        self.tab.session.symmetry()
    }

    /// Applies from the next stroke.
    pub fn set_symmetry(&mut self, symmetry: Symmetry) {
        self.tab.session.set_symmetry(symmetry);
    }

    /// The ruler's lines from its centre to where they leave the canvas, in
    /// document pixels; none when it is off or its centre is off the canvas.
    pub fn symmetry_lines(&self) -> Vec<[[f64; 2]; 2]> {
        let symmetry = self.symmetry();
        if !symmetry.on {
            return Vec::new();
        }
        let canvas = self.tab.session.document().canvas.map(f64::from);
        symmetry
            .rays()
            .into_iter()
            .filter_map(|degrees| {
                let end = to_edge(symmetry.centre, degrees, canvas)?;
                Some([symmetry.centre, end])
            })
            .collect()
    }
}

/// Where a ray from `from`, `degrees` clockwise from up, leaves a canvas of
/// `size`; `None` when `from` is outside it.
fn to_edge(from: [f64; 2], degrees: f64, size: [f64; 2]) -> Option<[f64; 2]> {
    if !(0.0..=size[0]).contains(&from[0]) || !(0.0..=size[1]).contains(&from[1]) {
        return None;
    }
    let (sin, cos) = degrees.to_radians().sin_cos();
    let direction = [sin, -cos];
    let reach = (0..2)
        .filter(|&axis| direction[axis].abs() > 1e-12)
        .map(|axis| {
            let edge = if direction[axis] > 0.0 {
                size[axis]
            } else {
                0.0
            };
            (edge - from[axis]) / direction[axis]
        })
        .fold(f64::INFINITY, f64::min);
    Some([
        from[0] + direction[0] * reach,
        from[1] + direction[1] * reach,
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn near(a: [f64; 2], b: [f64; 2]) -> bool {
        (a[0] - b[0]).abs() < 1e-9 && (a[1] - b[1]).abs() < 1e-9
    }

    #[test]
    fn rays_end_on_the_canvas_edge() {
        let size = [200.0, 100.0];
        assert!(near(
            to_edge([100.0, 50.0], 0.0, size).unwrap(),
            [100.0, 0.0]
        ));
        assert!(near(
            to_edge([100.0, 50.0], 180.0, size).unwrap(),
            [100.0, 100.0]
        ));
        assert!(near(
            to_edge([100.0, 50.0], 90.0, size).unwrap(),
            [200.0, 50.0]
        ));
        // Up and to the right, the top edge comes first.
        assert!(near(
            to_edge([100.0, 50.0], 45.0, size).unwrap(),
            [150.0, 0.0]
        ));
        assert_eq!(to_edge([-1.0, 50.0], 0.0, size), None);
    }
}

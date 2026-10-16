// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! The symmetry ruler of the document shown, the lines that show it, and
//! the ruler tool that places it: a drag lays a new first line from where
//! it starts, and a drag from the centre moves the ruler.

use ugu_session::{Symmetry, Tool};

use super::{Canvas, Interaction};

/// Logical pixels from the centre that still grab it.
const CENTRE_REACH: f64 = 9.0;
/// Logical pixels a new line is dragged before it takes the drag's
/// direction; a click only moves the centre.
const LINE_REACH: f64 = 4.0;
/// Lines snap to this many degrees with Shift.
const SNAP: f64 = 15.0;

/// What a drag with the ruler tool changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RulerGrip {
    /// Moves the ruler, keeping its lines' angle.
    Centre,
    /// Lays a new first line from where the drag starts.
    Line,
}

impl Canvas {
    pub fn symmetry(&self) -> Symmetry {
        self.tab.session.symmetry()
    }

    /// Applies from the next stroke.
    pub fn set_symmetry(&mut self, symmetry: Symmetry) {
        self.tab.session.set_symmetry(symmetry);
    }

    /// What a drag from `position` (client physical pixels) with the ruler
    /// tool would change; `None` with another tool.
    pub fn ruler_grip_at(&self, position: [f64; 2]) -> Option<RulerGrip> {
        if self.tools.tool != Tool::Symmetry || self.sizing.is_some() {
            return None;
        }
        let symmetry = self.symmetry();
        let [x, y] = self.to_client(symmetry.centre);
        let reach = CENTRE_REACH * f64::from(self.pixels_per_point);
        let near = (x - position[0]).hypot(y - position[1]) <= reach;
        Some(if symmetry.on && near {
            RulerGrip::Centre
        } else {
            RulerGrip::Line
        })
    }

    pub(super) fn begin_ruler_drag(&mut self, position: [f64; 2]) {
        let Some(grip) = self.ruler_grip_at(position) else {
            return;
        };
        self.notice = None;
        self.interaction = Interaction::Ruling {
            grip,
            start: position,
            base: self.symmetry(),
        };
        self.drag_ruler(position);
    }

    pub(super) fn drag_ruler(&mut self, position: [f64; 2]) {
        let Interaction::Ruling { grip, start, base } = self.interaction else {
            return;
        };
        let canvas = self.tab.session.document().canvas.map(f64::from);
        let [from, to] = [start, position].map(|point| self.to_document(point));
        let mut symmetry = Symmetry { on: true, ..base };
        match grip {
            RulerGrip::Centre => {
                symmetry.centre = placed(
                    [
                        base.centre[0] + to[0] - from[0],
                        base.centre[1] + to[1] - from[1],
                    ],
                    canvas,
                );
            }
            RulerGrip::Line => {
                symmetry.centre = placed(from, canvas);
                let dragged = (position[0] - start[0]).hypot(position[1] - start[1]);
                if dragged >= LINE_REACH * f64::from(self.pixels_per_point) {
                    let mut angle = (to[0] - from[0]).atan2(from[1] - to[1]).to_degrees();
                    if self.modifiers.0 {
                        angle = (angle / SNAP).round() * SNAP;
                    }
                    symmetry.angle = angle;
                }
            }
        }
        self.set_symmetry(symmetry);
    }

    pub(super) fn end_ruler_drag(&mut self, position: [f64; 2]) {
        self.drag_ruler(position);
        self.interaction = Interaction::Idle;
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

/// `point` on the canvas of `size`, on a whole or half pixel, so that strokes
/// mirrored across an upright or level line land on whole pixels.
fn placed(point: [f64; 2], size: [f64; 2]) -> [f64; 2] {
    [0, 1].map(|axis| ((point[axis] * 2.0).round() / 2.0).clamp(0.0, size[axis]))
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

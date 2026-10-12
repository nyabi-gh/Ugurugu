// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! Turning and mirroring the view, and zooming by dragging, as 2.2.13 does
//! (`ui/CanvasWidget.cpp`, `ui/CanvasWidgetTools.cpp`): 5° steps from the
//! menu, the keys and Shift+wheel, half a degree per point dragged
//! sideways with Shift+Space, and doubling the zoom every 120 points dragged
//! with Ctrl+Space. Steps and mirroring keep the document's middle where it
//! is on screen; a drag keeps the point where it started. Only the view
//! changes, never the document.

use ugu_render::view::Placement;

use super::{Canvas, Interaction, ZOOM_RANGE};

/// Degrees a menu item, a key or a wheel notch turns the view.
pub const TURN_STEP: f64 = 5.0;
/// Degrees per logical pixel dragged sideways.
const DRAG_DEGREES: f64 = 0.5;
/// Logical pixels dragged sideways that double the zoom.
const ZOOM_DOUBLING: f64 = 120.0;

/// `degrees` in (-180, 180], as 2.2.13's `normalizedRotation`.
pub fn normalized(degrees: f64) -> f64 {
    let turned = (degrees + 180.0).rem_euclid(360.0) - 180.0;
    if turned == -180.0 {
        180.0
    } else if turned.abs() < 1e-9 {
        0.0
    } else {
        turned
    }
}

impl Canvas {
    fn origin(&self) -> [f64; 2] {
        self.area
            .map_or([0.0, 0.0], |area| [f64::from(area[0]), f64::from(area[1])])
    }

    /// The cosine and sine of the view's angle, exactly 1 and 0 upright.
    pub(super) fn turn(&self) -> [f64; 2] {
        if self.tab.angle == 0.0 {
            return [1.0, 0.0];
        }
        let radians = self.tab.angle.to_radians();
        [radians.cos(), radians.sin()]
    }

    /// Physical pixels on screen per document pixel along x and along y.
    fn axes(&self) -> [[f64; 2]; 2] {
        let [cos, sin] = self.turn();
        let flip = if self.tab.mirrored { -1.0 } else { 1.0 };
        [
            [self.tab.scale * flip * cos, self.tab.scale * flip * sin],
            [-self.tab.scale * sin, self.tab.scale * cos],
        ]
    }

    /// Document pixels to client physical pixels.
    pub(super) fn to_client(&self, [x, y]: [f64; 2]) -> [f64; 2] {
        let origin = self.origin();
        let [along_x, along_y] = self.axes();
        std::array::from_fn(|axis| {
            origin[axis] + self.tab.offset[axis] + (x * along_x[axis] + y * along_y[axis])
        })
    }

    /// Client physical pixels to document pixels.
    pub(super) fn to_document(&self, position: [f64; 2]) -> [f64; 2] {
        let origin = self.origin();
        let [vx, vy] = std::array::from_fn(|axis| {
            (position[axis] - origin[axis] - self.tab.offset[axis]) / self.tab.scale
        });
        let [cos, sin] = self.turn();
        let flip = if self.tab.mirrored { -1.0 } else { 1.0 };
        [flip * (cos * vx + sin * vy), cos * vy - sin * vx]
    }

    /// Places the view so that document point `point` is at `client`.
    fn pin(&mut self, point: [f64; 2], client: [f64; 2]) {
        let at = self.to_client(point);
        for axis in 0..2 {
            self.tab.offset[axis] += client[axis] - at[axis];
        }
    }

    pub(super) fn middle(&self) -> [f64; 2] {
        self.tab
            .session
            .document()
            .canvas
            .map(|edge| f64::from(edge) / 2.0)
    }

    /// Clockwise on screen, in degrees, in (-180, 180].
    pub fn angle(&self) -> f64 {
        self.tab.angle
    }

    pub fn is_mirrored(&self) -> bool {
        self.tab.mirrored
    }

    /// Turns the view to `degrees`, keeping the document's middle in place.
    pub fn turn_to(&mut self, degrees: f64) {
        if !degrees.is_finite() {
            return;
        }
        let middle = self.middle();
        let kept = self.to_client(middle);
        self.tab.angle = normalized(degrees);
        self.pin(middle, kept);
    }

    /// Turns the view by `degrees`, keeping the document's middle in place.
    pub fn turn_by(&mut self, degrees: f64) {
        self.turn_to(self.tab.angle + degrees);
    }

    /// Flips the view left to right, keeping the document's middle in
    /// place.
    pub fn set_mirrored(&mut self, mirrored: bool) {
        let middle = self.middle();
        let kept = self.to_client(middle);
        self.tab.mirrored = mirrored;
        self.pin(middle, kept);
    }

    pub(super) fn begin_turn_drag(&mut self, position: [f64; 2]) {
        self.interaction = Interaction::Turning {
            start: position[0],
            angle: self.tab.angle,
        };
    }

    pub(super) fn begin_zoom_drag(&mut self, position: [f64; 2]) {
        self.interaction = Interaction::Zooming {
            start: position,
            scale: self.tab.scale,
            anchor: self.to_document(position),
        };
    }

    /// Follows a turning or zooming drag to `position`.
    pub(super) fn drag_view(&mut self, position: [f64; 2]) {
        let logical = f64::from(self.pixels_per_point);
        match self.interaction {
            Interaction::Turning { start, angle } => {
                self.turn_to(angle + (position[0] - start) / logical * DRAG_DEGREES);
            }
            Interaction::Zooming {
                start,
                scale,
                anchor,
            } => {
                let doubled = (position[0] - start[0]) / logical / ZOOM_DOUBLING;
                self.tab.scale =
                    (scale * doubled.exp2()).clamp(*ZOOM_RANGE.start(), *ZOOM_RANGE.end());
                self.pin(anchor, start);
            }
            _ => {}
        }
    }

    /// Follows two fingers as 2.2.13 does (`continueTouchGesture`): the
    /// zoom follows how far apart they are, the angle how they turn, and
    /// the document point under their middle follows the middle. `fresh`
    /// starts from where they are.
    pub(super) fn pinch(&mut self, [a, b]: [[f64; 2]; 2], fresh: bool) {
        let apart = (b[0] - a[0]).hypot(b[1] - a[1]);
        if !apart.is_finite() || apart <= 0.01 {
            if matches!(self.interaction, Interaction::Pinching { .. }) {
                self.interaction = Interaction::Idle;
            }
            return;
        }
        let middle = [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0];
        let angle = (b[1] - a[1]).atan2(b[0] - a[0]).to_degrees();
        if let Interaction::Pinching {
            middle: last_middle,
            apart: last_apart,
            angle: last_angle,
        } = self.interaction
            && !fresh
        {
            let anchor = self.to_document(last_middle);
            let turned = angle - last_angle;
            self.tab.scale =
                (self.tab.scale * apart / last_apart).clamp(*ZOOM_RANGE.start(), *ZOOM_RANGE.end());
            self.tab.angle = normalized(self.tab.angle + turned - 360.0 * (turned / 360.0).round());
            self.pin(anchor, middle);
        } else if !matches!(
            self.interaction,
            Interaction::Idle | Interaction::Pinching { .. }
        ) {
            return;
        }
        self.interaction = Interaction::Pinching {
            middle,
            apart,
            angle,
        };
    }

    /// The document pixels in view, as the bounds of the canvas area's
    /// corners: left, top, right, bottom.
    pub(super) fn in_view(&self) -> Option<[f64; 4]> {
        let area = self.area?;
        let corners = [[0, 1], [2, 1], [2, 3], [0, 3]]
            .map(|[x, y]| self.to_document([f64::from(area[x]), f64::from(area[y])]));
        let low = |axis: usize| {
            corners
                .iter()
                .map(|corner| corner[axis])
                .fold(f64::INFINITY, f64::min)
        };
        let high = |axis: usize| {
            corners
                .iter()
                .map(|corner| corner[axis])
                .fold(f64::NEG_INFINITY, f64::max)
        };
        Some([low(0), low(1), high(0), high(1)])
    }

    /// Where the document is drawn, in client physical pixels.
    pub fn placement(&self) -> Placement {
        let origin = self.origin();
        Placement {
            offset: [
                (origin[0] + self.tab.offset[0]) as f32,
                (origin[1] + self.tab.offset[1]) as f32,
            ],
            scale: self.tab.scale as f32,
            angle: self.tab.angle as f32,
            mirrored: self.tab.mirrored,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::{CanvasInput, Gesture};
    use ugu_core::document::Document;
    use ugu_win::clock::Ticks;
    use ugu_win::pointer::{Buttons, PointerKind, PointerSample};

    fn canvas() -> Canvas {
        let mut canvas = Canvas::new(Document::new([320, 200]), |_| {});
        canvas.set_area([10, 20, 410, 320]);
        canvas
    }

    fn close(a: [f64; 2], b: [f64; 2]) -> bool {
        (a[0] - b[0]).abs() < 1e-6 && (a[1] - b[1]).abs() < 1e-6
    }

    fn sample(position: [f64; 2]) -> PointerSample {
        PointerSample {
            position,
            pressure: None,
            tilt: None,
            twist: None,
            buttons: Buttons::PRIMARY,
            in_contact: true,
            inverted: false,
            time: Ticks(0),
            frame_id: 0,
        }
    }

    #[test]
    fn turned_and_mirrored_views_map_both_ways_as_the_display_does() {
        let mut canvas = canvas();
        for (degrees, mirrored) in [(0.0, false), (35.0, false), (-90.0, true), (180.0, true)] {
            canvas.turn_to(degrees);
            canvas.set_mirrored(mirrored);
            for point in [[0.0, 0.0], [12.5, 190.0], [320.0, 7.0]] {
                let client = canvas.to_client(point);
                assert!(
                    close(canvas.to_document(client), point),
                    "{degrees} {mirrored}"
                );
                // The display's mapping, in f32, agrees to well within a pixel.
                let shown = canvas.placement().to_target(point);
                assert!((shown[0] - client[0]).abs() < 1e-3 && (shown[1] - client[1]).abs() < 1e-3);
            }
        }
    }

    #[test]
    fn steps_and_mirroring_keep_the_document_middle_in_place() {
        let mut canvas = canvas();
        let middle = canvas.to_client([160.0, 100.0]);
        canvas.turn_by(TURN_STEP);
        assert_eq!(canvas.angle(), 5.0);
        assert!(close(canvas.to_client([160.0, 100.0]), middle));
        canvas.set_mirrored(true);
        assert!(close(canvas.to_client([160.0, 100.0]), middle));
        // The document's top-left corner, right of the middle once mirrored.
        assert!(canvas.to_client([0.0, 0.0])[0] > middle[0]);
        canvas.turn_by(-2.0 * TURN_STEP);
        canvas.turn_to(0.0);
        assert_eq!(canvas.angle(), 0.0);
        assert!(close(canvas.to_client([160.0, 100.0]), middle));
    }

    #[test]
    fn a_quarter_turn_shows_the_document_s_x_axis_going_down() {
        let mut canvas = canvas();
        canvas.turn_to(90.0);
        let [ox, oy] = canvas.to_client([0.0, 0.0]);
        let [x, y] = canvas.to_client([10.0, 0.0]);
        assert!((x - ox).abs() < 1e-9 && y > oy);
    }

    #[test]
    fn zooming_and_dragging_keep_the_point_under_the_pointer() {
        let mut canvas = canvas();
        canvas.turn_to(30.0);
        canvas.set_mirrored(true);
        let pointer = [123.0, 234.0];
        let under = canvas.to_document(pointer);
        canvas.apply(CanvasInput::Zoom {
            position: pointer,
            notches: 2.0,
        });
        // The offset is rounded to whole pixels.
        let after = canvas.to_client(under);
        assert!((after[0] - pointer[0]).abs() <= 0.5 && (after[1] - pointer[1]).abs() <= 0.5);

        let before = canvas.tab.scale;
        canvas.apply(CanvasInput::Begin(
            Gesture::Zoom,
            PointerKind::Mouse,
            sample(pointer),
        ));
        let under = canvas.to_document(pointer);
        canvas.apply(CanvasInput::Extend(sample([
            pointer[0] + 120.0,
            pointer[1],
        ])));
        assert!((canvas.tab.scale - before * 2.0).abs() < 1e-9);
        assert!(close(canvas.to_client(under), pointer));
        canvas.apply(CanvasInput::End(sample([pointer[0] + 120.0, pointer[1]])));

        let angle = canvas.angle();
        canvas.apply(CanvasInput::Begin(
            Gesture::Turn,
            PointerKind::Mouse,
            sample(pointer),
        ));
        canvas.apply(CanvasInput::Extend(sample([
            pointer[0] + 20.0,
            pointer[1] + 50.0,
        ])));
        assert!((canvas.angle() - (angle + 10.0)).abs() < 1e-9);
        canvas.apply(CanvasInput::Cancel);
        canvas.apply(CanvasInput::Turn { notches: -1.0 });
        assert!((canvas.angle() - (angle + 5.0)).abs() < 1e-9);
    }

    #[test]
    fn two_fingers_zoom_turn_and_carry_the_point_between_them() {
        let mut canvas = canvas();
        let scale = canvas.tab.scale;
        let pinch = |canvas: &mut Canvas, fingers, fresh| {
            canvas.apply(CanvasInput::Pinch { fingers, fresh });
        };
        pinch(&mut canvas, [[100.0, 150.0], [200.0, 150.0]], true);
        let under = canvas.to_document([150.0, 150.0]);
        // Twice as far apart, turned a quarter clockwise, moved by (40, 30).
        pinch(&mut canvas, [[190.0, 80.0], [190.0, 280.0]], false);
        assert!((canvas.tab.scale - scale * 2.0).abs() < 1e-9);
        assert!((canvas.angle() - 90.0).abs() < 1e-9);
        assert!(close(canvas.to_client(under), [190.0, 180.0]));
        // Across the half turn the angle takes the short way round.
        pinch(&mut canvas, [[190.0, 80.0], [190.0, 280.0]], true);
        pinch(&mut canvas, [[290.0, 180.0], [90.0, 181.0]], false);
        assert!((canvas.angle() - 180.0).abs() < 1.0);
        canvas.apply(CanvasInput::PinchEnd);
        assert!(matches!(canvas.interaction, Interaction::Idle));
    }

    #[test]
    fn fitting_a_turned_view_fits_the_turned_bounds_in_the_middle() {
        let mut canvas = canvas();
        canvas.turn_to(90.0);
        canvas.fit();
        // 200 wide and 320 high once turned, in a 400 by 300 area.
        assert!((canvas.tab.scale - 300.0 / 320.0).abs() < 1e-9);
        let middle = canvas.to_client([160.0, 100.0]);
        assert!((middle[0] - 210.0).abs() <= 0.5 && (middle[1] - 170.0).abs() <= 0.5);
        let view = canvas.in_view().unwrap();
        // The document's height runs across the wider area, its width fills its height.
        assert!(view[1] < 0.0 && view[3] > 200.0 && view[0] <= 0.5 && view[2] >= 319.5);
    }

    #[test]
    fn angles_wrap_into_a_half_open_turn() {
        for (given, expected) in [
            (0.0, 0.0),
            (5.0, 5.0),
            (180.0, 180.0),
            (-180.0, 180.0),
            (185.0, -175.0),
            (-185.0, 175.0),
            (360.0, 0.0),
            (725.0, 5.0),
        ] {
            assert_eq!(normalized(given), expected, "{given}");
        }
    }
}

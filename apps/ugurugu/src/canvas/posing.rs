// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! The layer move tool: a box around the current layer as the frame shows
//! it, dragged to move the layer, by a handle to scale it or from outside
//! to turn it. The pose shows as it is dragged and is keyed on the frame
//! shown when the drag ends.

use ugu_core::keys::{Pose, limits};
use ugu_core::ops::Affine;

use super::{Canvas, Grip, HANDLES, Interaction, transform::handle_point};

/// Logical pixels from a handle's middle that still grab it.
const HANDLE_REACH: f64 = 7.0;
/// Turns snap to this many degrees with Shift.
const SNAP: f64 = 15.0;

impl Canvas {
    /// What the current layer turns and scales about the first time it is
    /// keyed: the middle of the canvas.
    pub fn default_pivot(&self) -> [f64; 2] {
        self.tab
            .session
            .document()
            .canvas
            .map(|edge| f64::from(edge) / 2.0)
    }

    /// Where the current layer's canvas is shown on this frame, with the
    /// pose being dragged: its corners and the middles of its sides, in
    /// the order of `HANDLES`, in document pixels. `None` unless the move
    /// tool is chosen.
    pub fn pose_box(&self) -> Option<[[f64; 2]; 8]> {
        if self.tools.tool != ugu_session::Tool::Move {
            return None;
        }
        let session = &self.tab.session;
        let layer = session.document().layer(session.current_layer())?;
        let [width, height] = session.document().canvas.map(f64::from);
        let transform = session
            .shown_document()
            .transform(layer.id, session.cycle())
            .unwrap_or(Affine::IDENTITY);
        let bounds = [0.0, 0.0, width, height];
        Some(HANDLES.map(|handle| transform.apply(handle_point(bounds, handle))))
    }

    /// What a drag from `position` (client physical pixels) with the move
    /// tool would change.
    pub fn pose_grip_at(&self, position: [f64; 2]) -> Option<Grip> {
        let corners = self.pose_box()?;
        let reach = HANDLE_REACH * f64::from(self.pixels_per_point);
        for (handle, corner) in HANDLES.iter().zip(corners) {
            let [x, y] = self.to_client(corner);
            if (x - position[0]).hypot(y - position[1]) <= reach {
                return Some(Grip::Scale(*handle));
            }
        }
        let point = self.to_document(position);
        // Inside the shown box, whichever way it is turned.
        let inside = (0..4).all(|index| {
            let [a, b] = [corners[index], corners[(index + 1) % 4]];
            let cross = (b[0] - a[0]) * (point[1] - a[1]) - (b[1] - a[1]) * (point[0] - a[0]);
            cross >= 0.0
        }) || (0..4).all(|index| {
            let [a, b] = [corners[index], corners[(index + 1) % 4]];
            let cross = (b[0] - a[0]) * (point[1] - a[1]) - (b[1] - a[1]) * (point[0] - a[0]);
            cross <= 0.0
        });
        Some(if inside { Grip::Move } else { Grip::Rotate })
    }

    pub(super) fn begin_pose_drag(&mut self, position: [f64; 2]) {
        self.notice = None;
        self.pause_playback();
        let Some(grip) = self.pose_grip_at(position) else {
            return;
        };
        if let Err(error) = self.tab.session.settle() {
            self.notice = Some(super::failed_because(
                crate::i18n::tr("pose-failed"),
                &error,
            ));
            return;
        }
        let layer = self.tab.session.current_layer();
        let base = self.tab.session.pose(layer);
        self.interaction = Interaction::Posing {
            grip,
            start: self.to_document(position),
            base,
        };
    }

    pub(super) fn drag_pose(&mut self, position: [f64; 2]) {
        let Interaction::Posing { grip, start, base } = self.interaction else {
            return;
        };
        let session = &self.tab.session;
        let layer = session.current_layer();
        let pivot = session
            .document()
            .layer(layer)
            .map(|layer| layer.keys())
            .filter(|keys| !keys.is_empty())
            .map_or(self.default_pivot(), |keys| keys.pivot);
        // The pivot as shown: the groups' moves, then the pose's own offset.
        let outer = session
            .document()
            .ancestors(layer)
            .unwrap_or_default()
            .iter()
            .rev()
            .filter_map(|group| group.keys().at(session.cycle(), session.document().frames))
            .reduce(|inner, outer| inner.then(outer));
        let outer = outer.unwrap_or(Affine::IDENTITY);
        let back = outer.inverse().unwrap_or(Affine::IDENTITY);
        // Worked out in the layer's group's pixels, where the pose applies.
        let [from, to] = [start, self.to_document(position)].map(|point| back.apply(point));
        let centre = [pivot[0] + base.offset[0], pivot[1] + base.offset[1]];
        let mut pose = base;
        match grip {
            Grip::Move => {
                let mut by = [to[0] - from[0], to[1] - from[1]];
                if self.tab.session.pixel_mode() {
                    by = by.map(f64::round);
                }
                pose.offset = [base.offset[0] + by[0], base.offset[1] + by[1]];
            }
            Grip::Rotate => {
                let angle = |[x, y]: [f64; 2]| (y - centre[1]).atan2(x - centre[0]).to_degrees();
                let mut rotation = base.rotation + angle(to) - angle(from);
                if self.modifiers.0 {
                    rotation = (rotation / SNAP).round() * SNAP;
                }
                pose.rotation = rotation.clamp(-limits::ROTATION, limits::ROTATION);
            }
            Grip::Scale(_) => {
                let reach = |[x, y]: [f64; 2]| (x - centre[0]).hypot(y - centre[1]);
                let (was, now) = (reach(from), reach(to));
                if was > 0.0 {
                    pose.scale = (base.scale * now / was)
                        .clamp(*limits::SCALE.start(), *limits::SCALE.end());
                }
            }
        }
        self.tab.posed += 1;
        self.tab.session.drag_pose(layer, pose);
    }

    pub(super) fn end_pose_drag(&mut self, position: [f64; 2]) {
        self.drag_pose(position);
        self.interaction = Interaction::Idle;
        let pivot = self.default_pivot();
        let result = self.edit(|session| session.end_pose(pivot));
        if let Err(error) = result {
            self.notice = Some(super::failed_because(
                crate::i18n::tr("pose-failed"),
                &error,
            ));
        }
    }

    /// Gives the current layer `pose` on this frame, keying it there.
    pub fn set_pose(&mut self, pose: Pose) {
        let layer = self.tab.session.current_layer();
        let pivot = self.default_pivot();
        if let Err(error) = self.edit(|session| session.set_pose(layer, pose, pivot)) {
            self.notice = Some(super::failed_because(
                crate::i18n::tr("pose-failed"),
                &error,
            ));
        }
    }

    /// Adds a key on this frame to the current layer, or takes it away.
    pub fn toggle_key(&mut self) {
        let layer = self.tab.session.current_layer();
        let pivot = self.default_pivot();
        if let Err(error) = self.edit(|session| session.toggle_key(layer, pivot)) {
            self.notice = Some(super::failed_because(
                crate::i18n::tr("pose-failed"),
                &error,
            ));
        }
    }

    /// Goes to the frame before (`-1`) or after (`1`).
    pub fn step_frame(&mut self, by: i64) {
        if self.is_playing() {
            self.toggle_playback();
        }
        let frame = self.tab.session.frame() + by;
        self.edit_settled(|session| session.set_frame(frame));
    }

    /// Goes to the key before (`-1`) or after (`1`) of the current layer or
    /// the groups holding it.
    pub fn step_key(&mut self, direction: i32) {
        let Some(frame) = self.tab.session.neighbour_key(direction) else {
            return;
        };
        if self.is_playing() {
            self.toggle_playback();
        }
        self.edit_settled(|session| session.set_frame(i64::from(frame)));
    }

    /// Goes to `frame`, stopping playback.
    pub fn go_to_frame(&mut self, frame: u32) {
        if self.is_playing() {
            self.toggle_playback();
        }
        self.edit_settled(|session| session.set_frame(i64::from(frame)));
    }
}

impl Canvas {
    /// Shows `pose` for the current layer without keying it yet.
    pub fn preview_pose(&mut self, pose: Pose) {
        let layer = self.tab.session.current_layer();
        self.tab.posed += 1;
        self.tab.session.drag_pose(layer, pose);
    }

    /// How the current layer's key on this frame eases on to the next.
    pub fn set_ease(&mut self, ease: ugu_core::keys::Ease) {
        let layer = self.tab.session.current_layer();
        let frame = self.tab.session.cycle();
        if let Err(error) = self.edit(|session| session.set_ease(layer, frame, ease)) {
            self.notice = Some(super::failed_because(
                crate::i18n::tr("pose-failed"),
                &error,
            ));
        }
    }
}

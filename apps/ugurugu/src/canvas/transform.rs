// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! The transform box: a pending transform of the selected part, changed by
//! dragging its handles on the canvas and shown on the edited layer as it
//! would be applied.

use ugu_core::edit::EditError;
use ugu_core::ops::{Affine, Sampling};
use ugu_render::moving::{Glance, Moving};
use ugu_render::raster::PixelRect;
use ugu_session::{Ended, FillError, Session};

use crate::i18n::tr;

use super::{Canvas, Interaction, Key, preview_shrink, union};

/// Logical pixels from a handle's middle that still grab it.
const HANDLE_REACH: f64 = 7.0;
const SNAP: f64 = std::f64::consts::PI / 12.0;
/// Selections smaller than this many pixels are shown at full size while
/// dragged: changing them takes a few milliseconds.
const GLANCE_LEAST: f64 = 1024.0 * 1024.0;

/// What a drag on the transform box changes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Grip {
    Move,
    /// A corner or an edge: -1, 0 or 1 across and down from the middle.
    Scale([i8; 2]),
    Rotate,
}

/// What a preview shows: the transform, sampling and whether the selected
/// part stays.
type Shown = (Affine, Sampling, bool);

/// The preview of a pending transform and what it shows.
pub(super) struct Preview {
    moving: Moving,
    shown: Option<Shown>,
    /// While a large selection is dragged: the transform where it is seen,
    /// as small as it is seen, and what it shows; the layer follows when the
    /// drag ends.
    glance: Option<(Glance, Option<Shown>)>,
}

impl Preview {
    pub(super) fn glance(&self) -> Option<&Glance> {
        self.glance.as_ref().map(|(glance, _)| glance)
    }
}

/// The selected part's box: left, top, right, bottom before the transform.
fn source_box(canvas: &Canvas) -> Option<([f64; 4], Affine)> {
    let pending = canvas.tab.session.pending()?;
    let [left, top, width, height] = pending.selection.mask().bounds.map(f64::from);
    Some(([left, top, left + width, top + height], pending.transform))
}

pub(super) fn handle_point([left, top, right, bottom]: [f64; 4], [x, y]: [i8; 2]) -> [f64; 2] {
    let pick = |low: f64, high: f64, at: i8| match at {
        -1 => low,
        1 => high,
        _ => (low + high) / 2.0,
    };
    [pick(left, right, x), pick(top, bottom, y)]
}

/// The eight handles, corners first.
pub const HANDLES: [[i8; 2]; 8] = [
    [-1, -1],
    [1, -1],
    [1, 1],
    [-1, 1],
    [0, -1],
    [1, 0],
    [0, 1],
    [-1, 0],
];

impl Canvas {
    /// The transform box's corners and handles in document pixels, in the
    /// order of `HANDLES`; `None` without a pending transform.
    pub fn transform_box(&self) -> Option<[[f64; 2]; 8]> {
        let (bounds, transform) = source_box(self)?;
        Some(HANDLES.map(|handle| transform.apply(handle_point(bounds, handle))))
    }

    /// Left, top, right and bottom of the selection as shown, moved by a
    /// pending transform, in document pixels.
    pub fn selection_bounds(&self) -> Option<[f64; 4]> {
        if let Some(corners) = self.transform_box() {
            return Some(corners.iter().fold(
                [
                    f64::INFINITY,
                    f64::INFINITY,
                    f64::NEG_INFINITY,
                    f64::NEG_INFINITY,
                ],
                |[l, t, r, b], &[x, y]| [l.min(x), t.min(y), r.max(x), b.max(y)],
            ));
        }
        let [left, top, width, height] = self.tab.session.selection()?.mask().bounds.map(f64::from);
        Some([left, top, left + width, top + height])
    }

    /// Whether a drawing, selecting or transforming gesture is under way.
    pub fn is_dragging(&self) -> bool {
        !matches!(
            self.interaction,
            Interaction::Idle | Interaction::Panning { .. }
        )
    }

    /// What a drag starting at `position` (client physical pixels) would
    /// change; `None` without a pending transform.
    pub fn grip_at(&self, position: [f64; 2]) -> Option<Grip> {
        let (bounds, transform) = source_box(self)?;
        let reach = HANDLE_REACH * f64::from(self.pixels_per_point);
        let screen = |point: [f64; 2]| self.to_client(point);
        for handle in HANDLES {
            let [x, y] = screen(transform.apply(handle_point(bounds, handle)));
            if (x - position[0]).hypot(y - position[1]) <= reach {
                return Some(Grip::Scale(handle));
            }
        }
        let [x, y] = transform.inverse()?.apply(self.to_document(position));
        let [left, top, right, bottom] = bounds;
        Some(
            if (left..=right).contains(&x) && (top..=bottom).contains(&y) {
                Grip::Move
            } else {
                Grip::Rotate
            },
        )
    }

    pub(super) fn begin_transform_drag(&mut self, position: [f64; 2]) {
        let (Some(grip), Some(pending)) = (self.grip_at(position), self.tab.session.pending())
        else {
            return;
        };
        self.interaction = Interaction::Transforming {
            grip,
            start: self.to_document(position),
            base: pending.transform,
        };
    }

    pub(super) fn drag_transform(&mut self, position: [f64; 2]) {
        let Interaction::Transforming { grip, start, base } = self.interaction else {
            return;
        };
        let Some((bounds, _)) = source_box(self) else {
            return;
        };
        let point = self.to_document(position);
        let shift = self.modifiers.0;
        let transform = match grip {
            Grip::Move => {
                let mut by = [point[0] - start[0], point[1] - start[1]];
                // Whole pixels in pixel mode, so the pixels moved stay pixels.
                if self.tab.session.pixel_mode() {
                    by = by.map(f64::round);
                }
                base.then(Affine::translation(by[0], by[1]))
            }
            Grip::Rotate => {
                let centre = base.apply(handle_point(bounds, [0, 0]));
                let angle = |[x, y]: [f64; 2]| (y - centre[1]).atan2(x - centre[0]);
                let mut turn = angle(point) - angle(start);
                if shift {
                    // The box's own angle snaps, not the turn.
                    let [a, _, _, d, _, _] = base.0;
                    let now = d.atan2(a);
                    turn = ((now + turn) / SNAP).round() * SNAP - now;
                }
                base.then(Affine::rotation_about(turn, centre))
            }
            Grip::Scale(handle) => {
                let Some(inverse) = base.inverse() else {
                    return;
                };
                scaled(bounds, handle, inverse.apply(point), shift).then(base)
            }
        };
        // Shown once per frame by `sync`, with the newest transform: input
        // comes faster than a large selection can be drawn.
        self.tab.session.set_transform(transform);
    }

    /// Starts a transform of the selection (Ctrl+T).
    pub fn begin_transform(&mut self) {
        self.notice = None;
        if let Err(error) = self.edit(|session| session.begin_transform()) {
            self.fill_notice(&error);
        }
        self.refresh_preview();
    }

    /// Whether no transform is left pending.
    pub fn apply_transform(&mut self) -> bool {
        if let Err(error) = self.edit(|session| session.apply_transform()) {
            self.notice = Some(super::failed_because(
                crate::i18n::tr("transform-failed"),
                &error,
            ));
        }
        self.tab.session.pending().is_none()
    }

    /// Applies a pending transform or placed text, as what comes next
    /// would leave it out or end it; whether nothing is left pending.
    pub fn apply_pending(&mut self) -> bool {
        self.apply_transform() && self.apply_text()
    }

    /// Makes `change`, which applies a pending transform or placed text
    /// first; when they cannot be, says so and leaves them pending.
    pub fn edit_settled<R: Default>(
        &mut self,
        change: impl FnOnce(&mut Session) -> Result<R, EditError>,
    ) -> R {
        self.edit(change).unwrap_or_else(|_| {
            self.notice = Some(tr("restyle-pending").to_owned());
            R::default()
        })
    }

    /// Changes the tool, saying why when a pending transform or placed text
    /// the document cannot take keeps it.
    pub fn choose_tool(&mut self, tool: ugu_session::Tool) {
        if self.set_tool(tool).is_err() {
            self.notice = Some(tr("restyle-pending").to_owned());
        }
    }

    pub fn cancel_transform(&mut self) {
        self.edit(|session| session.cancel_transform());
    }

    pub fn flip(&mut self, horizontally: bool) {
        self.notice = None;
        if let Err(error) = self.edit(|session| session.flip(horizontally)) {
            self.fill_notice(&error);
        }
        self.refresh_preview();
    }

    /// Duplicate: whether the selected part also stays where it was.
    pub fn set_duplicate(&mut self, keep: bool) {
        self.notice = None;
        let begun = self.edit(|session| {
            session.begin_transform()?;
            session.set_keep_source(keep);
            Ok::<_, FillError>(())
        });
        if let Err(error) = begun {
            self.fill_notice(&error);
        }
        self.refresh_preview();
    }

    pub fn delete_selected(&mut self) {
        self.notice = None;
        if let Err(error) = self.edit(|session| session.delete_selected()) {
            self.fill_notice(&error);
        }
    }

    pub fn set_transform_sampling(&mut self, sampling: Sampling) {
        self.tools.transform_sampling = sampling;
        self.edit(|_| ());
        self.refresh_preview();
    }

    /// After the session changed: ends the preview of a transform or text
    /// that ended, keeping its pixels when it was applied and nothing else
    /// changed the split's layer and frame, and shows a pending one.
    pub(super) fn follow_transform(&mut self, before: Key) {
        match self.tab.session.take_ended() {
            Some(Ended::Applied { revision }) => {
                let after = self.key();
                let moving = self.tab.preview.take().map(|preview| preview.moving);
                let placing = self.tab.text_preview.take().map(|preview| preview.placing);
                if let Some((key, split)) = &mut self.tab.split
                    && (moving.is_some() || placing.is_some())
                {
                    if let Some(moving) = moving {
                        split.end_move(moving, true);
                    }
                    if let Some(placing) = placing {
                        split.end_place(placing, true);
                    }
                    let unchanged = *key == before
                        && after.version.revision == revision
                        && after.layer == key.layer
                        && after.frame == key.frame;
                    if unchanged {
                        *key = after;
                    }
                }
            }
            Some(Ended::Dropped) => {
                if let (Some(preview), Some((_, split))) =
                    (self.tab.preview.take(), &mut self.tab.split)
                {
                    let rect = split.end_move(preview.moving, false);
                    self.recomposite(rect);
                }
                if let (Some(preview), Some((_, split))) =
                    (self.tab.text_preview.take(), &mut self.tab.split)
                {
                    let rect = split.end_place(preview.placing, false);
                    self.recomposite(rect);
                }
            }
            None => {}
        }
        self.refresh_preview();
        self.refresh_text_preview();
    }

    /// Where and how small a dragged transform is shown: the part of the
    /// canvas in view, as small as keeps a pixel for every screen pixel.
    /// `None` when no large selection is dragged, or when that would be no
    /// fewer pixels than the canvas.
    fn glance_view(&self) -> Option<(PixelRect, u32)> {
        if !matches!(self.interaction, Interaction::Transforming { .. }) {
            return None;
        }
        let ([left, top, right, bottom], _) = source_box(self)?;
        if (right - left) * (bottom - top) < GLANCE_LEAST {
            return None;
        }
        let size = self.tab.session.document().canvas;
        let rect = ugu_render::compose::clamp(self.in_view()?, size)?;
        let shrink = preview_shrink(self.tab.scale);
        let rect = [
            rect[0] / shrink * shrink,
            rect[1] / shrink * shrink,
            rect[2],
            rect[3],
        ];
        let pixels = u64::from(rect[2] - rect[0]) * u64::from(rect[3] - rect[1])
            / u64::from(shrink * shrink);
        (pixels * 2 <= u64::from(size[0]) * u64::from(size[1])).then_some((rect, shrink))
    }

    /// Shows the pending transform on the split's layer, once the split of
    /// the state it applies to is here.
    pub(super) fn refresh_preview(&mut self) {
        let key = self.key();
        let view = self.glance_view();
        let Some(pending) = self.tab.session.pending() else {
            self.tab.preview = None;
            return;
        };
        let Some((shown, split)) = self.tab.split.as_mut() else {
            self.tab.preview = None;
            return;
        };
        if *shown != key || split.layer != pending.layer {
            return;
        }
        // On a layer moved on this frame, in its own pixels.
        let Some((selection, transform)) = pending.in_layer(self.tab.session.layer_transform())
        else {
            return;
        };
        let view = view.filter(|_| split.moved.is_none());
        let threads =
            std::thread::available_parallelism().map_or(1, |count| count.get().min(8)) as u16;
        if self.tab.preview.is_none() {
            let Some(moving) = split.begin_move(selection.mask(), threads) else {
                return;
            };
            self.tab.preview = Some(Preview {
                moving,
                shown: None,
                glance: None,
            });
        }
        let preview = self.tab.preview.as_mut().expect("made above");
        let wanted = (
            transform,
            self.tab.session.sampling(self.tools.transform_sampling),
            pending.keep_source,
        );
        if let Some(view) = view {
            let started = std::time::Instant::now();
            if preview.glance().map(Glance::view) != Some(view) {
                preview.glance = split
                    .glance(&preview.moving, view.0, view.1, threads)
                    .map(|glance| (glance, None));
                tracing::debug!(
                    ms = started.elapsed().as_secs_f64() * 1000.0,
                    ?view,
                    "transform glance made"
                );
            }
            if let Some((glance, glanced)) = &mut preview.glance {
                if *glanced != Some(wanted) {
                    let started = std::time::Instant::now();
                    let rect = glance.show(wanted.0, wanted.1, wanted.2);
                    *glanced = Some(wanted);
                    self.upload = union(self.upload, rect);
                    tracing::debug!(
                        ms = started.elapsed().as_secs_f64() * 1000.0,
                        "transform glanced"
                    );
                }
                return;
            }
        }
        preview.glance = None;
        if preview.shown == Some(wanted) {
            return;
        }
        let started = std::time::Instant::now();
        let rect = split.show_move(&mut preview.moving, wanted.0, wanted.1, wanted.2);
        preview.shown = Some(wanted);
        let layer = started.elapsed();
        self.recomposite(rect);
        tracing::debug!(
            ms = started.elapsed().as_secs_f64() * 1000.0,
            layer_ms = layer.as_secs_f64() * 1000.0,
            "transform shown"
        );
    }
}

/// The scaling in the selection's own pixels that takes `handle` of
/// `bounds` to `point`, the opposite side staying; with `keep_ratio` both
/// ways alike.
fn scaled(bounds: [f64; 4], handle: [i8; 2], point: [f64; 2], keep_ratio: bool) -> Affine {
    let anchor = handle_point(bounds, handle.map(|at| -at));
    let grabbed = handle_point(bounds, handle);
    let size = [bounds[2] - bounds[0], bounds[3] - bounds[1]];
    let mut scale = [0, 1].map(|axis| {
        if handle[axis] == 0 {
            return 1.0;
        }
        let factor = (point[axis] - anchor[axis]) / (grabbed[axis] - anchor[axis]);
        // Never thinner than a pixel, which would flatten it.
        let least = 1.0 / size[axis];
        if factor.abs() < least {
            least.copysign(factor)
        } else {
            factor
        }
    });
    if keep_ratio {
        let factor = match handle {
            [0, _] => scale[1],
            [_, 0] => scale[0],
            _ => {
                // Along the diagonal from the anchor.
                let along = [grabbed[0] - anchor[0], grabbed[1] - anchor[1]];
                let to = [point[0] - anchor[0], point[1] - anchor[1]];
                (to[0] * along[0] + to[1] * along[1]) / (along[0].powi(2) + along[1].powi(2))
            }
        };
        let factor = if factor.abs() < 1e-3 {
            1e-3_f64.copysign(factor)
        } else {
            factor
        };
        scale = [factor, factor];
    }
    // An edge with the ratio kept grows both ways about its middle.
    let centre = match handle {
        [0, _] if keep_ratio => [(bounds[0] + bounds[2]) / 2.0, anchor[1]],
        [_, 0] if keep_ratio => [anchor[0], (bounds[1] + bounds[3]) / 2.0],
        _ => anchor,
    };
    Affine::scaling_about(scale, centre)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_handle_scales_from_the_opposite_side() {
        let bounds = [10.0, 20.0, 30.0, 60.0];
        let corner = scaled(bounds, [1, 1], [50.0, 70.0], false);
        assert_eq!(corner.apply([10.0, 20.0]), [10.0, 20.0]);
        assert_eq!(corner.apply([30.0, 60.0]), [50.0, 70.0]);
        let edge = scaled(bounds, [-1, 0], [0.0, 999.0], false);
        assert_eq!(edge.apply([10.0, 20.0]), [0.0, 20.0]);
        assert_eq!(edge.apply([30.0, 60.0]), [30.0, 60.0]);
        // Keeping the ratio, a corner follows the diagonal.
        let kept = scaled(bounds, [1, 1], [50.0, 60.0], true);
        let [x, y] = kept.apply([30.0, 60.0]);
        assert!(((x - 10.0) / 20.0 - (y - 20.0) / 40.0).abs() < 1e-9);
        // Past the anchor it flips, but never flattens.
        let thin = scaled(bounds, [1, 0], [10.0, 0.0], false);
        assert!(thin.inverse().is_some());
    }
}

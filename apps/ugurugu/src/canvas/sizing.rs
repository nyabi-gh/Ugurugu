// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! Changing the canvas size on the canvas: a frame for the new canvas over
//! the document, whose handles and inside are dragged, with the size, the
//! reference point and the offset typed beside it. Applying crops or extends
//! the canvas as the frame shows.

use ugu_core::document::limits;
use ugu_core::edit::{EditError, Outcome};
use ugu_session::Session;

use super::transform::{HANDLE_REACH, HANDLES, handle_point};
use super::{Canvas, Grip, Interaction};

const EDGE: std::ops::RangeInclusive<u32> = limits::CANVAS_EDGE;

/// How much of the canvas area the document takes when sizing starts, so
/// that there is room around it to drag the frame out into.
const ROOM_SHARE: f64 = 0.7;

pub fn clamp_edge(edge: i64) -> u32 {
    edge.clamp(i64::from(*EDGE.start()), i64::from(*EDGE.end())) as u32
}

/// Where the reference point puts the artwork along one axis when the
/// canvas grows by `delta`: 0 start, 1 middle, 2 end. The middle rounds
/// toward zero, as 2.2.13 does.
fn aligned(delta: i32, place: usize) -> i32 {
    match place {
        0 => 0,
        1 => delta / 2,
        _ => delta,
    }
}

/// A new canvas size and where the artwork lands on it.
#[derive(Clone, Debug, PartialEq)]
pub struct CanvasSize {
    pub current: [u32; 2],
    pub size: [u32; 2],
    /// Whether the size is shown as a change from the current one.
    pub relative: bool,
    /// The reference point, row-major from the top left; `None` when the
    /// offset was typed or dragged and matches none.
    pub anchor: Option<usize>,
    /// Where the artwork's top left lands on the new canvas.
    pub offset: [i32; 2],
}

impl CanvasSize {
    pub fn new(current: [u32; 2]) -> Self {
        Self {
            current,
            size: current,
            relative: false,
            anchor: Some(4),
            offset: [0, 0],
        }
    }

    fn anchored(&self, anchor: usize) -> [i32; 2] {
        let delta = |axis: usize| self.size[axis] as i32 - self.current[axis] as i32;
        [aligned(delta(0), anchor % 3), aligned(delta(1), anchor / 3)]
    }

    pub fn set_size(&mut self, size: [u32; 2]) {
        self.size = size.map(|edge| clamp_edge(i64::from(edge)));
        if let Some(anchor) = self.anchor {
            self.offset = self.anchored(anchor);
        }
    }

    pub fn set_anchor(&mut self, anchor: usize) {
        self.anchor = Some(anchor);
        self.offset = self.anchored(anchor);
    }

    /// Takes a typed offset; the reference point becomes the one that
    /// gives it, keeping the current one when it still does.
    pub fn set_offset(&mut self, offset: [i32; 2]) {
        let reach = *EDGE.end() as i32;
        self.offset = offset.map(|value| value.clamp(-reach, reach));
        let matches = |anchor: &usize| self.anchored(*anchor) == self.offset;
        self.anchor = self.anchor.filter(matches).or_else(|| (0..9).find(matches));
    }

    /// The new canvas in the current document's pixels: left, top, right
    /// and bottom.
    pub fn frame(&self) -> [i64; 4] {
        let [x, y] = self.offset.map(i64::from);
        let [width, height] = self.size.map(i64::from);
        [-x, -y, width - x, height - y]
    }

    /// Takes a dragged frame, as `frame` gives it.
    pub fn set_frame(&mut self, [left, top, right, bottom]: [i64; 4]) {
        self.size = [clamp_edge(right - left), clamp_edge(bottom - top)];
        let reach = i64::from(*EDGE.end());
        self.set_offset([-left, -top].map(|value| value.clamp(-reach, reach) as i32));
    }

    /// Whether applying changes anything: a new size, or the same size
    /// with the artwork moved.
    pub fn changes(&self) -> bool {
        self.size != self.current || self.offset != [0, 0]
    }

    pub fn hint(&self) -> &'static str {
        let [x, y] = self.offset;
        let [width, height] = self.current.map(|edge| edge as i32);
        let [new_width, new_height] = self.size.map(|edge| edge as i32);
        if x >= new_width || y >= new_height || x + width <= 0 || y + height <= 0 {
            "canvas-size-outside"
        } else if x < 0 || y < 0 || x + width > new_width || y + height > new_height {
            "canvas-size-clipped"
        } else if self.changes() {
            "canvas-size-expanded"
        } else {
            "canvas-size-unchanged"
        }
    }
}

/// The canvas being sized and the view before, which cancelling puts back.
pub(super) struct Sizing {
    pub(super) size: CanvasSize,
    view: (f64, [f64; 2]),
    /// Where the window the size is typed in is, in client physical pixels,
    /// once it is shown.
    cover: Option<[i32; 4]>,
    /// Whether the view was zoomed out for it, clear of that window.
    placed: bool,
}

/// The part of `area` (left, top, right, bottom) beside `cover`, left or
/// right of it, whichever is wider; all of `area` when `cover` is not over
/// it or leaves too little room either side.
fn beside(area: [i32; 4], cover: [i32; 4]) -> [i32; 4] {
    let over = cover[0] < area[2] && cover[2] > area[0] && cover[1] < area[3] && cover[3] > area[1];
    if !over {
        return area;
    }
    let left = [area[0], area[1], cover[0].max(area[0]), area[3]];
    let right = [cover[2].min(area[2]), area[1], area[2], area[3]];
    let wider = if left[2] - left[0] >= right[2] - right[0] {
        left
    } else {
        right
    };
    if (wider[2] - wider[0]) * 5 >= (area[2] - area[0]) * 2 {
        wider
    } else {
        area
    }
}

/// `base` (left, top, right, bottom) dragged by `by` document pixels from
/// `grip`, in whole pixels and within the edge limits: a handle moves its
/// sides and the inside moves the whole frame. `keep_ratio` scales both
/// ways alike and `from_middle` moves the opposite side the other way.
fn dragged(
    base: [i64; 4],
    grip: Grip,
    by: [f64; 2],
    keep_ratio: bool,
    from_middle: bool,
) -> [i64; 4] {
    let by = by.map(|value| value.round() as i64);
    let handle = match grip {
        Grip::Move => {
            return [
                base[0] + by[0],
                base[1] + by[1],
                base[2] + by[0],
                base[3] + by[1],
            ];
        }
        Grip::Scale(handle) => handle.map(i64::from),
        Grip::Rotate => return base,
    };
    let size = [base[2] - base[0], base[3] - base[1]];
    let reach = if from_middle { 2 } else { 1 };
    let mut wanted: [i64; 2] =
        std::array::from_fn(|axis| size[axis] + reach * handle[axis] * by[axis]);
    if keep_ratio {
        let factors = [0, 1].map(|axis| wanted[axis] as f64 / size[axis] as f64);
        // The side dragged further sets both; an edge sets the other side.
        let factor = match handle {
            [0, _] => factors[1],
            [_, 0] => factors[0],
            _ if (factors[0] - 1.0).abs() >= (factors[1] - 1.0).abs() => factors[0],
            _ => factors[1],
        };
        wanted = size.map(|edge| (edge as f64 * factor).round() as i64);
    }
    let wanted = wanted.map(|edge| i64::from(clamp_edge(edge)));
    let mut frame = base;
    for axis in 0..2 {
        let (low, high) = (base[axis], base[axis + 2]);
        let start = match handle[axis] {
            // An edge with the ratio kept grows both ways about its middle.
            _ if from_middle || handle[axis] == 0 => {
                low - (wanted[axis] - size[axis]).div_euclid(2)
            }
            -1 => high - wanted[axis],
            _ => low,
        };
        frame[axis] = start;
        frame[axis + 2] = start + wanted[axis];
    }
    frame
}

impl Canvas {
    pub fn sizing(&self) -> Option<&CanvasSize> {
        self.sizing.as_ref().map(|sizing| &sizing.size)
    }

    pub fn sizing_mut(&mut self) -> Option<&mut CanvasSize> {
        self.sizing.as_mut().map(|sizing| &mut sizing.size)
    }

    /// Starts sizing the canvas: a pending transform or placed text is
    /// applied and playback stops. Once the window it is typed in is shown,
    /// the view zooms out to leave room around the document, beside it.
    pub fn begin_sizing(&mut self) {
        if self.sizing.is_some() {
            return;
        }
        self.cancel_gesture();
        if self.edit(Session::settle).is_err() {
            self.notice = Some(crate::i18n::tr("restyle-pending").to_owned());
            return;
        }
        self.stop_playback();
        self.sizing = Some(Sizing {
            size: CanvasSize::new(self.tab.session.document().canvas),
            view: (self.tab.scale, self.tab.offset),
            cover: None,
            placed: false,
        });
    }

    /// Where the window the size is typed in is now, in client physical
    /// pixels; the first places the view.
    pub fn set_sizing_cover(&mut self, cover: [i32; 4]) {
        let Some(sizing) = &mut self.sizing else {
            return;
        };
        let first = sizing.cover.is_none();
        sizing.cover = Some(cover);
        if first && let Some(area) = self.area {
            self.place_sizing(area);
        }
    }

    /// Zooms the view out around the document in `area`, beside the window
    /// the size is typed in, once per sizing.
    pub(super) fn place_sizing(&mut self, area: [i32; 4]) {
        let area = match &mut self.sizing {
            Some(Sizing {
                cover: Some(cover),
                placed,
                ..
            }) if !*placed => {
                *placed = true;
                beside(area, *cover)
            }
            _ => return,
        };
        self.place(area);
        self.tab.scale *= ROOM_SHARE;
        self.tab.offset = [0.0, 0.0];
        let middle = self.to_client(self.middle());
        let centre = [
            f64::from(area[0] + area[2]) / 2.0,
            f64::from(area[1] + area[3]) / 2.0,
        ];
        self.tab.offset = std::array::from_fn(|axis| (centre[axis] - middle[axis]).round());
    }

    /// Changes the canvas as the frame shows; the view fits the new canvas.
    pub fn apply_sizing(&mut self) -> Result<Outcome, EditError> {
        self.cancel_gesture();
        let Some(sizing) = self.sizing.take() else {
            return Ok(Outcome::NoChange);
        };
        let (offset, size) = (sizing.size.offset, sizing.size.size);
        let result = self.edit(|session| session.crop_canvas(offset, size));
        if !matches!(result, Ok(Outcome::Committed(_))) {
            (self.tab.scale, self.tab.offset) = sizing.view;
        }
        result
    }

    /// Leaves the canvas as it was, and the view as it was before sizing.
    pub fn cancel_sizing(&mut self) {
        self.cancel_gesture();
        if let Some(sizing) = self.sizing.take() {
            (self.tab.scale, self.tab.offset) = sizing.view;
        }
    }

    /// The frame's corners and handles in document pixels, in the order of
    /// `HANDLES`.
    pub fn sizing_box(&self) -> Option<[[f64; 2]; 8]> {
        let frame = self.sizing()?.frame().map(|edge| edge as f64);
        Some(HANDLES.map(|handle| handle_point(frame, handle)))
    }

    /// What a drag starting at `position` (client physical pixels) would
    /// change: a handle, or the frame from inside it.
    pub fn sizing_grip_at(&self, position: [f64; 2]) -> Option<Grip> {
        let corners = self.sizing_box()?;
        let reach = HANDLE_REACH * f64::from(self.pixels_per_point);
        for (handle, corner) in HANDLES.iter().zip(corners) {
            let [x, y] = self.to_client(corner);
            if (x - position[0]).hypot(y - position[1]) <= reach {
                return Some(Grip::Scale(*handle));
            }
        }
        let [x, y] = self.to_document(position);
        let [[left, top], _, [right, bottom], ..] = corners;
        ((left..=right).contains(&x) && (top..=bottom).contains(&y)).then_some(Grip::Move)
    }

    pub(super) fn begin_sizing_drag(&mut self, position: [f64; 2]) {
        let (Some(grip), Some(size)) = (self.sizing_grip_at(position), self.sizing()) else {
            return;
        };
        self.interaction = Interaction::Sizing {
            grip,
            start: self.to_document(position),
            base: size.frame(),
        };
    }

    pub(super) fn drag_sizing(&mut self, position: [f64; 2]) {
        let Interaction::Sizing { grip, start, base } = self.interaction else {
            return;
        };
        let point = self.to_document(position);
        let by = [point[0] - start[0], point[1] - start[1]];
        let (shift, alt) = self.modifiers;
        let frame = dragged(base, grip, by, shift, alt);
        if let Some(size) = self.sizing_mut() {
            size.set_frame(frame);
        }
    }

    /// Puts the frame back where the drag under way began.
    pub(super) fn undrag_sizing(&mut self, base: [i64; 4]) {
        if let Some(size) = self.sizing_mut() {
            size.set_frame(base);
        }
    }
}

#[cfg(test)]
mod tests {
    use ugu_core::document::Document;
    use ugu_win::pointer::{PointerKind, PointerSample};

    use crate::input::{CanvasInput, Gesture};

    use super::*;

    #[test]
    fn the_reference_point_and_typed_offsets_follow_each_other() {
        // As 2.2.13's configuresCanvasSizeDialog.
        let mut dialog = CanvasSize::new([640, 480]);
        assert_eq!((dialog.anchor, dialog.offset), (Some(4), [0, 0]));
        assert!(!dialog.changes());
        dialog.set_size([800, 600]);
        assert_eq!(dialog.offset, [80, 60]);
        dialog.set_anchor(0);
        assert_eq!(dialog.offset, [0, 0]);
        dialog.set_anchor(8);
        assert_eq!(dialog.offset, [160, 120]);
        dialog.set_size([540, 500]);
        assert_eq!(dialog.offset, [-100, 20]);
        dialog.set_anchor(4);
        assert_eq!(dialog.offset, [-50, 10]);
        dialog.set_offset([-50, 11]);
        assert_eq!(dialog.anchor, None);
        dialog.set_offset([0, 20]);
        assert_eq!(dialog.anchor, Some(6));
        // The middle rounds toward zero.
        dialog.set_size([635, 485]);
        dialog.set_anchor(4);
        assert_eq!(dialog.offset, [-2, 2]);
        // The same size with the artwork moved is a change.
        let mut shift = CanvasSize::new([100, 100]);
        shift.set_offset([-20, 5]);
        assert!(shift.changes());
        assert_eq!(shift.anchor, None);
    }

    #[test]
    fn canvas_sizes_and_offsets_stay_within_the_limits() {
        let mut dialog = CanvasSize::new([100, 80]);
        dialog.set_size([0, 5000]);
        assert_eq!(dialog.size, [1, 4096]);
        dialog.set_offset([-9000, 9000]);
        assert_eq!(dialog.offset, [-4096, 4096]);
        assert_eq!(dialog.hint(), "canvas-size-outside");
        dialog.set_size([100, 80]);
        dialog.set_offset([-10, 0]);
        assert_eq!(dialog.hint(), "canvas-size-clipped");
        dialog.set_size([120, 80]);
        dialog.set_offset([10, 0]);
        assert_eq!(dialog.hint(), "canvas-size-expanded");
    }

    #[test]
    fn the_document_goes_beside_the_window_when_there_is_room() {
        let area = [0, 0, 1000, 600];
        assert_eq!(beside(area, [1100, 0, 1300, 400]), area);
        assert_eq!(beside(area, [700, 50, 980, 500]), [0, 0, 700, 600]);
        assert_eq!(beside(area, [20, 50, 300, 500]), [300, 0, 1000, 600]);
        // Over the middle of a narrow area: the window is moved, not the view.
        assert_eq!(beside(area, [300, 50, 700, 500]), area);
    }

    #[test]
    fn the_frame_and_the_typed_size_are_one() {
        let mut size = CanvasSize::new([100, 80]);
        assert_eq!(size.frame(), [0, 0, 100, 80]);
        size.set_frame([-20, 0, 100, 90]);
        assert_eq!((size.size, size.offset), ([120, 90], [20, 0]));
        // Grown to the left and down: the artwork keeps the top right.
        assert_eq!(size.anchor, Some(2));
        size.set_frame([-10, -10, 120, 90]);
        assert_eq!(size.anchor, None);
        size.set_frame([-20, -10, 100, 80]);
        assert_eq!(size.anchor, Some(8));
        size.set_size([140, 100]);
        assert_eq!(size.frame(), [-40, -20, 100, 80]);
    }

    #[test]
    fn handles_move_their_sides_in_whole_pixels_within_the_limits() {
        let base = [0, 0, 100, 80];
        let right = Grip::Scale([1, 0]);
        assert_eq!(
            dragged(base, right, [20.4, 7.0], false, false),
            [0, 0, 120, 80]
        );
        let top_left = Grip::Scale([-1, -1]);
        assert_eq!(
            dragged(base, top_left, [-10.0, 5.6], false, false),
            [-10, 6, 100, 80]
        );
        assert_eq!(
            dragged(base, Grip::Move, [3.5, -2.2], false, false),
            [4, -2, 104, 78]
        );
        // Alt: the opposite side moves the other way.
        assert_eq!(
            dragged(base, right, [10.0, 0.0], false, true),
            [-10, 0, 110, 80]
        );
        // Shift: a corner keeps the ratio, an edge grows about its middle.
        let bottom_right = Grip::Scale([1, 1]);
        assert_eq!(
            dragged(base, bottom_right, [100.0, 2.0], true, false),
            [0, 0, 200, 160]
        );
        assert_eq!(
            dragged(base, right, [100.0, 0.0], true, false),
            [0, -40, 200, 120]
        );
        // Never thinner than a pixel, never past the largest edge.
        assert_eq!(
            dragged(base, right, [-500.0, 0.0], false, false),
            [0, 0, 1, 80]
        );
        assert_eq!(
            dragged(base, top_left, [-9000.0, 0.0], false, false),
            [-3996, 0, 100, 80]
        );
    }

    fn sample(position: [f64; 2]) -> PointerSample {
        PointerSample {
            position,
            pressure: None,
            tilt: None,
            twist: None,
            buttons: ugu_win::pointer::Buttons::empty(),
            in_contact: true,
            inverted: false,
            time: ugu_win::clock::Ticks(0),
            frame_id: 0,
        }
    }

    #[test]
    fn dragging_the_frame_and_applying_crops_the_canvas_as_it_shows() {
        let mut canvas = Canvas::new(Document::new([200, 100]), |_| {});
        canvas.set_area([0, 0, 800, 600]);
        let scale = canvas.tab.scale;
        canvas.begin_sizing();
        // Placed once the window is shown, beside it.
        canvas.set_area([0, 0, 800, 600]);
        assert_eq!(canvas.tab.scale, scale);
        canvas.set_sizing_cover([560, 20, 790, 400]);
        assert!(canvas.tab.scale < scale);
        let [left, top] = canvas.to_client([0.0, 0.0]);
        let [right, bottom] = canvas.to_client([200.0, 100.0]);
        assert!(left > 0.0 && right < 560.0 && top > 0.0 && bottom < 600.0);
        assert!((left - (560.0 - right)).abs() <= 1.0);
        // Moving the window keeps the view.
        let placed = (canvas.tab.scale, canvas.tab.offset);
        canvas.set_sizing_cover([20, 20, 250, 400]);
        canvas.set_area([0, 0, 800, 600]);
        assert_eq!((canvas.tab.scale, canvas.tab.offset), placed);
        // The right edge, dragged 50 document pixels further right.
        let edge = canvas.to_client([200.0, 50.0]);
        assert_eq!(canvas.sizing_grip_at(edge), Some(Grip::Scale([1, 0])));
        let to = canvas.to_client([250.0, 60.0]);
        canvas.apply(CanvasInput::Begin(
            Gesture::Draw,
            PointerKind::Mouse,
            sample(edge),
        ));
        canvas.apply(CanvasInput::Extend(sample(to)));
        canvas.apply(CanvasInput::End(sample(to)));
        assert_eq!(canvas.sizing().unwrap().frame(), [0, 0, 250, 100]);
        // Esc during a drag puts the frame back and keeps sizing.
        let inside = canvas.to_client([100.0, 50.0]);
        canvas.apply(CanvasInput::Begin(
            Gesture::Draw,
            PointerKind::Mouse,
            sample(inside),
        ));
        canvas.apply(CanvasInput::Extend(sample(canvas.to_client([90.0, 40.0]))));
        canvas.escape();
        assert_eq!(canvas.sizing().unwrap().frame(), [0, 0, 250, 100]);
        assert!(matches!(canvas.apply_sizing(), Ok(Outcome::Committed(_))));
        assert!(canvas.sizing().is_none());
        assert_eq!(canvas.session().document().canvas, [250, 100]);
        // Nothing was drawn while the frame was dragged.
        assert_eq!(canvas.session().undo_label(), Some("Canvas size"));
        // Cancelling leaves the canvas and puts the view back.
        canvas.zoom_to(2.0);
        let view = (canvas.tab.scale, canvas.tab.offset);
        canvas.begin_sizing();
        canvas.sizing_mut().unwrap().set_size([10, 10]);
        canvas.escape();
        assert!(canvas.sizing().is_none());
        assert_eq!(canvas.session().document().canvas, [250, 100]);
        assert_eq!((canvas.tab.scale, canvas.tab.offset), view);
    }

    #[test]
    fn closing_the_tab_or_starting_another_edit_ends_sizing() {
        let mut canvas = Canvas::new(Document::new([64, 64]), |_| {});
        canvas.open_tab(Document::new([32, 32]), true).unwrap();
        canvas.begin_sizing();
        canvas.close_tab(canvas.tab_position(), Document::new([8, 8]));
        assert!(canvas.sizing().is_none());
        assert_eq!(canvas.session().document().canvas, [64, 64]);
        // A transform would be applied by the canvas size, a step of its own.
        canvas.begin_sizing();
        canvas.edit(Session::select_all).unwrap();
        assert!(canvas.sizing().is_some());
        canvas.begin_transform();
        assert!(canvas.session().pending().is_some());
        assert!(canvas.sizing().is_none());
        canvas.cancel_transform();
        canvas.begin_sizing();
        canvas.toggle_playback();
        assert!(canvas.sizing().is_none());
    }

    #[test]
    fn a_canvas_changed_another_way_ends_sizing() {
        let mut canvas = Canvas::new(Document::new([64, 64]), |_| {});
        canvas.begin_sizing();
        canvas.sizing_mut().unwrap().set_size([80, 80]);
        canvas
            .edit(|session| session.crop_canvas([0, 0], [32, 32]))
            .unwrap();
        assert!(canvas.sizing().is_none());
        canvas.begin_sizing();
        canvas.edit(Session::undo).unwrap();
        assert!(canvas.sizing().is_none());
    }
}

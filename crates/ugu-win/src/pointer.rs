// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! Pointer input through `WM_POINTER*` for mouse, pen and touch alike.
//!
//! Mouse is routed into the same path with `EnableMouseInPointer`, so the code
//! that pens rely on (history, capture, release outside the window) runs with
//! every device. The window is subclassed rather than hooked in the message
//! loop because `WM_POINTERCAPTURECHANGED` is sent, not posted.
//!
//! Windows keeps a coalesced history for pen and touch frames but not for the
//! mouse, whose moves between two retrieved messages are lost. Those are
//! recovered from `GetMouseMovePointsEx`.

use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt;

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::ScreenToClient;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GMMP_USE_DISPLAY_POINTS, GetMouseMovePointsEx, MOUSEMOVEPOINT,
};
use windows_sys::Win32::UI::Input::Pointer::{
    EnableMouseInPointer, GetPointerDeviceRects, GetPointerInfo, GetPointerInfoHistory,
    GetPointerPenInfoHistory, POINTER_FLAG_CANCELED, POINTER_FLAG_FIFTHBUTTON,
    POINTER_FLAG_FIRSTBUTTON, POINTER_FLAG_FOURTHBUTTON, POINTER_FLAG_HWHEEL,
    POINTER_FLAG_INCONTACT, POINTER_FLAG_SECONDBUTTON, POINTER_FLAG_THIRDBUTTON, POINTER_INFO,
    POINTER_PEN_INFO,
};
use windows_sys::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    PEN_FLAG_BARREL, PEN_FLAG_ERASER, PEN_FLAG_INVERTED, PEN_MASK_PRESSURE, PEN_MASK_ROTATION,
    PEN_MASK_TILT_X, PEN_MASK_TILT_Y, PT_MOUSE, PT_PEN, PT_TOUCH, PT_TOUCHPAD, WM_NCDESTROY,
    WM_POINTERCAPTURECHANGED, WM_POINTERDOWN, WM_POINTERHWHEEL, WM_POINTERLEAVE, WM_POINTERUP,
    WM_POINTERUPDATE, WM_POINTERWHEEL,
};

use crate::clock::Ticks;

const SUBCLASS_ID: usize = 0x5547_5550;
const PEN_PRESSURE_MAX: f32 = 1024.0;
/// Capacity of the system mouse move buffer.
const MOUSE_MOVE_BUFFER: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PointerId(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PointerKind {
    Mouse,
    Pen,
    Touch,
    Touchpad,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Buttons(u8);

impl Buttons {
    pub const PRIMARY: Self = Self(1);
    pub const SECONDARY: Self = Self(1 << 1);
    pub const MIDDLE: Self = Self(1 << 2);
    pub const X1: Self = Self(1 << 3);
    pub const X2: Self = Self(1 << 4);
    /// Pen barrel button.
    pub const BARREL: Self = Self(1 << 5);
    /// Pen eraser end pressed against the surface.
    pub const ERASER: Self = Self(1 << 6);

    pub const fn empty() -> Self {
        Self(0)
    }

    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    pub const fn difference(self, other: Self) -> Self {
        Self(self.0 & !other.0)
    }
}

/// One input frame of one pointer, in client-area physical pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PointerSample {
    pub position: [f64; 2],
    /// Normalised to 0..=1; `None` when the device does not report pressure.
    pub pressure: Option<f32>,
    /// Degrees, -90..=90 per axis.
    pub tilt: Option<[f32; 2]>,
    /// Degrees, 0..360.
    pub twist: Option<f32>,
    pub buttons: Buttons,
    pub in_contact: bool,
    /// Pen held with the eraser end towards the screen, touching or not.
    pub inverted: bool,
    pub time: Ticks,
    pub frame_id: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PointerEvent {
    /// Chronological samples taken from one message and its coalesced history.
    Samples {
        pointer: PointerId,
        kind: PointerKind,
        samples: Vec<PointerSample>,
    },
    /// The pointer left the client area or went out of range.
    Leave { pointer: PointerId },
    /// The window lost capture; any interaction of this pointer must end now.
    CaptureLost { pointer: PointerId },
    Wheel {
        position: [f64; 2],
        /// Wheel notches; positive is away from the user or to the right.
        delta: f32,
        horizontal: bool,
        time: Ticks,
    },
}

#[derive(Debug)]
pub struct PointerError(&'static str);

impl fmt::Display for PointerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

impl std::error::Error for PointerError {}

/// Routes the mouse through `WM_POINTER*` for the whole process.
///
/// Must run before any window is created; Windows allows it once per process.
pub fn enable_mouse_in_pointer() -> Result<(), PointerError> {
    // SAFETY: plain call without pointers.
    if unsafe { EnableMouseInPointer(1) } == 0 {
        return Err(PointerError("EnableMouseInPointer failed"));
    }
    Ok(())
}

/// Collects pointer events of one window until dropped.
pub struct PointerInput {
    hwnd: HWND,
    shared: Box<Shared>,
}

struct Shared {
    events: RefCell<Vec<PointerEvent>>,
    last_keys: RefCell<HashMap<u32, SampleKey>>,
    last_mouse: RefCell<HashMap<u32, LastMouse>>,
}

/// The newest delivered mouse frame, where recovery of skipped moves stops.
#[derive(Clone, Copy)]
struct LastMouse {
    /// The move buffer entry matched for that frame, if any.
    entry: Option<MovePoint>,
    time_ms: u32,
    sample: PointerSample,
}

impl PointerInput {
    /// # Safety
    /// `hwnd` must be a live window owned by the calling thread, which must
    /// also be the thread that drops the returned value.
    pub unsafe fn install(hwnd: isize) -> Result<Self, PointerError> {
        let hwnd = hwnd as HWND;
        let shared = Box::new(Shared {
            events: RefCell::new(Vec::new()),
            last_keys: RefCell::new(HashMap::new()),
            last_mouse: RefCell::new(HashMap::new()),
        });
        let data = &*shared as *const Shared as usize;
        // SAFETY: `shared` is boxed and outlives the subclass, which `Drop` removes.
        if unsafe { SetWindowSubclass(hwnd, Some(subclass_proc), SUBCLASS_ID, data) } == 0 {
            return Err(PointerError("SetWindowSubclass failed"));
        }
        Ok(Self { hwnd, shared })
    }

    pub fn drain(&self) -> Vec<PointerEvent> {
        std::mem::take(&mut *self.shared.events.borrow_mut())
    }
}

impl Drop for PointerInput {
    fn drop(&mut self) {
        // SAFETY: removing a subclass from a destroyed window fails harmlessly.
        unsafe { RemoveWindowSubclass(self.hwnd, Some(subclass_proc), SUBCLASS_ID) };
    }
}

unsafe extern "system" fn subclass_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    data: usize,
) -> LRESULT {
    // SAFETY: `data` is the `Shared` installed with this subclass and stays
    // alive until `RemoveWindowSubclass` runs.
    let shared = unsafe { &*(data as *const Shared) };
    let pointer_id = (wparam & 0xffff) as u32;
    let handled = match message {
        WM_POINTERDOWN | WM_POINTERUPDATE | WM_POINTERUP => {
            // SAFETY: called on the window's thread for a pointer message.
            unsafe { shared.read_frames(hwnd, pointer_id) }
        }
        WM_POINTERLEAVE => {
            shared.push(PointerEvent::Leave {
                pointer: PointerId(pointer_id),
            });
            true
        }
        WM_POINTERCAPTURECHANGED => {
            shared.forget(pointer_id);
            shared.push(PointerEvent::CaptureLost {
                pointer: PointerId(pointer_id),
            });
            true
        }
        WM_POINTERWHEEL | WM_POINTERHWHEEL => {
            // SAFETY: called on the window's thread for a pointer message.
            unsafe { shared.read_wheel(hwnd, message, wparam, lparam, pointer_id) }
        }
        WM_NCDESTROY => {
            // SAFETY: removal during WM_NCDESTROY is the documented clean-up.
            unsafe { RemoveWindowSubclass(hwnd, Some(subclass_proc), SUBCLASS_ID) };
            false
        }
        _ => false,
    };
    if handled {
        0
    } else {
        // SAFETY: forwards the original arguments down the subclass chain.
        unsafe { DefSubclassProc(hwnd, message, wparam, lparam) }
    }
}

impl Shared {
    fn push(&self, event: PointerEvent) {
        self.events.borrow_mut().push(event);
    }

    /// Chronological mouse samples: the moves skipped since the last frame,
    /// then this frame. Skipped moves have millisecond timing, so several may
    /// share a time; their order comes from the move buffer, not from sorting.
    unsafe fn mouse_samples(
        &self,
        hwnd: HWND,
        info: &POINTER_INFO,
        pointer_id: u32,
        mut newest_first: Vec<PointerSample>,
    ) -> Vec<PointerSample> {
        let current = MovePoint {
            x: info.ptPixelLocation.x,
            y: info.ptPixelLocation.y,
            time_ms: info.dwTime,
        };
        // SAFETY: plain query of the system move buffer.
        let buffer = unsafe { mouse_move_buffer(current) };
        let last = self.last_mouse.borrow().get(&pointer_id).copied();
        self.last_mouse.borrow_mut().insert(
            pointer_id,
            LastMouse {
                entry: buffer.as_ref().map(|buffer| buffer[0]),
                time_ms: current.time_ms,
                sample: newest_first[0],
            },
        );
        if let (Some(buffer), Some(last)) = (buffer.as_deref(), last) {
            // SAFETY: forwarded from the caller.
            newest_first.extend(unsafe { recover_mouse_moves(hwnd, info, current, buffer, last) });
        }
        newest_first.reverse();
        newest_first
    }

    fn forget(&self, pointer_id: u32) {
        self.last_keys.borrow_mut().remove(&pointer_id);
        self.last_mouse.borrow_mut().remove(&pointer_id);
    }

    unsafe fn read_frames(&self, hwnd: HWND, pointer_id: u32) -> bool {
        let mut info = POINTER_INFO::default();
        // SAFETY: valid out pointer.
        if unsafe { GetPointerInfo(pointer_id, &mut info) } == 0 {
            return false;
        }
        let Some(kind) = pointer_kind(info.pointerType) else {
            return false;
        };
        let count = info.historyCount.max(1);
        // SAFETY: called on the window's thread while handling the message.
        let raw = unsafe { read_history(hwnd, pointer_id, kind, count) };
        if raw.is_empty() {
            return false;
        }
        let samples = if kind == PointerKind::Mouse {
            // SAFETY: called on the window's thread while handling the message.
            unsafe { self.mouse_samples(hwnd, &info, pointer_id, raw) }
        } else {
            let last = self.last_keys.borrow().get(&pointer_id).copied();
            let samples = chronological_after(raw, last);
            if let Some(newest) = samples.last() {
                self.last_keys
                    .borrow_mut()
                    .insert(pointer_id, SampleKey::of(newest));
            }
            samples
        };
        if info.pointerFlags & POINTER_FLAG_CANCELED != 0 {
            self.forget(pointer_id);
        }
        if !samples.is_empty() {
            self.push(PointerEvent::Samples {
                pointer: PointerId(pointer_id),
                kind,
                samples,
            });
        }
        true
    }

    unsafe fn read_wheel(
        &self,
        hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
        pointer_id: u32,
    ) -> bool {
        let mut info = POINTER_INFO::default();
        // SAFETY: valid out pointer.
        let time = if unsafe { GetPointerInfo(pointer_id, &mut info) } != 0 {
            sample_time(&info)
        } else {
            Ticks::now()
        };
        let mut point = POINT {
            x: (lparam & 0xffff) as i16 as i32,
            y: ((lparam >> 16) & 0xffff) as i16 as i32,
        };
        // SAFETY: valid window and out pointer.
        if unsafe { ScreenToClient(hwnd, &mut point) } == 0 {
            return false;
        }
        let horizontal =
            message == WM_POINTERHWHEEL || info.pointerFlags & POINTER_FLAG_HWHEEL != 0;
        let raw_delta = ((wparam >> 16) & 0xffff) as i16;
        self.push(PointerEvent::Wheel {
            position: [point.x as f64, point.y as f64],
            delta: raw_delta as f32 / 120.0,
            horizontal,
            time,
        });
        true
    }
}

fn pointer_kind(pointer_type: i32) -> Option<PointerKind> {
    match pointer_type {
        PT_MOUSE => Some(PointerKind::Mouse),
        PT_PEN => Some(PointerKind::Pen),
        PT_TOUCH => Some(PointerKind::Touch),
        PT_TOUCHPAD => Some(PointerKind::Touchpad),
        _ => None,
    }
}

/// Returns history entries newest first, as Windows does.
unsafe fn read_history(
    hwnd: HWND,
    pointer_id: u32,
    kind: PointerKind,
    count: u32,
) -> Vec<PointerSample> {
    let mut entries = count;
    if kind == PointerKind::Pen {
        let mut pens = vec![POINTER_PEN_INFO::default(); entries as usize];
        // SAFETY: the buffer holds `entries` elements.
        if unsafe { GetPointerPenInfoHistory(pointer_id, &mut entries, pens.as_mut_ptr()) } == 0 {
            return Vec::new();
        }
        pens.truncate(entries as usize);
        pens.iter()
            // SAFETY: same window and thread as the caller.
            .filter_map(|pen| unsafe { pen_sample(hwnd, pen) })
            .collect()
    } else {
        let mut infos = vec![POINTER_INFO::default(); entries as usize];
        // SAFETY: the buffer holds `entries` elements.
        if unsafe { GetPointerInfoHistory(pointer_id, &mut entries, infos.as_mut_ptr()) } == 0 {
            return Vec::new();
        }
        infos.truncate(entries as usize);
        infos
            .iter()
            // SAFETY: same window and thread as the caller.
            .filter_map(|info| unsafe { base_sample(hwnd, info) })
            .collect()
    }
}

unsafe fn base_sample(hwnd: HWND, info: &POINTER_INFO) -> Option<PointerSample> {
    // SAFETY: forwarded from the caller.
    let position = unsafe { client_position(hwnd, info) }?;
    let flags = info.pointerFlags;
    let mut buttons = Buttons::empty();
    for (flag, button) in [
        (POINTER_FLAG_FIRSTBUTTON, Buttons::PRIMARY),
        (POINTER_FLAG_SECONDBUTTON, Buttons::SECONDARY),
        (POINTER_FLAG_THIRDBUTTON, Buttons::MIDDLE),
        (POINTER_FLAG_FOURTHBUTTON, Buttons::X1),
        (POINTER_FLAG_FIFTHBUTTON, Buttons::X2),
    ] {
        if flags & flag != 0 {
            buttons = buttons.union(button);
        }
    }
    Some(PointerSample {
        position,
        pressure: None,
        tilt: None,
        twist: None,
        buttons,
        in_contact: flags & POINTER_FLAG_INCONTACT != 0,
        inverted: false,
        time: sample_time(info),
        frame_id: info.frameId,
    })
}

unsafe fn pen_sample(hwnd: HWND, pen: &POINTER_PEN_INFO) -> Option<PointerSample> {
    // SAFETY: forwarded from the caller.
    let mut sample = unsafe { base_sample(hwnd, &pen.pointerInfo) }?;
    if pen.penMask & PEN_MASK_PRESSURE != 0 {
        sample.pressure = Some((pen.pressure as f32 / PEN_PRESSURE_MAX).clamp(0.0, 1.0));
    }
    if pen.penMask & (PEN_MASK_TILT_X | PEN_MASK_TILT_Y) != 0 {
        sample.tilt = Some([pen.tiltX as f32, pen.tiltY as f32]);
    }
    if pen.penMask & PEN_MASK_ROTATION != 0 {
        sample.twist = Some(pen.rotation as f32);
    }
    if pen.penFlags & PEN_FLAG_BARREL != 0 {
        sample.buttons = sample.buttons.union(Buttons::BARREL);
    }
    if pen.penFlags & PEN_FLAG_ERASER != 0 {
        sample.buttons = sample.buttons.union(Buttons::ERASER);
    }
    sample.inverted = pen.penFlags & (PEN_FLAG_INVERTED | PEN_FLAG_ERASER) != 0;
    Some(sample)
}

/// The system move buffer, newest first, starting at the entry for `current`.
///
/// Absolute devices, injected input among them, round the buffer entry and the
/// cursor position separately, so the entry may sit one pixel away.
unsafe fn mouse_move_buffer(current: MovePoint) -> Option<Vec<MovePoint>> {
    let mut buffer = [MOUSEMOVEPOINT::default(); MOUSE_MOVE_BUFFER];
    for (dx, dy) in [
        (0, 0),
        (1, 0),
        (0, 1),
        (1, 1),
        (-1, 0),
        (0, -1),
        (-1, -1),
        (1, -1),
        (-1, 1),
    ] {
        let query = MOUSEMOVEPOINT {
            x: (current.x + dx) & 0xffff,
            y: (current.y + dy) & 0xffff,
            time: current.time_ms,
            dwExtraInfo: 0,
        };
        // SAFETY: the buffer holds MOUSE_MOVE_BUFFER elements.
        let found = unsafe {
            GetMouseMovePointsEx(
                size_of::<MOUSEMOVEPOINT>() as u32,
                &query,
                buffer.as_mut_ptr(),
                MOUSE_MOVE_BUFFER as i32,
                GMMP_USE_DISPLAY_POINTS,
            )
        };
        if found > 0 {
            return Some(
                buffer[..found as usize]
                    .iter()
                    .map(MovePoint::from_display)
                    .collect(),
            );
        }
    }
    None
}

/// Mouse moves the system coalesced since `last`, newest first as in `read_history`.
///
/// They carry the button state of `last`: a button change always arrives as
/// its own message, so no skipped move can follow one.
unsafe fn recover_mouse_moves(
    hwnd: HWND,
    info: &POINTER_INFO,
    current: MovePoint,
    buffer: &[MovePoint],
    last: LastMouse,
) -> Vec<PointerSample> {
    let mut origin = POINT { x: 0, y: 0 };
    // SAFETY: valid window and out pointer.
    if unsafe { ScreenToClient(hwnd, &mut origin) } == 0 {
        return Vec::new();
    }
    // Moves the buffer rounding offset of this frame onto the cursor position.
    let shift = [
        origin.x + current.x - buffer[0].x,
        origin.y + current.y - buffer[0].y,
    ];
    let now = sample_time(info);
    skipped_moves(buffer, last.entry, last.time_ms)
        .into_iter()
        .map(|entry| PointerSample {
            position: [(entry.x + shift[0]) as f64, (entry.y + shift[1]) as f64],
            time: now.minus_millis(current.time_ms.wrapping_sub(entry.time_ms)),
            frame_id: info.frameId,
            ..last.sample
        })
        .collect()
}

/// A mouse position in virtual-screen pixels with the millisecond input time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct MovePoint {
    x: i32,
    y: i32,
    time_ms: u32,
}

impl MovePoint {
    /// Display points are 16-bit; monitors left of or above the primary one
    /// produce values that wrap.
    fn from_display(point: &MOUSEMOVEPOINT) -> Self {
        Self {
            x: point.x as i16 as i32,
            y: point.y as i16 as i32,
            time_ms: point.time,
        }
    }
}

/// Entries of a newest-first move buffer between its first entry and the
/// last delivered frame, newest first. Without the last frame's entry, only
/// strictly newer times are certain to be new.
fn skipped_moves(
    newest_first: &[MovePoint],
    last_entry: Option<MovePoint>,
    last_time_ms: u32,
) -> Vec<MovePoint> {
    let current = newest_first[0];
    newest_first[1..]
        .iter()
        .copied()
        .take_while(|entry| {
            let age = entry.time_ms.wrapping_sub(last_time_ms) as i32;
            Some(*entry) != last_entry && (age > 0 || (age == 0 && last_entry.is_some()))
        })
        .filter(|entry| *entry != current)
        .collect()
}

fn sample_time(info: &POINTER_INFO) -> Ticks {
    if info.PerformanceCount != 0 {
        Ticks(info.PerformanceCount)
    } else {
        Ticks::now()
    }
}

/// Client position with the sub-pixel precision of the himetric location
/// when the device reports one.
unsafe fn client_position(hwnd: HWND, info: &POINTER_INFO) -> Option<[f64; 2]> {
    let mut screen = [info.ptPixelLocation.x as f64, info.ptPixelLocation.y as f64];
    if info.pointerType != PT_MOUSE {
        let mut device = RECT::default();
        let mut display = RECT::default();
        // SAFETY: valid out pointers.
        if unsafe { GetPointerDeviceRects(info.sourceDevice, &mut device, &mut display) } != 0 {
            let device_width = (device.right - device.left) as f64;
            let device_height = (device.bottom - device.top) as f64;
            if device_width > 0.0 && device_height > 0.0 {
                screen = [
                    display.left as f64
                        + info.ptHimetricLocation.x as f64 * (display.right - display.left) as f64
                            / device_width,
                    display.top as f64
                        + info.ptHimetricLocation.y as f64 * (display.bottom - display.top) as f64
                            / device_height,
                ];
            }
        }
    }
    let mut origin = POINT { x: 0, y: 0 };
    // SAFETY: valid window and out pointer.
    if unsafe { ScreenToClient(hwnd, &mut origin) } == 0 {
        return None;
    }
    Some([screen[0] + origin.x as f64, screen[1] + origin.y as f64])
}

/// Orders samples by time and frame so that no frame is delivered twice.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct SampleKey {
    time: Ticks,
    frame_id: u32,
}

impl SampleKey {
    fn of(sample: &PointerSample) -> Self {
        Self {
            time: sample.time,
            frame_id: sample.frame_id,
        }
    }
}

/// Turns newest-first history into chronological samples newer than `last`.
fn chronological_after(
    mut newest_first: Vec<PointerSample>,
    last: Option<SampleKey>,
) -> Vec<PointerSample> {
    newest_first.reverse();
    let mut samples = newest_first;
    samples.sort_by_key(SampleKey::of);
    samples.dedup_by_key(|sample| SampleKey::of(sample));
    if let Some(last) = last {
        samples.retain(|sample| SampleKey::of(sample) > last);
    }
    samples
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(time: u64, frame_id: u32, x: f64) -> PointerSample {
        PointerSample {
            position: [x, 0.0],
            pressure: None,
            tilt: None,
            twist: None,
            buttons: Buttons::empty(),
            in_contact: true,
            inverted: false,
            time: Ticks(time),
            frame_id,
        }
    }

    fn xs(samples: &[PointerSample]) -> Vec<f64> {
        samples.iter().map(|sample| sample.position[0]).collect()
    }

    #[test]
    fn history_is_returned_oldest_first() {
        let history = vec![sample(30, 3, 3.0), sample(20, 2, 2.0), sample(10, 1, 1.0)];
        assert_eq!(xs(&chronological_after(history, None)), [1.0, 2.0, 3.0]);
    }

    #[test]
    fn frames_already_delivered_are_dropped() {
        let last = SampleKey::of(&sample(20, 2, 2.0));
        let history = vec![sample(40, 4, 4.0), sample(30, 3, 3.0), sample(20, 2, 2.0)];
        assert_eq!(xs(&chronological_after(history, Some(last))), [3.0, 4.0]);
    }

    #[test]
    fn duplicate_frames_collapse() {
        let history = vec![sample(20, 2, 2.0), sample(20, 2, 2.0), sample(10, 1, 1.0)];
        assert_eq!(xs(&chronological_after(history, None)), [1.0, 2.0]);
    }

    fn point(x: i32, time_ms: u32) -> MovePoint {
        MovePoint { x, y: 0, time_ms }
    }

    #[test]
    fn skipped_moves_stop_at_the_last_delivered_move() {
        let buffer = [
            point(5, 50),
            point(4, 40),
            point(3, 30),
            point(2, 20),
            point(1, 10),
        ];
        assert_eq!(
            skipped_moves(&buffer, Some(point(2, 20)), 20),
            [point(4, 40), point(3, 30)]
        );
    }

    #[test]
    fn skipped_moves_keep_moves_in_the_same_millisecond_before_the_last_entry() {
        let buffer = [point(5, 21), point(4, 20), point(3, 20), point(2, 20)];
        assert_eq!(
            skipped_moves(&buffer, Some(point(3, 20)), 20),
            [point(4, 20)]
        );
    }

    #[test]
    fn skipped_moves_without_the_last_entry_take_only_newer_times() {
        let buffer = [point(5, 50), point(4, 40), point(3, 20), point(1, 10)];
        assert_eq!(skipped_moves(&buffer, None, 20), [point(4, 40)]);
    }

    #[test]
    fn skipped_moves_stop_at_older_times_when_the_last_entry_is_gone() {
        let buffer = [point(5, 50), point(4, 40), point(1, 10)];
        assert_eq!(
            skipped_moves(&buffer, Some(point(2, 20)), 20),
            [point(4, 40)]
        );
    }

    #[test]
    fn skipped_moves_survive_the_millisecond_clock_wrapping() {
        let buffer = [point(3, 5), point(2, u32::MAX - 5), point(1, u32::MAX - 20)];
        assert_eq!(
            skipped_moves(&buffer, Some(point(1, u32::MAX - 20)), u32::MAX - 20),
            [point(2, u32::MAX - 5)]
        );
    }

    #[test]
    fn display_points_left_of_the_primary_monitor_are_negative() {
        let raw = MOUSEMOVEPOINT {
            x: 65536 - 2560,
            y: 235,
            time: 1,
            dwExtraInfo: 0,
        };
        assert_eq!(MovePoint::from_display(&raw).x, -2560);
    }

    #[test]
    fn equal_times_keep_frame_order() {
        let history = vec![sample(10, 6, 6.0), sample(10, 5, 5.0)];
        assert_eq!(xs(&chronological_after(history, None)), [5.0, 6.0]);
    }
}

// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 NewfolderGames

//! Wayland graphics tablet input (`zwp_tablet_v2` and `wp_cursor_shape_v1`) and
//! the shared Linux pen event queue.
//!
//! `zwp_tablet_tool_v2` delivers axis and button updates sequentially and marks
//! each hardware report with `Event::Frame { time }` on `CLOCK_MONOTONIC`
//! milliseconds. [`ToolTracker`] accumulates updates per tool and flushes one
//! [`PointerSample`] per frame in client-area physical pixels.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use ugu_platform_types::clock::Ticks;
use ugu_platform_types::pointer::{Buttons, PointerEvent, PointerId, PointerKind, PointerSample};
use wayland_backend::client::ObjectId;
use wayland_client::protocol::wl_seat::WlSeat;
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle, WEnum};
use wayland_protocols::wp::cursor_shape::v1::client::{
    wp_cursor_shape_device_v1::{self, Shape, WpCursorShapeDeviceV1},
    wp_cursor_shape_manager_v1::WpCursorShapeManagerV1,
};
use wayland_protocols::wp::tablet::zv2::client::{
    zwp_tablet_manager_v2::ZwpTabletManagerV2,
    zwp_tablet_pad_group_v2::{self, ZwpTabletPadGroupV2},
    zwp_tablet_pad_ring_v2::ZwpTabletPadRingV2,
    zwp_tablet_pad_strip_v2::ZwpTabletPadStripV2,
    zwp_tablet_pad_v2::{self, ZwpTabletPadV2},
    zwp_tablet_seat_v2::{self, ZwpTabletSeatV2},
    zwp_tablet_tool_v2::{self, ButtonState, Capability, Type as ToolType, ZwpTabletToolV2},
    zwp_tablet_v2::{self, ZwpTabletV2},
};

use super::clock;
use super::window::WaylandState;

/// Keeps Linux pen pointer IDs disjoint from `WinitPointer`'s mouse (`0`) and
/// low-numbered touch IDs (`1 + touch.id`).
pub(crate) const PEN_POINTER_BASE: u32 = 0x4000_0000;

/// Linux `input-event-codes.h` button codes emitted by `zwp_tablet_tool_v2`.
pub(crate) const BTN_RIGHT: u32 = 0x111;
pub(crate) const BTN_MIDDLE: u32 = 0x112;
pub(crate) const BTN_TOOL_RUBBER: u32 = 0x141;
pub(crate) const BTN_STYLUS: u32 = 0x14b;
pub(crate) const BTN_STYLUS2: u32 = 0x14c;

/// Maximum idle gap on XInput2 (which lacks an XI2 `ProximityOut` event when a
/// stylus lifts straight up above the window) before mouse suppression expires
/// while no stylus button or tip is held.
const X11_IDLE_PROXIMITY_TIMEOUT: Duration = Duration::from_millis(150);

/// Thread-safe queue of [`PointerEvent`]s and proximity state shared between a
/// Wayland or X11 tablet worker thread and the UI thread.
pub(crate) struct PenQueue {
    inner: Mutex<PenQueueInner>,
    wake: Arc<dyn Fn() + Send + Sync>,
}

struct PenQueueInner {
    events: VecDeque<PointerEvent>,
    wayland_proximity_count: usize,
    x11_active: Option<X11Proximity>,
}

#[derive(Clone, Copy)]
struct X11Proximity {
    pointer: PointerId,
    in_contact_or_held: bool,
    last_event: Instant,
}

impl PenQueue {
    pub(crate) fn new(wake: Arc<dyn Fn() + Send + Sync>) -> Arc<Self> {
        Arc::new(Self {
            inner: Mutex::new(PenQueueInner {
                events: VecDeque::new(),
                wayland_proximity_count: 0,
                x11_active: None,
            }),
            wake,
        })
    }

    pub(crate) fn push_wayland(&self, batch: Vec<PointerEvent>, proximity_count: usize) {
        let should_wake = {
            let mut inner = self.inner.lock().expect("pen queue poisoned");
            inner.wayland_proximity_count = proximity_count;
            let non_empty = !batch.is_empty();
            for event in batch {
                coalesce_push(&mut inner.events, event);
            }
            non_empty
        };
        if should_wake {
            (self.wake)();
        }
    }

    pub(crate) fn set_wayland_proximity(&self, proximity_count: usize) {
        let mut inner = self.inner.lock().expect("pen queue poisoned");
        inner.wayland_proximity_count = proximity_count;
    }

    pub(crate) fn push_x11(
        &self,
        pointer: PointerId,
        event: PointerEvent,
        in_contact_or_held: bool,
        now: Instant,
    ) {
        {
            let mut inner = self.inner.lock().expect("pen queue poisoned");
            inner.x11_active = Some(X11Proximity {
                pointer,
                in_contact_or_held,
                last_event: now,
            });
            coalesce_push(&mut inner.events, event);
        }
        (self.wake)();
    }

    pub(crate) fn leave_x11(&self, pointer: PointerId) {
        let should_wake = {
            let mut inner = self.inner.lock().expect("pen queue poisoned");
            let was_active = inner.x11_active.is_some_and(|st| st.pointer == pointer);
            if was_active {
                inner.x11_active = None;
                coalesce_push(&mut inner.events, PointerEvent::Leave { pointer });
            }
            was_active
        };
        if should_wake {
            (self.wake)();
        }
    }

    pub(crate) fn clear_x11_from_non_pen(&self) {
        let should_wake = {
            let mut inner = self.inner.lock().expect("pen queue poisoned");
            if let Some(active) = inner.x11_active.take() {
                coalesce_push(
                    &mut inner.events,
                    PointerEvent::Leave {
                        pointer: active.pointer,
                    },
                );
                true
            } else {
                false
            }
        };
        if should_wake {
            (self.wake)();
        }
    }

    pub(crate) fn in_proximity(&self) -> bool {
        let mut inner = self.inner.lock().expect("pen queue poisoned");
        if inner.wayland_proximity_count > 0 {
            return true;
        }
        if let Some(active) = inner.x11_active {
            if active.in_contact_or_held
                || active.last_event.elapsed() <= X11_IDLE_PROXIMITY_TIMEOUT
            {
                return true;
            }
            inner.x11_active = None;
        }
        false
    }

    pub(crate) fn drain(&self) -> Vec<PointerEvent> {
        let mut inner = self.inner.lock().expect("pen queue poisoned");
        inner.events.drain(..).collect()
    }
}

fn coalesce_push(queue: &mut VecDeque<PointerEvent>, event: PointerEvent) {
    if let PointerEvent::Samples {
        pointer,
        kind: PointerKind::Pen,
        samples,
    } = event
    {
        if samples.is_empty() {
            return;
        }
        if let Some(PointerEvent::Samples {
            pointer: tail_ptr,
            kind: PointerKind::Pen,
            samples: tail_samples,
        }) = queue.back_mut()
            && *tail_ptr == pointer
        {
            tail_samples.extend(samples);
            return;
        }
        queue.push_back(PointerEvent::Samples {
            pointer,
            kind: PointerKind::Pen,
            samples,
        });
    } else {
        queue.push_back(event);
    }
}

/// Maps pen tip contact, barrel buttons, and eraser state to [`Buttons`] and
/// `in_contact`, matching Windows `WM_POINTER` (`crates/ugu-win/src/pointer.rs`).
pub(crate) fn pen_buttons(
    tip_down: bool,
    barrel_held: bool,
    stylus2_held: bool,
    inverted: bool,
) -> (Buttons, bool) {
    let mut buttons = Buttons::empty();
    if tip_down {
        if barrel_held {
            buttons = buttons.union(Buttons::SECONDARY).union(Buttons::BARREL);
        } else {
            buttons = buttons.union(Buttons::PRIMARY);
        }
        if inverted {
            buttons = buttons.union(Buttons::ERASER);
        }
    } else if barrel_held {
        buttons = buttons.union(Buttons::BARREL);
    }
    if stylus2_held {
        buttons = buttons.union(Buttons::MIDDLE);
    }
    (buttons, tip_down)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ToolClass {
    Pen,
    Eraser,
    NonPen,
}

impl ToolClass {
    fn from_wayland(kind: ToolType) -> Self {
        match kind {
            ToolType::Eraser => Self::Eraser,
            ToolType::Mouse | ToolType::Lens => Self::NonPen,
            _ => Self::Pen,
        }
    }
}

/// Pure per-tool `zwp_tablet_tool_v2` frame accumulator.
pub(crate) struct ToolTracker {
    pointer: PointerId,
    class: ToolClass,
    has_pressure: bool,
    has_tilt: bool,
    has_rotation: bool,
    in_proximity: bool,
    proximity_serial: u32,
    position_logical: Option<[f64; 2]>,
    tip_down: bool,
    barrel_held: bool,
    stylus2_held: bool,
    eraser_button_held: bool,
    raw_pressure: Option<u32>,
    tilt_deg: Option<[f32; 2]>,
    twist_deg: Option<f32>,
    frame_counter: u32,
    dirty: bool,
    pending_leave: bool,
    pending_capture_lost: bool,
}

impl ToolTracker {
    pub(crate) fn new(pointer: PointerId) -> Self {
        Self {
            pointer,
            class: ToolClass::Pen,
            has_pressure: false,
            has_tilt: false,
            has_rotation: false,
            in_proximity: false,
            proximity_serial: 0,
            position_logical: None,
            tip_down: false,
            barrel_held: false,
            stylus2_held: false,
            eraser_button_held: false,
            raw_pressure: None,
            tilt_deg: None,
            twist_deg: None,
            frame_counter: 0,
            dirty: false,
            pending_leave: false,
            pending_capture_lost: false,
        }
    }

    pub(crate) fn in_proximity(&self) -> bool {
        self.in_proximity && self.class != ToolClass::NonPen
    }

    pub(crate) fn proximity_serial(&self) -> Option<u32> {
        (self.in_proximity && self.proximity_serial != 0).then_some(self.proximity_serial)
    }

    pub(crate) fn set_type(&mut self, class: ToolClass) {
        self.class = class;
    }

    pub(crate) fn add_capability(&mut self, capability: Capability) {
        match capability {
            Capability::Pressure => self.has_pressure = true,
            Capability::Tilt => self.has_tilt = true,
            Capability::Rotation => self.has_rotation = true,
            _ => {}
        }
    }

    pub(crate) fn proximity_in(&mut self, serial: u32, on_our_surface: bool) {
        if !on_our_surface {
            if self.in_proximity {
                self.proximity_out();
            }
            return;
        }
        self.in_proximity = true;
        self.proximity_serial = serial;
        self.pending_leave = false;
        self.pending_capture_lost = false;
        self.dirty = true;
    }

    pub(crate) fn proximity_out(&mut self) {
        if !self.in_proximity {
            return;
        }
        self.in_proximity = false;
        self.proximity_serial = 0;
        // If focus moved to another surface while a stroke or button was still
        // held without a preceding `Up` / `Button(Released)` in this frame,
        // cancel the gesture cleanly before leaving.
        if self.tip_down || self.barrel_held || self.stylus2_held {
            self.tip_down = false;
            self.barrel_held = false;
            self.stylus2_held = false;
            self.eraser_button_held = false;
            self.raw_pressure = None;
            self.pending_capture_lost = true;
            self.dirty = false;
        }
        self.pending_leave = true;
    }

    pub(crate) fn down(&mut self) {
        if self.in_proximity {
            self.tip_down = true;
            self.dirty = true;
        }
    }

    pub(crate) fn up(&mut self) {
        if self.in_proximity || self.tip_down {
            self.tip_down = false;
            self.dirty = true;
        }
    }

    pub(crate) fn motion(&mut self, x: f64, y: f64) {
        self.position_logical = Some([x, y]);
        if self.in_proximity {
            self.dirty = true;
        }
    }

    pub(crate) fn pressure(&mut self, value: u32) {
        self.has_pressure = true;
        self.raw_pressure = Some(value.min(65_535));
        if self.in_proximity {
            self.dirty = true;
        }
    }

    pub(crate) fn tilt(&mut self, tilt_x: f64, tilt_y: f64) {
        self.has_tilt = true;
        self.tilt_deg = Some([
            (tilt_x as f32).clamp(-90.0, 90.0),
            (tilt_y as f32).clamp(-90.0, 90.0),
        ]);
        if self.in_proximity {
            self.dirty = true;
        }
    }

    pub(crate) fn rotation(&mut self, degrees: f64) {
        self.has_rotation = true;
        self.twist_deg = Some((degrees as f32).rem_euclid(360.0));
        if self.in_proximity {
            self.dirty = true;
        }
    }

    pub(crate) fn button(&mut self, code: u32, pressed: bool) {
        let changed = match code {
            BTN_STYLUS | BTN_RIGHT => {
                self.barrel_held = pressed;
                true
            }
            BTN_STYLUS2 | BTN_MIDDLE => {
                self.stylus2_held = pressed;
                true
            }
            BTN_TOOL_RUBBER => {
                self.eraser_button_held = pressed;
                true
            }
            _ => false,
        };
        if changed && (self.in_proximity || !pressed) {
            self.dirty = true;
        }
    }

    pub(crate) fn frame(&mut self, time: Ticks, scale_factor: f64) -> Vec<PointerEvent> {
        if self.class == ToolClass::NonPen {
            self.dirty = false;
            self.pending_leave = false;
            self.pending_capture_lost = false;
            return Vec::new();
        }

        let mut out = Vec::new();
        if self.dirty
            && let Some([lx, ly]) = self.position_logical
        {
            self.dirty = false;
            self.frame_counter = self.frame_counter.wrapping_add(1);
            let scale = if scale_factor > 0.0 {
                scale_factor
            } else {
                1.0
            };
            let inverted = self.class == ToolClass::Eraser || self.eraser_button_held;
            let (buttons, in_contact) =
                pen_buttons(self.tip_down, self.barrel_held, self.stylus2_held, inverted);
            let pressure = if self.has_pressure {
                Some(if self.tip_down {
                    (self.raw_pressure.unwrap_or(65_535) as f32 / 65_535.0).clamp(0.0, 1.0)
                } else {
                    0.0
                })
            } else {
                None
            };
            let tilt = self.has_tilt.then(|| self.tilt_deg.unwrap_or([0.0, 0.0]));
            let twist = self.has_rotation.then(|| self.twist_deg.unwrap_or(0.0));
            out.push(PointerEvent::Samples {
                pointer: self.pointer,
                kind: PointerKind::Pen,
                samples: vec![PointerSample {
                    position: [lx * scale, ly * scale],
                    pressure,
                    tilt,
                    twist,
                    buttons,
                    in_contact,
                    inverted,
                    time,
                    frame_id: self.frame_counter,
                }],
            });
        } else {
            self.dirty = false;
        }

        if self.pending_capture_lost {
            self.pending_capture_lost = false;
            out.push(PointerEvent::CaptureLost {
                pointer: self.pointer,
            });
        }
        if self.pending_leave {
            self.pending_leave = false;
            self.position_logical = None;
            out.push(PointerEvent::Leave {
                pointer: self.pointer,
            });
        }
        out
    }
}

struct LiveTool {
    proxy: ZwpTabletToolV2,
    shape_device: Option<WpCursorShapeDeviceV1>,
    tracker: ToolTracker,
}

/// Wayland tablet protocol objects owned by [`WaylandState`].
pub(crate) struct WaylandTablet {
    pub(crate) queue: Arc<PenQueue>,
    pub(crate) scale_factor: f64,
    pub(crate) cursor_name: Option<&'static str>,
    manager: Option<ZwpTabletManagerV2>,
    cursor_manager: Option<WpCursorShapeManagerV1>,
    tablet_seats: HashMap<ObjectId, ZwpTabletSeatV2>,
    tablets: HashMap<ObjectId, ZwpTabletV2>,
    tools: HashMap<ObjectId, LiveTool>,
    pads: HashMap<ObjectId, ZwpTabletPadV2>,
    pad_groups: Vec<ZwpTabletPadGroupV2>,
    pad_rings: Vec<ZwpTabletPadRingV2>,
    pad_strips: Vec<ZwpTabletPadStripV2>,
    next_tool_id: u32,
}

impl WaylandTablet {
    pub(crate) fn new(queue: Arc<PenQueue>, scale_factor: f64) -> Self {
        Self {
            queue,
            scale_factor: if scale_factor > 0.0 {
                scale_factor
            } else {
                1.0
            },
            cursor_name: Some("default"),
            manager: None,
            cursor_manager: None,
            tablet_seats: HashMap::new(),
            tablets: HashMap::new(),
            tools: HashMap::new(),
            pads: HashMap::new(),
            pad_groups: Vec::new(),
            pad_rings: Vec::new(),
            pad_strips: Vec::new(),
            next_tool_id: 0,
        }
    }

    #[allow(clippy::mutable_key_type)]
    pub(crate) fn bind_global(
        &mut self,
        interface: &str,
        name: u32,
        version: u32,
        registry: &wayland_client::protocol::wl_registry::WlRegistry,
        seats: &HashMap<ObjectId, (WlSeat, super::window::SeatSlot)>,
        qh: &QueueHandle<WaylandState>,
    ) -> bool {
        match interface {
            "zwp_tablet_manager_v2" if self.manager.is_none() => {
                let mgr = registry.bind::<ZwpTabletManagerV2, _, _>(name, version.min(1), qh, ());
                for (seat_id, (seat, _)) in seats {
                    self.tablet_seats
                        .entry(seat_id.clone())
                        .or_insert_with(|| mgr.get_tablet_seat(seat, qh, seat_id.clone()));
                }
                self.manager = Some(mgr);
                true
            }
            "wp_cursor_shape_manager_v1" if self.cursor_manager.is_none() => {
                let cmgr =
                    registry.bind::<WpCursorShapeManagerV1, _, _>(name, version.min(1), qh, ());
                for tool in self.tools.values_mut() {
                    if tool.shape_device.is_none() {
                        tool.shape_device = Some(cmgr.get_tablet_tool_v2(&tool.proxy, qh, ()));
                    }
                }
                self.cursor_manager = Some(cmgr);
                true
            }
            _ => false,
        }
    }

    pub(crate) fn on_seat_added(
        &mut self,
        seat_id: ObjectId,
        seat: &WlSeat,
        qh: &QueueHandle<WaylandState>,
    ) {
        if let Some(mgr) = &self.manager {
            self.tablet_seats
                .entry(seat_id.clone())
                .or_insert_with(|| mgr.get_tablet_seat(seat, qh, seat_id));
        }
    }

    pub(crate) fn set_cursor(&mut self, cursor_name: Option<&'static str>, conn: &Connection) {
        self.cursor_name = cursor_name;
        for tool in self.tools.values() {
            apply_tool_cursor(tool, cursor_name);
        }
        let _ = conn.flush();
    }

    fn proximity_count(&self) -> usize {
        self.tools
            .values()
            .filter(|tool| tool.tracker.in_proximity())
            .count()
    }

    pub(crate) fn cleanup(&mut self) {
        for strip in self.pad_strips.drain(..) {
            strip.destroy();
        }
        for ring in self.pad_rings.drain(..) {
            ring.destroy();
        }
        for group in self.pad_groups.drain(..) {
            group.destroy();
        }
        for (_, pad) in self.pads.drain() {
            pad.destroy();
        }
        for (_, mut tool) in self.tools.drain() {
            if let Some(shape) = tool.shape_device.take() {
                shape.destroy();
            }
            tool.proxy.destroy();
        }
        for (_, tablet) in self.tablets.drain() {
            tablet.destroy();
        }
        for (_, seat) in self.tablet_seats.drain() {
            seat.destroy();
        }
        if let Some(cmgr) = self.cursor_manager.take() {
            cmgr.destroy();
        }
        if let Some(mgr) = self.manager.take() {
            mgr.destroy();
        }
    }
}

fn apply_tool_cursor(tool: &LiveTool, cursor_name: Option<&'static str>) {
    let Some(serial) = tool.tracker.proximity_serial() else {
        return;
    };
    match cursor_name {
        None => {
            tool.proxy.set_cursor(serial, None, 0, 0);
        }
        Some(name) => {
            if let Some(shape_dev) = &tool.shape_device {
                shape_dev.set_shape(serial, css_to_wayland_shape(name));
            }
        }
    }
}

fn css_to_wayland_shape(name: &str) -> Shape {
    match name {
        "context-menu" => Shape::ContextMenu,
        "help" => Shape::Help,
        "pointer" => Shape::Pointer,
        "progress" => Shape::Progress,
        "wait" => Shape::Wait,
        "cell" => Shape::Cell,
        "crosshair" => Shape::Crosshair,
        "text" => Shape::Text,
        "vertical-text" => Shape::VerticalText,
        "alias" => Shape::Alias,
        "copy" => Shape::Copy,
        "move" => Shape::Move,
        "no-drop" => Shape::NoDrop,
        "not-allowed" => Shape::NotAllowed,
        "grab" => Shape::Grab,
        "grabbing" => Shape::Grabbing,
        "e-resize" => Shape::EResize,
        "n-resize" => Shape::NResize,
        "ne-resize" => Shape::NeResize,
        "nw-resize" => Shape::NwResize,
        "s-resize" => Shape::SResize,
        "se-resize" => Shape::SeResize,
        "sw-resize" => Shape::SwResize,
        "w-resize" => Shape::WResize,
        "ew-resize" => Shape::EwResize,
        "ns-resize" => Shape::NsResize,
        "nesw-resize" => Shape::NeswResize,
        "nwse-resize" => Shape::NwseResize,
        "col-resize" => Shape::ColResize,
        "row-resize" => Shape::RowResize,
        "all-scroll" => Shape::AllScroll,
        "zoom-in" => Shape::ZoomIn,
        "zoom-out" => Shape::ZoomOut,
        _ => Shape::Default,
    }
}

impl Dispatch<ZwpTabletManagerV2, ()> for WaylandState {
    fn event(
        _: &mut Self,
        _: &ZwpTabletManagerV2,
        _: <ZwpTabletManagerV2 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<WpCursorShapeManagerV1, ()> for WaylandState {
    fn event(
        _: &mut Self,
        _: &WpCursorShapeManagerV1,
        _: <WpCursorShapeManagerV1 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<WpCursorShapeDeviceV1, ()> for WaylandState {
    fn event(
        _: &mut Self,
        _: &WpCursorShapeDeviceV1,
        _: wp_cursor_shape_device_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZwpTabletSeatV2, ObjectId> for WaylandState {
    fn event(
        state: &mut Self,
        _: &ZwpTabletSeatV2,
        event: zwp_tablet_seat_v2::Event,
        seat_id: &ObjectId,
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        match event {
            zwp_tablet_seat_v2::Event::TabletAdded { id } => {
                state.tablet.tablets.insert(id.id(), id);
            }
            zwp_tablet_seat_v2::Event::ToolAdded { id } => {
                let pointer = PointerId(PEN_POINTER_BASE.wrapping_add(state.tablet.next_tool_id));
                state.tablet.next_tool_id = state.tablet.next_tool_id.wrapping_add(1);
                let shape_device = state
                    .tablet
                    .cursor_manager
                    .as_ref()
                    .map(|cmgr| cmgr.get_tablet_tool_v2(&id, qh, ()));
                state.tablet.tools.insert(
                    id.id(),
                    LiveTool {
                        proxy: id,
                        shape_device,
                        tracker: ToolTracker::new(pointer),
                    },
                );
                let _ = seat_id;
            }
            zwp_tablet_seat_v2::Event::PadAdded { id } => {
                state.tablet.pads.insert(id.id(), id);
            }
            _ => {}
        }
    }

    fn event_created_child(
        opcode: u16,
        qhandle: &QueueHandle<Self>,
    ) -> Arc<dyn wayland_backend::client::ObjectData> {
        match opcode {
            zwp_tablet_seat_v2::EVT_TABLET_ADDED_OPCODE => qhandle.make_data::<ZwpTabletV2, ()>(()),
            zwp_tablet_seat_v2::EVT_TOOL_ADDED_OPCODE => {
                qhandle.make_data::<ZwpTabletToolV2, ()>(())
            }
            zwp_tablet_seat_v2::EVT_PAD_ADDED_OPCODE => qhandle.make_data::<ZwpTabletPadV2, ()>(()),
            _ => unreachable!("unexpected zwp_tablet_seat_v2 child opcode {opcode}"),
        }
    }
}

impl Dispatch<ZwpTabletV2, ()> for WaylandState {
    fn event(
        state: &mut Self,
        tablet: &ZwpTabletV2,
        event: zwp_tablet_v2::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let zwp_tablet_v2::Event::Removed = event
            && let Some(dead) = state.tablet.tablets.remove(&tablet.id())
        {
            dead.destroy();
        }
    }
}

impl Dispatch<ZwpTabletToolV2, ()> for WaylandState {
    fn event(
        state: &mut Self,
        tool: &ZwpTabletToolV2,
        event: zwp_tablet_tool_v2::Event,
        _: &(),
        conn: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let tool_id = tool.id();
        match event {
            zwp_tablet_tool_v2::Event::Type {
                tool_type: WEnum::Value(kind),
            } => {
                if let Some(live) = state.tablet.tools.get_mut(&tool_id) {
                    live.tracker.set_type(ToolClass::from_wayland(kind));
                }
            }
            zwp_tablet_tool_v2::Event::Capability {
                capability: WEnum::Value(cap),
            } => {
                if let Some(live) = state.tablet.tools.get_mut(&tool_id) {
                    live.tracker.add_capability(cap);
                }
            }
            zwp_tablet_tool_v2::Event::ProximityIn {
                serial, surface, ..
            } => {
                let on_ours = surface == state.surface;
                if on_ours {
                    state.record_any_seat_serial(serial);
                }
                let cursor_name = state.tablet.cursor_name;
                if let Some(live) = state.tablet.tools.get_mut(&tool_id) {
                    live.tracker.proximity_in(serial, on_ours);
                    if on_ours {
                        apply_tool_cursor(live, cursor_name);
                        let _ = conn.flush();
                    }
                }
                let prox = state.tablet.proximity_count();
                state.tablet.queue.set_wayland_proximity(prox);
            }
            zwp_tablet_tool_v2::Event::ProximityOut => {
                if let Some(live) = state.tablet.tools.get_mut(&tool_id) {
                    live.tracker.proximity_out();
                }
                let prox = state.tablet.proximity_count();
                state.tablet.queue.set_wayland_proximity(prox);
            }
            zwp_tablet_tool_v2::Event::Down { serial } => {
                state.record_any_seat_serial(serial);
                if let Some(live) = state.tablet.tools.get_mut(&tool_id) {
                    live.tracker.down();
                }
            }
            zwp_tablet_tool_v2::Event::Up => {
                if let Some(live) = state.tablet.tools.get_mut(&tool_id) {
                    live.tracker.up();
                }
            }
            zwp_tablet_tool_v2::Event::Motion { x, y } => {
                if let Some(live) = state.tablet.tools.get_mut(&tool_id) {
                    live.tracker.motion(x, y);
                }
            }
            zwp_tablet_tool_v2::Event::Pressure { pressure } => {
                if let Some(live) = state.tablet.tools.get_mut(&tool_id) {
                    live.tracker.pressure(pressure);
                }
            }
            zwp_tablet_tool_v2::Event::Tilt { tilt_x, tilt_y } => {
                if let Some(live) = state.tablet.tools.get_mut(&tool_id) {
                    live.tracker.tilt(tilt_x, tilt_y);
                }
            }
            zwp_tablet_tool_v2::Event::Rotation { degrees } => {
                if let Some(live) = state.tablet.tools.get_mut(&tool_id) {
                    live.tracker.rotation(degrees);
                }
            }
            zwp_tablet_tool_v2::Event::Button {
                serial,
                button,
                state: WEnum::Value(btn_state),
            } => {
                state.record_any_seat_serial(serial);
                if let Some(live) = state.tablet.tools.get_mut(&tool_id) {
                    live.tracker
                        .button(button, matches!(btn_state, ButtonState::Pressed));
                }
            }
            zwp_tablet_tool_v2::Event::Frame { time } => {
                let scale = state.tablet.scale_factor;
                let stamp = clock::from_wrapped_millis(time, clock::now());
                let batch = state
                    .tablet
                    .tools
                    .get_mut(&tool_id)
                    .map(|live| live.tracker.frame(stamp, scale))
                    .unwrap_or_default();
                let prox = state.tablet.proximity_count();
                state.tablet.queue.push_wayland(batch, prox);
            }
            zwp_tablet_tool_v2::Event::Removed => {
                let mut flush = Vec::new();
                if let Some(mut dead) = state.tablet.tools.remove(&tool_id) {
                    dead.tracker.proximity_out();
                    flush = dead.tracker.frame(clock::now(), state.tablet.scale_factor);
                    if let Some(shape) = dead.shape_device.take() {
                        shape.destroy();
                    }
                    dead.proxy.destroy();
                }
                let prox = state.tablet.proximity_count();
                state.tablet.queue.push_wayland(flush, prox);
            }
            _ => {}
        }
    }
}

impl Dispatch<ZwpTabletPadV2, ()> for WaylandState {
    fn event(
        state: &mut Self,
        pad: &ZwpTabletPadV2,
        event: zwp_tablet_pad_v2::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            zwp_tablet_pad_v2::Event::Group { pad_group } => {
                state.tablet.pad_groups.push(pad_group);
            }
            zwp_tablet_pad_v2::Event::Removed => {
                if let Some(dead) = state.tablet.pads.remove(&pad.id()) {
                    dead.destroy();
                }
            }
            _ => {}
        }
    }

    fn event_created_child(
        opcode: u16,
        qhandle: &QueueHandle<Self>,
    ) -> Arc<dyn wayland_backend::client::ObjectData> {
        match opcode {
            zwp_tablet_pad_v2::EVT_GROUP_OPCODE => qhandle.make_data::<ZwpTabletPadGroupV2, ()>(()),
            _ => unreachable!("unexpected zwp_tablet_pad_v2 child opcode {opcode}"),
        }
    }
}

impl Dispatch<ZwpTabletPadGroupV2, ()> for WaylandState {
    fn event(
        state: &mut Self,
        _: &ZwpTabletPadGroupV2,
        event: zwp_tablet_pad_group_v2::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            zwp_tablet_pad_group_v2::Event::Ring { ring } => {
                state.tablet.pad_rings.push(ring);
            }
            zwp_tablet_pad_group_v2::Event::Strip { strip } => {
                state.tablet.pad_strips.push(strip);
            }
            _ => {}
        }
    }

    fn event_created_child(
        opcode: u16,
        qhandle: &QueueHandle<Self>,
    ) -> Arc<dyn wayland_backend::client::ObjectData> {
        match opcode {
            zwp_tablet_pad_group_v2::EVT_RING_OPCODE => {
                qhandle.make_data::<ZwpTabletPadRingV2, ()>(())
            }
            zwp_tablet_pad_group_v2::EVT_STRIP_OPCODE => {
                qhandle.make_data::<ZwpTabletPadStripV2, ()>(())
            }
            _ => unreachable!("unexpected zwp_tablet_pad_group_v2 child opcode {opcode}"),
        }
    }
}

impl Dispatch<ZwpTabletPadRingV2, ()> for WaylandState {
    fn event(
        _: &mut Self,
        _: &ZwpTabletPadRingV2,
        _: <ZwpTabletPadRingV2 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZwpTabletPadStripV2, ()> for WaylandState {
    fn event(
        _: &mut Self,
        _: &ZwpTabletPadStripV2,
        _: <ZwpTabletPadStripV2 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn single_sample(events: &[PointerEvent]) -> PointerSample {
        assert_eq!(events.len(), 1, "expected 1 event, got {events:?}");
        match &events[0] {
            PointerEvent::Samples {
                kind: PointerKind::Pen,
                samples,
                ..
            } => {
                assert_eq!(samples.len(), 1);
                samples[0]
            }
            other => panic!("expected pen sample, got {other:?}"),
        }
    }

    #[test]
    fn frame_accumulates_axes_and_scales_surface_coordinates() {
        let mut tool = ToolTracker::new(PointerId(PEN_POINTER_BASE));
        tool.set_type(ToolClass::Pen);
        tool.add_capability(Capability::Pressure);
        tool.add_capability(Capability::Tilt);
        tool.add_capability(Capability::Rotation);

        tool.proximity_in(42, true);
        tool.motion(100.5, 200.25);
        tool.pressure(1200);
        tool.tilt(-15.5, 32.0);
        tool.rotation(-30.0);

        // Hover frame: pressure clamps to 0 while `!tip_down`.
        let hover = single_sample(&tool.frame(Ticks(1_000), 2.0));
        assert_eq!(hover.position, [201.0, 400.5]);
        assert_eq!(hover.pressure, Some(0.0));
        assert_eq!(hover.tilt, Some([-15.5, 32.0]));
        assert_eq!(hover.twist, Some(330.0));
        assert_eq!(hover.buttons, Buttons::empty());
        assert!(!hover.in_contact);
        assert!(!hover.inverted);

        // Contact frame: normalized pressure and PRIMARY button.
        tool.down();
        tool.pressure(32_768);
        tool.motion(102.0, 201.0);
        let down = single_sample(&tool.frame(Ticks(2_000), 2.0));
        assert_eq!(down.position, [204.0, 402.0]);
        assert!((down.pressure.unwrap() - 32_768.0 / 65_535.0).abs() < 1e-4);
        assert_eq!(down.buttons, Buttons::PRIMARY);
        assert!(down.in_contact);
        assert_eq!(down.frame_id, hover.frame_id + 1);
    }

    #[test]
    fn barrel_and_eraser_buttons_match_windows_pointer_flags() {
        let mut tool = ToolTracker::new(PointerId(PEN_POINTER_BASE + 1));
        tool.set_type(ToolClass::Eraser);
        tool.add_capability(Capability::Pressure);
        tool.proximity_in(7, true);
        tool.motion(10.0, 20.0);

        // Hovering eraser with barrel held: BARREL only, not in contact, inverted.
        tool.button(BTN_STYLUS, true);
        let hover_barrel = single_sample(&tool.frame(Ticks(10), 1.0));
        assert_eq!(hover_barrel.buttons, Buttons::BARREL);
        assert!(!hover_barrel.in_contact);
        assert!(hover_barrel.inverted);

        // Touching down while barrel is held: SECONDARY | BARREL | ERASER.
        tool.down();
        tool.pressure(65_535);
        let contact_barrel = single_sample(&tool.frame(Ticks(20), 1.0));
        assert_eq!(
            contact_barrel.buttons,
            Buttons::SECONDARY
                .union(Buttons::BARREL)
                .union(Buttons::ERASER)
        );
        assert!(contact_barrel.in_contact);
        assert_eq!(contact_barrel.pressure, Some(1.0));

        // Releasing barrel while tip stays down switches to PRIMARY | ERASER,
        // and pressing BTN_STYLUS2 adds MIDDLE.
        tool.button(BTN_STYLUS, false);
        tool.button(BTN_STYLUS2, true);
        let middle = single_sample(&tool.frame(Ticks(30), 1.0));
        assert_eq!(
            middle.buttons,
            Buttons::PRIMARY
                .union(Buttons::ERASER)
                .union(Buttons::MIDDLE)
        );
    }

    #[test]
    fn up_and_proximity_out_in_one_frame_end_the_stroke_before_leaving() {
        let ptr = PointerId(PEN_POINTER_BASE + 2);
        let mut tool = ToolTracker::new(ptr);
        tool.proximity_in(1, true);
        tool.motion(50.0, 60.0);
        tool.down();
        let _ = tool.frame(Ticks(100), 1.0);

        // Normal lift out of proximity: compositor sends `up`, `proximity_out`, `frame`.
        tool.up();
        tool.proximity_out();
        let events = tool.frame(Ticks(200), 1.0);
        assert_eq!(events.len(), 2, "{events:?}");
        match &events[0] {
            PointerEvent::Samples { samples, .. } => {
                assert_eq!(samples[0].buttons, Buttons::empty());
                assert!(!samples[0].in_contact);
            }
            other => panic!("expected release sample first, got {other:?}"),
        }
        assert_eq!(events[1], PointerEvent::Leave { pointer: ptr });
    }

    #[test]
    fn proximity_loss_mid_stroke_without_up_emits_capture_lost() {
        let ptr = PointerId(PEN_POINTER_BASE + 3);
        let mut tool = ToolTracker::new(ptr);
        tool.proximity_in(1, true);
        tool.motion(50.0, 60.0);
        tool.down();
        let _ = tool.frame(Ticks(100), 1.0);

        // Focus stolen while tip is still down (no `up` event).
        tool.proximity_out();
        let events = tool.frame(Ticks(200), 1.0);
        assert_eq!(
            events,
            vec![
                PointerEvent::CaptureLost { pointer: ptr },
                PointerEvent::Leave { pointer: ptr },
            ]
        );
    }

    #[test]
    fn puck_mouse_tools_are_ignored_by_pen_tracker() {
        let mut puck = ToolTracker::new(PointerId(PEN_POINTER_BASE + 4));
        puck.set_type(ToolClass::from_wayland(ToolType::Mouse));
        puck.proximity_in(3, true);
        puck.motion(12.0, 34.0);
        puck.down();
        assert!(!puck.in_proximity());
        assert!(puck.frame(Ticks(10), 1.0).is_empty());
    }
}

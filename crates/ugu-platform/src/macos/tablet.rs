// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! Pen input from AppKit. winit 0.30.13 hands the pen on as a mouse, without
//! its pressure or which end is down, so a local event monitor reads each
//! tablet event first. AppKit then dispatches the event on to winit's view,
//! which reports it to the app before the next event is read; the app drops
//! the mouse events winit makes of it (`Tablet::drain`).

use std::cell::RefCell;
use std::collections::HashMap;
use std::ptr::NonNull;
use std::rc::Rc;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_app_kit::{
    NSEvent, NSEventButtonMask, NSEventMask, NSEventSubtype, NSEventType, NSPointingDeviceType,
    NSView,
};
use ugu_platform_types::clock::Ticks;
use ugu_platform_types::pointer::{PointerEvent, PointerId, PointerKind, PointerSample};

use super::clock;
use crate::pen::{PEN_POINTER_BASE, pen_buttons};

pub struct Tablet {
    monitor: Retained<AnyObject>,
    state: Rc<RefCell<State>>,
}

impl Tablet {
    /// Starts reading the pen over `view`. `wake` is called when pen events
    /// wait that no window event of winit's will follow to collect.
    ///
    /// # Safety
    /// On the main thread, with `view` the view of the app's window, which
    /// outlives the returned value.
    pub unsafe fn install(view: isize, wake: impl Fn() + 'static) -> Self {
        // SAFETY: as the caller promises.
        let view = unsafe { Retained::retain(view as *mut NSView) }.expect("the view is not null");
        // Every sample the pen makes, not one per frame.
        NSEvent::setMouseCoalescingEnabled(false);
        let state = Rc::new(RefCell::new(State::default()));
        let monitored = state.clone();
        let handler = RcBlock::new(move |event: NonNull<NSEvent>| -> *mut NSEvent {
            // SAFETY: AppKit passes a live event.
            let reading = read(&view, unsafe { event.as_ref() });
            let wake_now = monitored.borrow_mut().take(reading);
            if wake_now {
                wake();
            }
            event.as_ptr()
        });
        let mask = NSEventMask::TabletPoint
            | NSEventMask::TabletProximity
            | NSEventMask::MouseMoved
            | NSEventMask::LeftMouseDown
            | NSEventMask::LeftMouseUp
            | NSEventMask::LeftMouseDragged
            | NSEventMask::RightMouseDown
            | NSEventMask::RightMouseUp
            | NSEventMask::RightMouseDragged
            | NSEventMask::OtherMouseDown
            | NSEventMask::OtherMouseUp
            | NSEventMask::OtherMouseDragged;
        // SAFETY: the handler returns the event it was given.
        let monitor =
            unsafe { NSEvent::addLocalMonitorForEventsMatchingMask_handler(mask, &handler) }
                .expect("AppKit returns a monitor");
        Self { monitor, state }
    }

    /// The pen events since the last call, and whether winit's latest mouse
    /// events came from the pen and so are not the mouse's.
    pub fn drain(&self) -> (Vec<PointerEvent>, bool) {
        let mut state = self.state.borrow_mut();
        (std::mem::take(&mut state.events), state.mouse_is_pen)
    }
}

impl Drop for Tablet {
    fn drop(&mut self) {
        // SAFETY: the monitor `addLocalMonitorForEventsMatchingMask` returned.
        unsafe { NSEvent::removeMonitor(&self.monitor) };
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Reading {
    /// Not an event of the app's window, or not pointer input.
    Other,
    /// A mouse's event, which winit reports as the mouse.
    Mouse,
    Proximity {
        device: usize,
        entering: bool,
        eraser: bool,
    },
    Point(Point),
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Point {
    device: usize,
    /// In the view's physical pixels from its top left.
    position: [f64; 2],
    inside: bool,
    pressure: f32,
    tip: bool,
    lower: bool,
    upper: bool,
    /// Whether winit sees it too: a mouse event, not a bare tablet one.
    as_mouse: bool,
    time: Ticks,
}

fn read(view: &NSView, event: &NSEvent) -> Reading {
    let kind = event.r#type();
    let subtype = mouse_subtype(event, kind);
    if kind == NSEventType::TabletProximity || subtype == Some(NSEventSubtype::TabletProximity) {
        return Reading::Proximity {
            device: event.deviceID(),
            entering: event.isEnteringProximity(),
            eraser: event.pointingDeviceType() == NSPointingDeviceType::Eraser,
        };
    }
    let Some(window) = view.window() else {
        return Reading::Other;
    };
    if event.windowNumber() != window.windowNumber() {
        return Reading::Other;
    }
    let as_mouse = kind != NSEventType::TabletPoint;
    if as_mouse && subtype != Some(NSEventSubtype::TabletPoint) {
        return Reading::Mouse;
    }
    let point = view.convertPoint_fromView(event.locationInWindow(), None);
    let size = view.frame().size;
    let scale = window.backingScaleFactor();
    let held = event.buttonMask();
    let (down, up) = match kind {
        NSEventType::LeftMouseDown | NSEventType::LeftMouseDragged => (Some(Button::Tip), None),
        NSEventType::LeftMouseUp => (None, Some(Button::Tip)),
        NSEventType::RightMouseDown | NSEventType::RightMouseDragged => (Some(Button::Lower), None),
        NSEventType::RightMouseUp => (None, Some(Button::Lower)),
        NSEventType::OtherMouseDown | NSEventType::OtherMouseDragged => (Some(Button::Upper), None),
        NSEventType::OtherMouseUp => (None, Some(Button::Upper)),
        _ => (None, None),
    };
    // The mask says which buttons are held. A barrel button the tablet's
    // settings make a right or middle click comes as that click instead,
    // which may or may not show in the mask.
    let pressed = |button: Button, bit: NSEventButtonMask| {
        up != Some(button) && (down == Some(button) || held.contains(bit))
    };
    Reading::Point(Point {
        device: event.deviceID(),
        position: [point.x * scale, point.y * scale],
        inside: (0.0..=size.width).contains(&point.x) && (0.0..=size.height).contains(&point.y),
        pressure: event.pressure(),
        tip: pressed(Button::Tip, NSEventButtonMask::PenTip),
        lower: pressed(Button::Lower, NSEventButtonMask::PenLowerSide),
        upper: pressed(Button::Upper, NSEventButtonMask::PenUpperSide),
        as_mouse,
        time: clock::from_uptime(event.timestamp()),
    })
}

/// AppKit raises an exception when `subtype` is asked of other events.
fn mouse_subtype(event: &NSEvent, kind: NSEventType) -> Option<NSEventSubtype> {
    matches!(
        kind,
        NSEventType::MouseMoved
            | NSEventType::LeftMouseDown
            | NSEventType::LeftMouseUp
            | NSEventType::LeftMouseDragged
            | NSEventType::RightMouseDown
            | NSEventType::RightMouseUp
            | NSEventType::RightMouseDragged
            | NSEventType::OtherMouseDown
            | NSEventType::OtherMouseUp
            | NSEventType::OtherMouseDragged
    )
    .then(|| event.subtype())
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Button {
    Tip,
    Lower,
    Upper,
}

#[derive(Default)]
struct State {
    events: Vec<PointerEvent>,
    mouse_is_pen: bool,
    devices: HashMap<usize, Device>,
    frame: u32,
}

#[derive(Default)]
struct Device {
    /// Told by the proximity event; a pen already near at launch is taken
    /// for its tip.
    eraser: bool,
    /// Whether the app was last told where it is.
    shown: bool,
}

impl State {
    /// Takes in one event, and says whether to wake the app for it.
    fn take(&mut self, reading: Reading) -> bool {
        let before = self.events.len();
        let winit_follows = match reading {
            Reading::Other => return false,
            Reading::Mouse => {
                self.mouse_is_pen = false;
                return false;
            }
            Reading::Proximity {
                device,
                entering,
                eraser,
            } => {
                if entering {
                    self.devices.entry(device).or_default().eraser = eraser;
                } else if let Some(gone) = self.devices.get_mut(&device) {
                    let shown = std::mem::take(&mut gone.shown);
                    self.leave(device, shown);
                }
                false
            }
            Reading::Point(point) => {
                if point.as_mouse {
                    self.mouse_is_pen = true;
                }
                self.point(point);
                point.as_mouse && point.inside
            }
        };
        self.events.len() > before && !winit_follows
    }

    fn point(&mut self, point: Point) {
        let device = self.devices.entry(point.device).or_default();
        let shown = device.shown;
        let held = point.tip || point.lower || point.upper;
        if !point.inside && !held {
            device.shown = false;
            self.leave(point.device, shown);
            return;
        }
        device.shown = true;
        let inverted = device.eraser;
        let (buttons, in_contact) = pen_buttons(point.tip, point.lower, point.upper, inverted);
        self.frame = self.frame.wrapping_add(1);
        let pointer = pointer_of(point.device);
        let sample = PointerSample {
            position: point.position,
            pressure: Some(point.pressure),
            tilt: None,
            twist: None,
            buttons,
            in_contact,
            inverted,
            time: point.time,
            frame_id: self.frame,
        };
        if let Some(PointerEvent::Samples {
            pointer: last,
            samples,
            ..
        }) = self.events.last_mut()
            && *last == pointer
        {
            samples.push(sample);
            return;
        }
        self.events.push(PointerEvent::Samples {
            pointer,
            kind: PointerKind::Pen,
            samples: vec![sample],
        });
    }

    fn leave(&mut self, device: usize, shown: bool) {
        if shown {
            self.events.push(PointerEvent::Leave {
                pointer: pointer_of(device),
            });
        }
    }
}

fn pointer_of(device: usize) -> PointerId {
    PointerId(PEN_POINTER_BASE.wrapping_add(device as u32))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ugu_platform_types::pointer::Buttons;

    fn point(device: usize, x: f64) -> Point {
        Point {
            device,
            position: [x, 20.0],
            inside: true,
            pressure: 0.0,
            tip: false,
            lower: false,
            upper: false,
            as_mouse: true,
            time: Ticks(1),
        }
    }

    fn samples(events: &[PointerEvent]) -> Vec<&PointerSample> {
        events
            .iter()
            .flat_map(|event| match event {
                PointerEvent::Samples { samples, .. } => samples.iter().collect(),
                _ => Vec::new(),
            })
            .collect()
    }

    #[test]
    fn a_stroke_comes_with_its_pressure_and_owns_the_mouse_events() {
        let mut state = State::default();
        state.take(Reading::Point(point(5, 10.0)));
        let down = Point {
            tip: true,
            pressure: 0.4,
            ..point(5, 11.0)
        };
        assert!(
            !state.take(Reading::Point(down)),
            "winit's event wakes the app"
        );
        let (events, mouse_is_pen) = (std::mem::take(&mut state.events), state.mouse_is_pen);
        assert!(mouse_is_pen);
        let [hover, down] = samples(&events)[..] else {
            panic!("{events:?}");
        };
        assert_eq!(hover.buttons, Buttons::empty());
        assert!(!hover.in_contact);
        assert_eq!(down.buttons, Buttons::PRIMARY);
        assert_eq!(down.pressure, Some(0.4));
        assert!(down.in_contact);
        assert!(matches!(
            events[..],
            [PointerEvent::Samples {
                pointer: PointerId(p),
                kind: PointerKind::Pen,
                ..
            }] if p == PEN_POINTER_BASE + 5
        ));
    }

    #[test]
    fn the_mouse_takes_its_events_back() {
        let mut state = State::default();
        state.take(Reading::Point(point(5, 10.0)));
        state.take(Reading::Mouse);
        assert!(!state.mouse_is_pen);
        state.take(Reading::Other);
        assert!(
            !state.mouse_is_pen,
            "a key or another window changes nothing"
        );
    }

    #[test]
    fn the_eraser_end_is_known_from_its_proximity() {
        let mut state = State::default();
        state.take(Reading::Proximity {
            device: 7,
            entering: true,
            eraser: true,
        });
        state.take(Reading::Point(Point {
            tip: true,
            ..point(7, 10.0)
        }));
        let [sample] = samples(&state.events)[..] else {
            panic!("{:?}", state.events);
        };
        assert!(sample.inverted);
        assert_eq!(sample.buttons, Buttons::PRIMARY.union(Buttons::ERASER));
    }

    #[test]
    fn a_barrel_button_held_over_the_canvas_is_reported() {
        let mut state = State::default();
        state.take(Reading::Point(Point {
            lower: true,
            ..point(1, 10.0)
        }));
        state.take(Reading::Point(Point {
            tip: true,
            lower: true,
            ..point(1, 11.0)
        }));
        let [hover, down] = samples(&state.events)[..] else {
            panic!("{:?}", state.events);
        };
        assert_eq!(hover.buttons, Buttons::BARREL);
        assert_eq!(down.buttons, Buttons::SECONDARY.union(Buttons::BARREL));
    }

    #[test]
    fn leaving_the_view_or_the_tablet_says_so_once() {
        let mut state = State::default();
        state.take(Reading::Point(point(2, 10.0)));
        let outside = Point {
            inside: false,
            ..point(2, -5.0)
        };
        assert!(
            state.take(Reading::Point(outside)),
            "winit sends nothing outside"
        );
        assert!(!state.take(Reading::Point(outside)));
        assert!(matches!(
            state.events[..],
            [PointerEvent::Samples { .. }, PointerEvent::Leave { .. }]
        ));
        state.events.clear();
        state.take(Reading::Point(point(2, 10.0)));
        let woke = state.take(Reading::Proximity {
            device: 2,
            entering: false,
            eraser: false,
        });
        assert!(woke, "no window event follows a proximity event");
        assert!(matches!(
            state.events[..],
            [PointerEvent::Samples { .. }, PointerEvent::Leave { .. }]
        ));
    }

    #[test]
    fn a_stroke_dragged_out_of_the_view_goes_on() {
        let mut state = State::default();
        let outside = Point {
            inside: false,
            tip: true,
            ..point(3, -5.0)
        };
        state.take(Reading::Point(outside));
        let [sample] = samples(&state.events)[..] else {
            panic!("{:?}", state.events);
        };
        assert!(sample.in_contact);
    }

    #[test]
    fn a_bare_tablet_event_wakes_the_app() {
        let mut state = State::default();
        let bare = Point {
            as_mouse: false,
            ..point(4, 10.0)
        };
        assert!(state.take(Reading::Point(bare)));
        assert!(!state.mouse_is_pen, "winit made no mouse event of it");
    }
}

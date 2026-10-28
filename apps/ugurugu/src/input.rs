// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! Splits Win32 pointer input between egui and the canvas.
//!
//! Every pointer message reaches the app only through `ugu_platform::pointer`, so
//! egui and the canvas see the same samples and no synthesised mouse events.

use std::collections::{BTreeMap, HashMap};

use ugu_platform::clock::Ticks;
use ugu_platform::pointer::{Buttons, PointerEvent, PointerId, PointerKind, PointerSample};

const EGUI_BUTTONS: [(Buttons, egui::PointerButton); 5] = [
    (Buttons::PRIMARY, egui::PointerButton::Primary),
    (Buttons::SECONDARY, egui::PointerButton::Secondary),
    (Buttons::MIDDLE, egui::PointerButton::Middle),
    (Buttons::X1, egui::PointerButton::Extra1),
    (Buttons::X2, egui::PointerButton::Extra2),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gesture {
    Draw,
    Pan,
    /// Turns the view: Shift+Space and drag.
    Turn,
    /// Zooms the view: Ctrl+Space and drag.
    Zoom,
}

/// What the canvas receives, in client physical pixels.
#[derive(Clone, Debug, PartialEq)]
pub enum CanvasInput {
    Begin(Gesture, PointerKind, PointerSample),
    Extend(PointerSample),
    End(PointerSample),
    Cancel,
    /// Wheel notches over the canvas; positive zooms in.
    Zoom {
        position: [f64; 2],
        notches: f32,
    },
    /// Wheel notches with Shift over the canvas; positive turns clockwise.
    Turn {
        notches: f32,
    },
    /// Exactly two fingers on the canvas, by pointer id; `fresh` when they
    /// are not the two of the last `Pinch`.
    Pinch {
        fingers: [[f64; 2]; 2],
        fresh: bool,
    },
    /// No longer two fingers.
    PinchEnd,
}

#[derive(Default)]
pub struct InputRouter {
    buttons: HashMap<PointerId, Buttons>,
    /// The pointer the canvas owns and the button that started its gesture.
    canvas_pointer: Option<(PointerId, Buttons)>,
    oldest_unpresented: Option<Ticks>,
    /// Fingers on the canvas, by pointer id, since the first of them went
    /// down there. They never draw: two pan, zoom and turn the view.
    touches: BTreeMap<u32, [f64; 2]>,
    /// The fingers do nothing until all are lifted: they came down while
    /// the canvas was busy, or a pen came down since.
    touch_ignored: bool,
    /// The two fingers of the last `Pinch`.
    pinching: Option<[u32; 2]>,
}

pub struct Routed {
    pub egui: Vec<egui::Event>,
    pub canvas: Vec<CanvasInput>,
    /// Whether the last pen or mouse sample was a pen's eraser end.
    pub eraser_end: Option<bool>,
}

impl Routed {
    /// Whether the pen or mouse pressed, dragged or let go: Alt held then
    /// is for that, as the shape tool's centre or the selection's subtract,
    /// not a tap for the menus.
    pub fn uses_buttons(&self) -> bool {
        self.canvas.iter().any(|input| {
            matches!(
                input,
                CanvasInput::Begin(..) | CanvasInput::Extend(..) | CanvasInput::End(..)
            )
        }) || self
            .egui
            .iter()
            .any(|event| matches!(event, egui::Event::PointerButton { pressed: true, .. }))
    }
}

impl InputRouter {
    /// `starts_on_canvas` decides, for a press at a physical position,
    /// whether the canvas owns the gesture instead of the UI. A primary
    /// press draws, or while `pan_held` pans, turns with Shift or zooms with
    /// Ctrl, as in 2.2.13; a middle press pans. Shift turns the wheel too.
    /// Fingers on the canvas follow 2.2.13's touch rules: two pan, zoom and
    /// turn, a pen press takes over from them, and a mouse press waits.
    pub fn route(
        &mut self,
        events: Vec<PointerEvent>,
        pixels_per_point: f32,
        modifiers: egui::Modifiers,
        pan_held: bool,
        starts_on_canvas: impl Fn([f64; 2]) -> bool,
    ) -> Routed {
        let mut routed = Routed {
            egui: Vec::new(),
            canvas: Vec::new(),
            eraser_end: None,
        };
        let to_points = |position: [f64; 2]| {
            egui::pos2(
                position[0] as f32 / pixels_per_point,
                position[1] as f32 / pixels_per_point,
            )
        };

        for event in events {
            match event {
                PointerEvent::Samples {
                    pointer,
                    kind,
                    samples,
                } => {
                    for sample in samples {
                        self.oldest_unpresented.get_or_insert(sample.time);
                        match kind {
                            PointerKind::Pen => routed.eraser_end = Some(sample.inverted),
                            PointerKind::Mouse => routed.eraser_end = Some(false),
                            PointerKind::Touch | PointerKind::Touchpad => {}
                        }
                        let before = self.buttons.get(&pointer).copied().unwrap_or_default();
                        self.buttons.insert(pointer, sample.buttons);
                        let pressed = sample.buttons.difference(before);
                        let released = before.difference(sample.buttons);

                        if matches!(kind, PointerKind::Touch | PointerKind::Touchpad)
                            && (self.touches.contains_key(&pointer.0)
                                || pressed.contains(Buttons::PRIMARY)
                                    && starts_on_canvas(sample.position))
                        {
                            if self.touches.is_empty() {
                                self.touch_ignored = self.canvas_pointer.is_some();
                            }
                            if sample.buttons.contains(Buttons::PRIMARY) {
                                self.touches.insert(pointer.0, sample.position);
                            } else {
                                self.touches.remove(&pointer.0);
                            }
                            self.touches_changed(&mut routed.canvas);
                            continue;
                        }
                        let fingers_down = !self.touches.is_empty() && !self.touch_ignored;
                        if fingers_down && pressed.contains(Buttons::PRIMARY) {
                            match kind {
                                PointerKind::Pen => {
                                    self.touch_ignored = true;
                                    self.touches_changed(&mut routed.canvas);
                                }
                                _ if starts_on_canvas(sample.position) => continue,
                                _ => {}
                            }
                        }

                        if let Some((owner, button)) = self.canvas_pointer
                            && owner == pointer
                        {
                            if released.contains(button) {
                                self.canvas_pointer = None;
                                routed.canvas.push(CanvasInput::End(sample));
                            } else {
                                routed.canvas.push(CanvasInput::Extend(sample));
                            }
                            continue;
                        }
                        let gesture = if pressed.contains(Buttons::PRIMARY) {
                            Some((
                                Buttons::PRIMARY,
                                match (pan_held, modifiers.command, modifiers.shift) {
                                    (false, ..) => Gesture::Draw,
                                    (true, true, _) => Gesture::Zoom,
                                    (true, false, true) => Gesture::Turn,
                                    (true, false, false) => Gesture::Pan,
                                },
                            ))
                        } else if pressed.contains(Buttons::MIDDLE) {
                            Some((Buttons::MIDDLE, Gesture::Pan))
                        } else {
                            None
                        };
                        if let Some((button, gesture)) = gesture
                            && self.canvas_pointer.is_none()
                            && starts_on_canvas(sample.position)
                        {
                            self.canvas_pointer = Some((pointer, button));
                            routed
                                .canvas
                                .push(CanvasInput::Begin(gesture, kind, sample));
                            continue;
                        }

                        let pos = to_points(sample.position);
                        routed.egui.push(egui::Event::PointerMoved(pos));
                        for (button, egui_button) in EGUI_BUTTONS {
                            if pressed.contains(button) || released.contains(button) {
                                routed.egui.push(egui::Event::PointerButton {
                                    pos,
                                    button: egui_button,
                                    pressed: pressed.contains(button),
                                    modifiers,
                                });
                            }
                        }
                    }
                }
                PointerEvent::Leave { pointer } | PointerEvent::CaptureLost { pointer }
                    if self.touches.contains_key(&pointer.0) =>
                {
                    self.buttons.remove(&pointer);
                    self.touches.remove(&pointer.0);
                    self.touches_changed(&mut routed.canvas);
                }
                PointerEvent::Leave { pointer } => {
                    if self.canvas_pointer.map(|(owner, _)| owner) != Some(pointer) {
                        routed.egui.push(egui::Event::PointerGone);
                    }
                }
                PointerEvent::CaptureLost { pointer } => {
                    self.buttons.remove(&pointer);
                    if self.canvas_pointer.map(|(owner, _)| owner) == Some(pointer) {
                        self.canvas_pointer = None;
                        routed.canvas.push(CanvasInput::Cancel);
                    }
                }
                PointerEvent::Wheel {
                    position,
                    delta,
                    horizontal,
                    time,
                } => {
                    self.oldest_unpresented.get_or_insert(time);
                    if !horizontal && starts_on_canvas(position) {
                        routed.canvas.push(if modifiers.shift {
                            CanvasInput::Turn { notches: delta }
                        } else {
                            CanvasInput::Zoom {
                                position,
                                notches: delta,
                            }
                        });
                        continue;
                    }
                    let delta = if horizontal {
                        egui::vec2(delta, 0.0)
                    } else {
                        egui::vec2(0.0, delta)
                    };
                    routed.egui.push(egui::Event::MouseWheel {
                        unit: egui::MouseWheelUnit::Line,
                        delta,
                        phase: egui::TouchPhase::Move,
                        modifiers,
                    });
                }
            }
        }
        routed
    }

    /// Pinches with exactly two fingers, and ends the pinch otherwise.
    fn touches_changed(&mut self, canvas: &mut Vec<CanvasInput>) {
        if self.touches.is_empty() {
            self.touch_ignored = false;
        }
        let two = (!self.touch_ignored && self.touches.len() == 2).then(|| {
            let mut fingers = self.touches.iter();
            let (Some((&first, &a)), Some((&second, &b))) = (fingers.next(), fingers.next()) else {
                unreachable!("two fingers");
            };
            ([first, second], [a, b])
        });
        match two {
            Some((ids, fingers)) => {
                canvas.push(CanvasInput::Pinch {
                    fingers,
                    fresh: self.pinching != Some(ids),
                });
                self.pinching = Some(ids);
            }
            None => {
                if self.pinching.take().is_some() {
                    canvas.push(CanvasInput::PinchEnd);
                }
            }
        }
    }

    /// The oldest input time not yet shown, cleared once a frame presents it.
    pub fn take_oldest_unpresented(&mut self) -> Option<Ticks> {
        self.oldest_unpresented.take()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(x: f64, buttons: Buttons, time: u64) -> PointerSample {
        PointerSample {
            position: [x, 0.0],
            pressure: None,
            tilt: None,
            twist: None,
            buttons,
            in_contact: !buttons.is_empty(),
            inverted: false,
            time: Ticks(time),
            frame_id: time as u32,
        }
    }

    fn samples(pointer: u32, samples: Vec<PointerSample>) -> PointerEvent {
        PointerEvent::Samples {
            pointer: PointerId(pointer),
            kind: PointerKind::Mouse,
            samples,
        }
    }

    fn route(router: &mut InputRouter, events: Vec<PointerEvent>) -> Routed {
        router.route(events, 1.0, egui::Modifiers::NONE, false, |position| {
            position[0] >= 100.0
        })
    }

    #[test]
    fn canvas_keeps_the_gesture_outside_its_area() {
        let mut router = InputRouter::default();
        let routed = route(
            &mut router,
            vec![samples(
                1,
                vec![
                    sample(150.0, Buttons::PRIMARY, 1),
                    sample(50.0, Buttons::PRIMARY, 2),
                    sample(40.0, Buttons::empty(), 3),
                ],
            )],
        );
        assert!(routed.egui.is_empty());
        assert!(matches!(
            routed.canvas[0],
            CanvasInput::Begin(Gesture::Draw, ..)
        ));
        assert!(matches!(routed.canvas[1], CanvasInput::Extend(..)));
        assert!(matches!(routed.canvas[2], CanvasInput::End(..)));
    }

    #[test]
    fn a_drag_or_release_on_the_canvas_uses_the_buttons() {
        let mut router = InputRouter::default();
        let pressed = route(
            &mut router,
            vec![samples(1, vec![sample(150.0, Buttons::PRIMARY, 1)])],
        );
        assert!(pressed.uses_buttons());
        // Alt pressed now, between the press and these.
        let dragged = route(
            &mut router,
            vec![samples(1, vec![sample(160.0, Buttons::PRIMARY, 2)])],
        );
        assert!(dragged.uses_buttons());
        let released = route(
            &mut router,
            vec![samples(1, vec![sample(160.0, Buttons::empty(), 3)])],
        );
        assert!(released.uses_buttons());
        let hovered = route(
            &mut router,
            vec![samples(1, vec![sample(170.0, Buttons::empty(), 4)])],
        );
        assert!(!hovered.uses_buttons());
    }

    #[test]
    fn the_middle_button_pans_until_it_is_released() {
        let mut router = InputRouter::default();
        let routed = route(
            &mut router,
            vec![samples(
                1,
                vec![
                    sample(150.0, Buttons::MIDDLE, 1),
                    sample(160.0, Buttons::MIDDLE.union(Buttons::PRIMARY), 2),
                    sample(170.0, Buttons::PRIMARY, 3),
                ],
            )],
        );
        assert!(matches!(
            routed.canvas[0],
            CanvasInput::Begin(Gesture::Pan, ..)
        ));
        assert!(matches!(routed.canvas[1], CanvasInput::Extend(..)));
        assert!(matches!(routed.canvas[2], CanvasInput::End(..)));
    }

    #[test]
    fn space_with_shift_turns_and_with_ctrl_zooms_and_shift_turns_the_wheel() {
        let press = |modifiers: egui::Modifiers, pan_held: bool| {
            let mut router = InputRouter::default();
            let routed = router.route(
                vec![samples(1, vec![sample(150.0, Buttons::PRIMARY, 1)])],
                1.0,
                modifiers,
                pan_held,
                |_| true,
            );
            match routed.canvas[0] {
                CanvasInput::Begin(gesture, ..) => gesture,
                _ => panic!("no gesture"),
            }
        };
        assert_eq!(press(egui::Modifiers::NONE, true), Gesture::Pan);
        assert_eq!(press(egui::Modifiers::SHIFT, true), Gesture::Turn);
        assert_eq!(press(egui::Modifiers::COMMAND, true), Gesture::Zoom);
        assert_eq!(press(egui::Modifiers::SHIFT, false), Gesture::Draw);

        let mut router = InputRouter::default();
        let routed = router.route(
            vec![PointerEvent::Wheel {
                position: [150.0, 0.0],
                delta: -2.0,
                horizontal: false,
                time: Ticks(1),
            }],
            1.0,
            egui::Modifiers::SHIFT,
            false,
            |_| true,
        );
        assert_eq!(routed.canvas, [CanvasInput::Turn { notches: -2.0 }]);
    }

    fn finger(pointer: u32, x: f64, down: bool) -> PointerEvent {
        let buttons = if down {
            Buttons::PRIMARY
        } else {
            Buttons::empty()
        };
        PointerEvent::Samples {
            pointer: PointerId(pointer),
            kind: PointerKind::Touch,
            samples: vec![sample(x, buttons, 1)],
        }
    }

    fn pen(x: f64, down: bool) -> PointerEvent {
        PointerEvent::Samples {
            pointer: PointerId(9),
            kind: PointerKind::Pen,
            samples: vec![sample(
                x,
                if down {
                    Buttons::PRIMARY
                } else {
                    Buttons::empty()
                },
                1,
            )],
        }
    }

    fn pinch(input: &CanvasInput) -> Option<bool> {
        match input {
            CanvasInput::Pinch { fresh, .. } => Some(*fresh),
            _ => None,
        }
    }

    #[test]
    fn the_last_pen_or_mouse_sample_says_whether_the_eraser_end_shows() {
        let mut router = InputRouter::default();
        let mut eraser = sample(150.0, Buttons::empty(), 1);
        eraser.inverted = true;
        let pen_hover = PointerEvent::Samples {
            pointer: PointerId(9),
            kind: PointerKind::Pen,
            samples: vec![eraser],
        };
        assert_eq!(route(&mut router, vec![pen_hover]).eraser_end, Some(true));
        let mouse = samples(1, vec![sample(150.0, Buttons::empty(), 2)]);
        assert_eq!(route(&mut router, vec![mouse]).eraser_end, Some(false));
        assert_eq!(
            route(&mut router, vec![finger(3, 150.0, true)]).eraser_end,
            None
        );
    }

    #[test]
    fn fingers_never_draw_and_exactly_two_pinch() {
        let mut router = InputRouter::default();
        let routed = route(
            &mut router,
            vec![finger(1, 150.0, true), finger(1, 160.0, true)],
        );
        assert!(routed.canvas.is_empty() && routed.egui.is_empty());
        let routed = route(
            &mut router,
            vec![finger(2, 300.0, true), finger(2, 310.0, true)],
        );
        assert_eq!(
            routed.canvas.iter().map(pinch).collect::<Vec<_>>(),
            [Some(true), Some(false)]
        );
        let CanvasInput::Pinch { fingers, .. } = routed.canvas[1] else {
            unreachable!()
        };
        assert_eq!(fingers, [[160.0, 0.0], [310.0, 0.0]], "by pointer id");
        // A third finger ends the pinch; lifting it pinches the two again.
        let routed = route(
            &mut router,
            vec![finger(3, 200.0, true), finger(3, 200.0, false)],
        );
        assert_eq!(routed.canvas[0], CanvasInput::PinchEnd);
        assert_eq!(pinch(&routed.canvas[1]), Some(true));
        let routed = route(
            &mut router,
            vec![finger(1, 160.0, false), finger(2, 310.0, false)],
        );
        assert_eq!(routed.canvas, [CanvasInput::PinchEnd]);
        // Off the canvas a finger works the interface.
        let routed = route(&mut router, vec![finger(4, 50.0, true)]);
        assert!(routed.canvas.is_empty() && !routed.egui.is_empty());
    }

    #[test]
    fn a_pen_takes_over_from_fingers_and_a_mouse_waits_for_them() {
        let mut router = InputRouter::default();
        route(
            &mut router,
            vec![finger(1, 150.0, true), finger(2, 300.0, true)],
        );
        let routed = route(
            &mut router,
            vec![samples(5, vec![sample(200.0, Buttons::PRIMARY, 2)])],
        );
        assert!(
            routed.canvas.is_empty(),
            "a mouse press waits: {:?}",
            routed.canvas
        );
        let routed = route(
            &mut router,
            vec![samples(5, vec![sample(200.0, Buttons::empty(), 3)])],
        );
        assert!(routed.canvas.is_empty());
        let routed = route(&mut router, vec![pen(200.0, true), finger(2, 320.0, true)]);
        assert_eq!(routed.canvas[0], CanvasInput::PinchEnd);
        assert!(matches!(
            routed.canvas[1],
            CanvasInput::Begin(Gesture::Draw, PointerKind::Pen, _)
        ));
        assert_eq!(routed.canvas.len(), 2, "the fingers do nothing now");
        // Until every finger is up, then two pinch again.
        route(&mut router, vec![pen(200.0, false)]);
        route(
            &mut router,
            vec![finger(1, 150.0, false), finger(2, 320.0, false)],
        );
        let routed = route(
            &mut router,
            vec![finger(1, 150.0, true), finger(2, 300.0, true)],
        );
        assert_eq!(pinch(&routed.canvas[0]), Some(true));
    }

    #[test]
    fn fingers_that_come_down_while_the_canvas_is_busy_do_nothing() {
        let mut router = InputRouter::default();
        route(
            &mut router,
            vec![samples(5, vec![sample(200.0, Buttons::PRIMARY, 1)])],
        );
        let routed = route(
            &mut router,
            vec![finger(1, 150.0, true), finger(2, 300.0, true)],
        );
        assert!(routed.canvas.is_empty());
        route(
            &mut router,
            vec![samples(5, vec![sample(200.0, Buttons::empty(), 2)])],
        );
        let routed = route(&mut router, vec![finger(2, 310.0, true)]);
        assert!(routed.canvas.is_empty(), "not until they are all lifted");
    }

    #[test]
    fn the_wheel_zooms_over_the_canvas_and_scrolls_elsewhere() {
        let mut router = InputRouter::default();
        let wheel = |x: f64| PointerEvent::Wheel {
            position: [x, 0.0],
            delta: 1.0,
            horizontal: false,
            time: Ticks(1),
        };
        let routed = route(&mut router, vec![wheel(150.0), wheel(50.0)]);
        assert_eq!(
            routed.canvas,
            [CanvasInput::Zoom {
                position: [150.0, 0.0],
                notches: 1.0
            }]
        );
        assert_eq!(routed.egui.len(), 1);
    }

    #[test]
    fn press_on_the_ui_goes_to_egui() {
        let mut router = InputRouter::default();
        let routed = route(
            &mut router,
            vec![samples(1, vec![sample(10.0, Buttons::PRIMARY, 1)])],
        );
        assert!(routed.canvas.is_empty());
        assert!(
            routed
                .egui
                .iter()
                .any(|event| matches!(event, egui::Event::PointerButton { pressed: true, .. }))
        );
    }

    #[test]
    fn capture_loss_cancels_the_stroke() {
        let mut router = InputRouter::default();
        route(
            &mut router,
            vec![samples(1, vec![sample(150.0, Buttons::PRIMARY, 1)])],
        );
        let routed = route(
            &mut router,
            vec![PointerEvent::CaptureLost {
                pointer: PointerId(1),
            }],
        );
        assert_eq!(routed.canvas, [CanvasInput::Cancel]);
        let routed = route(
            &mut router,
            vec![samples(1, vec![sample(150.0, Buttons::empty(), 2)])],
        );
        assert!(routed.canvas.is_empty());
    }

    #[test]
    fn oldest_input_time_survives_until_taken() {
        let mut router = InputRouter::default();
        route(
            &mut router,
            vec![samples(1, vec![sample(1.0, Buttons::empty(), 7)])],
        );
        route(
            &mut router,
            vec![samples(1, vec![sample(2.0, Buttons::empty(), 9)])],
        );
        assert_eq!(router.take_oldest_unpresented(), Some(Ticks(7)));
        assert_eq!(router.take_oldest_unpresented(), None);
    }
}

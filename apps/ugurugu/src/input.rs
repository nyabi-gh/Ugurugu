// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! Splits Win32 pointer input between egui and the canvas.
//!
//! Every pointer message reaches the app only through `ugu_win::pointer`, so
//! egui and the canvas see the same samples and no synthesised mouse events.

use std::collections::HashMap;

use ugu_win::clock::Ticks;
use ugu_win::pointer::{Buttons, PointerEvent, PointerId, PointerKind, PointerSample};

const EGUI_BUTTONS: [(Buttons, egui::PointerButton); 5] = [
    (Buttons::PRIMARY, egui::PointerButton::Primary),
    (Buttons::SECONDARY, egui::PointerButton::Secondary),
    (Buttons::MIDDLE, egui::PointerButton::Middle),
    (Buttons::X1, egui::PointerButton::Extra1),
    (Buttons::X2, egui::PointerButton::Extra2),
];

/// What the canvas receives, in client physical pixels.
#[derive(Clone, Debug, PartialEq)]
pub enum CanvasInput {
    Begin(PointerKind, PointerSample),
    Extend(PointerSample),
    End(PointerSample),
    Cancel,
}

#[derive(Default)]
pub struct InputRouter {
    buttons: HashMap<PointerId, Buttons>,
    canvas_pointer: Option<PointerId>,
    oldest_unpresented: Option<Ticks>,
}

pub struct Routed {
    pub egui: Vec<egui::Event>,
    pub canvas: Vec<CanvasInput>,
}

impl InputRouter {
    /// `starts_on_canvas` decides, for a primary press at a physical position,
    /// whether the canvas owns the gesture instead of the UI.
    pub fn route(
        &mut self,
        events: Vec<PointerEvent>,
        pixels_per_point: f32,
        modifiers: egui::Modifiers,
        starts_on_canvas: impl Fn([f64; 2]) -> bool,
    ) -> Routed {
        let mut routed = Routed {
            egui: Vec::new(),
            canvas: Vec::new(),
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
                        let before = self.buttons.get(&pointer).copied().unwrap_or_default();
                        self.buttons.insert(pointer, sample.buttons);
                        let pressed = sample.buttons.difference(before);
                        let released = before.difference(sample.buttons);

                        if self.canvas_pointer == Some(pointer) {
                            if released.contains(Buttons::PRIMARY) {
                                self.canvas_pointer = None;
                                routed.canvas.push(CanvasInput::End(sample));
                            } else {
                                routed.canvas.push(CanvasInput::Extend(sample));
                            }
                            continue;
                        }
                        if self.canvas_pointer.is_none()
                            && pressed.contains(Buttons::PRIMARY)
                            && starts_on_canvas(sample.position)
                        {
                            self.canvas_pointer = Some(pointer);
                            routed.canvas.push(CanvasInput::Begin(kind, sample));
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
                PointerEvent::Leave { pointer } => {
                    if self.canvas_pointer != Some(pointer) {
                        routed.egui.push(egui::Event::PointerGone);
                    }
                }
                PointerEvent::CaptureLost { pointer } => {
                    self.buttons.remove(&pointer);
                    if self.canvas_pointer == Some(pointer) {
                        self.canvas_pointer = None;
                        routed.canvas.push(CanvasInput::Cancel);
                    }
                }
                PointerEvent::Wheel {
                    delta,
                    horizontal,
                    time,
                    ..
                } => {
                    self.oldest_unpresented.get_or_insert(time);
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
        router.route(events, 1.0, egui::Modifiers::NONE, |position| {
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
        assert!(matches!(routed.canvas[0], CanvasInput::Begin(..)));
        assert!(matches!(routed.canvas[1], CanvasInput::Extend(..)));
        assert!(matches!(routed.canvas[2], CanvasInput::End(..)));
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

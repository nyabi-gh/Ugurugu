// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! Pointer input from winit's window events, on the OSes where the app does
//! not read the OS's pointer messages itself. winit stamps no times, so a
//! sample has the time it reached the UI thread, and the mouse has no
//! pressure. These events go to `InputRouter` instead of egui-winit, as on
//! Windows, so egui and the canvas see the same samples.

use ugu_platform::clock::{self, Ticks};
use ugu_platform::pointer::{Buttons, PointerEvent, PointerId, PointerKind, PointerSample};
use winit::event::{ElementState, MouseButton, MouseScrollDelta, TouchPhase, WindowEvent};

const MOUSE: PointerId = PointerId(0);

#[derive(Default)]
pub struct WinitPointer {
    /// Where the mouse last was, in client physical pixels.
    position: Option<[f64; 2]>,
    buttons: Buttons,
    frame: u32,
}

impl WinitPointer {
    /// The pointer events `event` makes, or `None` when it is not pointer
    /// input and belongs to egui-winit.
    pub fn translate(&mut self, event: &WindowEvent) -> Option<Vec<PointerEvent>> {
        self.translate_at(event, clock::now())
    }

    fn translate_at(&mut self, event: &WindowEvent, time: Ticks) -> Option<Vec<PointerEvent>> {
        let events = match event {
            WindowEvent::CursorMoved { position, .. } => {
                self.position = Some([position.x, position.y]);
                self.mouse(time)
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let Some(button) = buttons_of(*button) else {
                    return Some(Vec::new());
                };
                self.buttons = match state {
                    ElementState::Pressed => self.buttons.union(button),
                    ElementState::Released => self.buttons.difference(button),
                };
                self.mouse(time)
            }
            WindowEvent::CursorLeft { .. } => {
                self.position = None;
                vec![PointerEvent::Leave { pointer: MOUSE }]
            }
            WindowEvent::CursorEntered { .. } => Vec::new(),
            WindowEvent::MouseWheel { delta, .. } => {
                let Some(position) = self.position else {
                    return Some(Vec::new());
                };
                let [x, y] = match *delta {
                    MouseScrollDelta::LineDelta(x, y) => [x, y],
                    MouseScrollDelta::PixelDelta(delta) => {
                        return Some(vec![PointerEvent::Scroll {
                            position,
                            delta: [delta.x, delta.y],
                            time,
                        }]);
                    }
                };
                [(y, false), (x, true)]
                    .into_iter()
                    .filter(|&(delta, _)| delta != 0.0)
                    .map(|(delta, horizontal)| PointerEvent::Wheel {
                        position,
                        delta,
                        horizontal,
                        time,
                    })
                    .collect()
            }
            WindowEvent::PinchGesture { delta, .. } => match self.position {
                Some(position) => vec![PointerEvent::Magnify {
                    position,
                    delta: *delta,
                    time,
                }],
                None => Vec::new(),
            },
            // winit counts counterclockwise.
            WindowEvent::RotationGesture { delta, .. } => match self.position {
                Some(position) => vec![PointerEvent::Rotate {
                    position,
                    degrees: -f64::from(*delta),
                    time,
                }],
                None => Vec::new(),
            },
            WindowEvent::Touch(touch) => {
                // Ids are the OS's, unique among the fingers down; 0 is the
                // mouse's.
                let pointer = PointerId(1 + touch.id as u32);
                if touch.phase == TouchPhase::Cancelled {
                    return Some(vec![PointerEvent::CaptureLost { pointer }]);
                }
                let buttons = match touch.phase {
                    TouchPhase::Started | TouchPhase::Moved => Buttons::PRIMARY,
                    TouchPhase::Ended | TouchPhase::Cancelled => Buttons::empty(),
                };
                let sample = self.sample(
                    [touch.location.x, touch.location.y],
                    touch.force.map(|force| force.normalized() as f32),
                    buttons,
                    time,
                );
                vec![PointerEvent::Samples {
                    pointer,
                    kind: PointerKind::Touch,
                    samples: vec![sample],
                }]
            }
            _ => return None,
        };
        Some(events)
    }

    fn mouse(&mut self, time: Ticks) -> Vec<PointerEvent> {
        let Some(position) = self.position else {
            return Vec::new();
        };
        let sample = self.sample(position, None, self.buttons, time);
        vec![PointerEvent::Samples {
            pointer: MOUSE,
            kind: PointerKind::Mouse,
            samples: vec![sample],
        }]
    }

    fn sample(
        &mut self,
        position: [f64; 2],
        pressure: Option<f32>,
        buttons: Buttons,
        time: Ticks,
    ) -> PointerSample {
        self.frame = self.frame.wrapping_add(1);
        PointerSample {
            position,
            pressure,
            tilt: None,
            twist: None,
            buttons,
            in_contact: !buttons.is_empty(),
            inverted: false,
            time,
            frame_id: self.frame,
        }
    }
}

fn buttons_of(button: MouseButton) -> Option<Buttons> {
    Some(match button {
        MouseButton::Left => Buttons::PRIMARY,
        MouseButton::Right => Buttons::SECONDARY,
        MouseButton::Middle => Buttons::MIDDLE,
        MouseButton::Back => Buttons::X1,
        MouseButton::Forward => Buttons::X2,
        MouseButton::Other(_) => return None,
    })
}

#[cfg(test)]
mod tests {
    use winit::dpi::PhysicalPosition;
    use winit::event::DeviceId;

    use super::*;

    fn moved(x: f64, y: f64) -> WindowEvent {
        WindowEvent::CursorMoved {
            device_id: DeviceId::dummy(),
            position: PhysicalPosition::new(x, y),
        }
    }

    fn button(state: ElementState, button: MouseButton) -> WindowEvent {
        WindowEvent::MouseInput {
            device_id: DeviceId::dummy(),
            state,
            button,
        }
    }

    fn mouse_samples(events: Vec<PointerEvent>) -> Vec<([f64; 2], Buttons)> {
        events
            .into_iter()
            .flat_map(|event| match event {
                PointerEvent::Samples {
                    pointer: MOUSE,
                    kind: PointerKind::Mouse,
                    samples,
                } => samples,
                other => panic!("not a mouse sample: {other:?}"),
            })
            .map(|sample| (sample.position, sample.buttons))
            .collect()
    }

    #[test]
    fn a_press_drag_and_release_are_mouse_samples_where_the_cursor_is() {
        let mut pointer = WinitPointer::default();
        let mut take = |event| pointer.translate_at(&event, Ticks(1)).unwrap();
        assert_eq!(
            mouse_samples(take(moved(10.0, 20.0))),
            [([10.0, 20.0], Buttons::empty())]
        );
        assert_eq!(
            mouse_samples(take(button(ElementState::Pressed, MouseButton::Left))),
            [([10.0, 20.0], Buttons::PRIMARY)]
        );
        assert_eq!(
            mouse_samples(take(moved(30.0, 25.0))),
            [([30.0, 25.0], Buttons::PRIMARY)]
        );
        assert_eq!(
            mouse_samples(take(button(ElementState::Pressed, MouseButton::Middle))),
            [([30.0, 25.0], Buttons::PRIMARY.union(Buttons::MIDDLE))]
        );
        assert_eq!(
            mouse_samples(take(button(ElementState::Released, MouseButton::Left))),
            [([30.0, 25.0], Buttons::MIDDLE)]
        );
    }

    #[test]
    fn nothing_is_sampled_before_the_cursor_is_known() {
        let mut pointer = WinitPointer::default();
        let pressed = button(ElementState::Pressed, MouseButton::Left);
        assert_eq!(pointer.translate_at(&pressed, Ticks(1)), Some(Vec::new()));
        // The button stays down for the first position.
        assert_eq!(
            mouse_samples(pointer.translate_at(&moved(5.0, 5.0), Ticks(2)).unwrap()),
            [([5.0, 5.0], Buttons::PRIMARY)]
        );
    }

    #[test]
    fn the_wheel_and_touchpad_act_where_the_cursor_is_and_leaving_forgets_it() {
        let mut pointer = WinitPointer::default();
        pointer.translate_at(&moved(7.0, 8.0), Ticks(1));
        let wheel = |delta| WindowEvent::MouseWheel {
            device_id: DeviceId::dummy(),
            delta,
            phase: TouchPhase::Moved,
        };
        assert_eq!(
            pointer.translate_at(&wheel(MouseScrollDelta::LineDelta(0.5, -2.0)), Ticks(2)),
            Some(vec![
                PointerEvent::Wheel {
                    position: [7.0, 8.0],
                    delta: -2.0,
                    horizontal: false,
                    time: Ticks(2),
                },
                PointerEvent::Wheel {
                    position: [7.0, 8.0],
                    delta: 0.5,
                    horizontal: true,
                    time: Ticks(2),
                },
            ])
        );
        assert_eq!(
            pointer.translate_at(
                &wheel(MouseScrollDelta::PixelDelta(PhysicalPosition::new(
                    3.0, -4.0
                ))),
                Ticks(3)
            ),
            Some(vec![PointerEvent::Scroll {
                position: [7.0, 8.0],
                delta: [3.0, -4.0],
                time: Ticks(3),
            }])
        );
        let pinch = WindowEvent::PinchGesture {
            device_id: DeviceId::dummy(),
            delta: 0.25,
            phase: TouchPhase::Moved,
        };
        assert_eq!(
            pointer.translate_at(&pinch, Ticks(4)),
            Some(vec![PointerEvent::Magnify {
                position: [7.0, 8.0],
                delta: 0.25,
                time: Ticks(4),
            }])
        );
        let turn = WindowEvent::RotationGesture {
            device_id: DeviceId::dummy(),
            delta: 10.0,
            phase: TouchPhase::Moved,
        };
        assert_eq!(
            pointer.translate_at(&turn, Ticks(5)),
            Some(vec![PointerEvent::Rotate {
                position: [7.0, 8.0],
                degrees: -10.0,
                time: Ticks(5),
            }]),
            "counterclockwise in winit"
        );
        let left = WindowEvent::CursorLeft {
            device_id: DeviceId::dummy(),
        };
        assert_eq!(
            pointer.translate_at(&left, Ticks(6)),
            Some(vec![PointerEvent::Leave { pointer: MOUSE }])
        );
        assert_eq!(
            pointer.translate_at(&wheel(MouseScrollDelta::LineDelta(0.0, 1.0)), Ticks(7)),
            Some(Vec::new())
        );
    }

    #[test]
    fn other_window_events_stay_with_egui() {
        let mut pointer = WinitPointer::default();
        assert_eq!(
            pointer.translate_at(&WindowEvent::Focused(true), Ticks(1)),
            None
        );
    }
}

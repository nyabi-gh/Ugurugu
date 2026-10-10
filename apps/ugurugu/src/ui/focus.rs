// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! Where the focus goes when menus, lists and dialogs open and close, as in
//! Windows: a dialog opened from the keyboard takes it, and closing one from
//! the keyboard gives it back to the control that opened it. egui leaves it
//! where it was, under a dialog, or on nothing, and the next Tab started
//! over at the menu bar.

use egui::{Context, FocusDirection, Id, Order, Popup};

#[derive(Default)]
pub struct Return {
    /// The control under the open menu, list or dialog that had the focus
    /// last: the one that opened it, or the menu title moved to.
    opener: Option<Id>,
    open: bool,
    /// Whether the last frame's input was the keyboard's.
    keyboard: bool,
}

impl Return {
    /// Before the widgets of a frame.
    pub fn begin(&mut self, ctx: &Context) {
        let modal = ctx.memory(|memory| memory.top_modal_layer());
        let open = modal.is_some() || Popup::is_any_open(ctx);
        let focused = ctx.memory(|memory| memory.focused());
        let layer = focused
            .and_then(|id| ctx.read_response(id))
            .map(|response| response.layer_id);
        let beneath = layer.is_some_and(|layer| layer.order != Order::Foreground);
        if open {
            if !self.open {
                self.opener = None;
            }
            if beneath {
                self.opener = focused;
            }
            // Nothing in the dialog has the focus: nothing does, or what is
            // under it.
            let outside = match layer {
                Some(layer) => !ctx.memory(|memory| memory.is_above_modal_layer(layer)),
                None => true,
            };
            if modal.is_some() && outside {
                ctx.memory_mut(|memory| {
                    if let Some(id) = focused {
                        memory.surrender_focus(id);
                    }
                    if self.keyboard {
                        memory.move_focus(FocusDirection::Next);
                    }
                });
            }
        } else if self.open {
            // Closed with the pointer, the focus stays where it is.
            if self.keyboard
                && let Some(opener) = self.opener
            {
                ctx.memory_mut(|memory| memory.request_focus(opener));
            }
            self.opener = None;
        }
        self.open = open;
        let mut key = false;
        ctx.input(|input| {
            for event in &input.events {
                match event {
                    egui::Event::Key { pressed: true, .. } => key = true,
                    egui::Event::PointerButton { pressed: true, .. } => self.keyboard = false,
                    _ => {}
                }
            }
        });
        if key {
            self.keyboard = true;
            // A key that closes a dialog shows only next frame that it has,
            // and the app draws no frame without input.
            if open {
                ctx.request_repaint();
            }
        }
    }
}

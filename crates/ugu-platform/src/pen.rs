// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! What the pens of the OSes without `WM_POINTER` share.

use ugu_platform_types::pointer::Buttons;

/// Keeps pen pointer IDs disjoint from `WinitPointer`'s mouse (`0`) and
/// low-numbered touch IDs (`1 + touch.id`).
pub(crate) const PEN_POINTER_BASE: u32 = 0x4000_0000;

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

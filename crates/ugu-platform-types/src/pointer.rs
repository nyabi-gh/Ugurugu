// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

use std::fmt;

use crate::clock::Ticks;

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
pub struct PointerError(pub &'static str);

impl fmt::Display for PointerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

impl std::error::Error for PointerError {}

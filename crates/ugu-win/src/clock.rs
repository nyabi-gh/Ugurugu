// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

use std::sync::OnceLock;

use windows_sys::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};

/// A `QueryPerformanceCounter` reading, the clock pointer input is stamped with.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Ticks(pub u64);

impl Ticks {
    pub fn now() -> Self {
        let mut value = 0i64;
        // SAFETY: the pointer is valid; the call cannot fail on Windows XP or later.
        unsafe { QueryPerformanceCounter(&mut value) };
        Self(value as u64)
    }

    pub fn minus_millis(self, millis: u32) -> Self {
        Self(
            self.0
                .saturating_sub(u64::from(millis) * frequency() / 1000),
        )
    }

    pub fn seconds_since(self, earlier: Ticks) -> f64 {
        self.0.saturating_sub(earlier.0) as f64 / frequency() as f64
    }
}

fn frequency() -> u64 {
    static FREQUENCY: OnceLock<u64> = OnceLock::new();
    *FREQUENCY.get_or_init(|| {
        let mut value = 0i64;
        // SAFETY: the pointer is valid; the call cannot fail on Windows XP or later.
        unsafe { QueryPerformanceFrequency(&mut value) };
        value.max(1) as u64
    })
}

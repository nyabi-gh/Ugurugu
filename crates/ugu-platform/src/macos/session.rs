// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! Hears that macOS wants the app gone: the user logging out, restarting or
//! shutting down, or choosing Quit in the Dock.

use super::requests;

/// Calls back as the session ends, until dropped.
pub struct SessionEnd;

impl SessionEnd {
    /// `ending` is called on a thread of its own when the session ends, and
    /// the process ends once it returns.
    ///
    /// # Safety
    /// None; unsafe to match the OSes that have it.
    pub unsafe fn install(
        _hwnd: isize,
        ending: impl FnMut() + Send + 'static,
    ) -> Result<Self, &'static str> {
        requests::set_ending(Some(Box::new(ending)));
        Ok(Self)
    }
}

impl Drop for SessionEnd {
    fn drop(&mut self) {
        requests::set_ending(None);
    }
}

/// Calls back on the main thread when the user asks the app to quit from
/// outside its window, until dropped.
pub struct QuitRequests;

impl QuitRequests {
    pub fn install(quit: impl FnMut() + 'static) -> Self {
        requests::set_quit(Some(Box::new(quit)));
        Self
    }
}

impl Drop for QuitRequests {
    fn drop(&mut self) {
        requests::set_quit(None);
    }
}

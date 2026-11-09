// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! One running app, which macOS sees to: LaunchServices starts no second
//! one and sends the documents to the first.

use std::path::PathBuf;

use objc2::MainThreadMarker;
use objc2_app_kit::{NSApplication, NSView};

use super::requests;

pub use crate::unsupported::instance::{StartLock, hand_over};

/// Shows the window even when it was minimized, for what was just asked of
/// it: nothing is drawn while it is.
///
/// # Safety
/// On the main thread, with `view` the live view of the app's window.
pub unsafe fn bring_to_front(view: isize) {
    let mtm = MainThreadMarker::new().expect("the caller is on the main thread");
    // SAFETY: as the caller promises.
    let view = unsafe { &*(view as *const NSView) };
    if let Some(window) = view.window() {
        if window.isMiniaturized() {
            window.deminiaturize(None);
        }
        window.makeKeyAndOrderFront(None);
    }
    NSApplication::sharedApplication(mtm).activate();
}

/// Gives the documents macOS asks to open to a callback, until dropped.
pub struct DocumentReceiver;

impl DocumentReceiver {
    /// `receive` is called on the main thread, at once for any document
    /// that came before it.
    ///
    /// # Safety
    /// None; unsafe to match the OSes that have it.
    pub unsafe fn install(
        _hwnd: isize,
        _key: &str,
        receive: impl FnMut(Option<PathBuf>) -> bool + 'static,
    ) -> Result<Self, &'static str> {
        requests::receive_documents(Box::new(receive));
        Ok(Self)
    }
}

impl Drop for DocumentReceiver {
    fn drop(&mut self) {
        requests::stop_receiving_documents();
    }
}

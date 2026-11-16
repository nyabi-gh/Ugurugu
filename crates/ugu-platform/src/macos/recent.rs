// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

use std::path::Path;

use dispatch2::DispatchQueue;
use objc2::MainThreadMarker;
use objc2_app_kit::NSDocumentController;
use objc2_foundation::{NSString, NSURL};

/// Lists `path` among the app's recent documents, which the Dock menu shows
/// and opens through `application:openURLs:`.
pub fn add(path: &Path) {
    // APFS and HFS+ names are UTF-8.
    let Some(path) = path.to_str().map(str::to_owned) else {
        return;
    };
    // NSDocumentController is main-thread only; the caller is the file thread.
    DispatchQueue::main().exec_async(move || {
        let mtm = MainThreadMarker::new().expect("the main queue runs on the main thread");
        let url = NSURL::fileURLWithPath(&NSString::from_str(&path));
        NSDocumentController::sharedDocumentController(mtm).noteNewRecentDocumentURL(&url);
    });
}

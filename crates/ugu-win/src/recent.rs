// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! The documents recently opened or saved, which the taskbar's Jump List
//! shows for the program the file type is registered to.

use std::os::windows::ffi::OsStrExt;
use std::path::Path;

use windows_sys::Win32::UI::Shell::{SHARD_PATHW, SHAddToRecentDocs};

/// Adds `path` to the documents Windows lists as used recently.
pub fn add(path: &Path) {
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
    // SAFETY: `wide` is a NUL-terminated path that outlives the call.
    unsafe { SHAddToRecentDocs(SHARD_PATHW as u32, wide.as_ptr().cast()) };
}

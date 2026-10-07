// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

use windows_sys::Win32::Globalization::{GetUserPreferredUILanguages, MUI_LANGUAGE_NAME};

/// The user's display languages in order of preference, such as "ko-KR",
/// as Qt's `QLocale::system().uiLanguages()` reads them.
pub fn preferred_ui_languages() -> Vec<String> {
    let (mut count, mut length) = (0u32, 0u32);
    // SAFETY: a null buffer asks for the length only.
    let sized = unsafe {
        GetUserPreferredUILanguages(
            MUI_LANGUAGE_NAME,
            &mut count,
            std::ptr::null_mut(),
            &mut length,
        )
    };
    if sized == 0 || length == 0 {
        return Vec::new();
    }
    let mut buffer = vec![0u16; length as usize];
    // SAFETY: the buffer holds the `length` characters asked for.
    let filled = unsafe {
        GetUserPreferredUILanguages(
            MUI_LANGUAGE_NAME,
            &mut count,
            buffer.as_mut_ptr(),
            &mut length,
        )
    };
    if filled == 0 {
        return Vec::new();
    }
    // A list of names, each ending in a null, ended by an empty name.
    buffer
        .split(|&unit| unit == 0)
        .filter(|name| !name.is_empty())
        .map(String::from_utf16_lossy)
        .collect()
}

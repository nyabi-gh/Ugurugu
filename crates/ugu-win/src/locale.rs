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

/// The Windows version as `Windows 10.0.<build>.<revision> (<release>)`,
/// read from the registry, since `GetVersionEx` reports the version the app
/// was manifested for.
pub fn os_version() -> String {
    use windows_sys::Win32::System::Registry::{
        HKEY_LOCAL_MACHINE, RRF_RT_REG_DWORD, RRF_RT_REG_SZ, RegGetValueW,
    };
    let wide = |text: &str| text.encode_utf16().chain([0]).collect::<Vec<u16>>();
    let key = wide(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion");
    let text = |name: &str| {
        let name = wide(name);
        let mut buffer = [0u16; 64];
        let mut bytes = (buffer.len() * 2) as u32;
        // SAFETY: the strings are null-terminated and the buffer's size is
        // given in bytes.
        let status = unsafe {
            RegGetValueW(
                HKEY_LOCAL_MACHINE,
                key.as_ptr(),
                name.as_ptr(),
                RRF_RT_REG_SZ,
                std::ptr::null_mut(),
                buffer.as_mut_ptr().cast(),
                &mut bytes,
            )
        };
        let length = (bytes as usize / 2).saturating_sub(1).min(buffer.len());
        (status == 0).then(|| String::from_utf16_lossy(&buffer[..length]))
    };
    let revision = {
        let name = wide("UBR");
        let mut value = 0u32;
        let mut bytes = 4u32;
        // SAFETY: as above, with a DWORD.
        let status = unsafe {
            RegGetValueW(
                HKEY_LOCAL_MACHINE,
                key.as_ptr(),
                name.as_ptr(),
                RRF_RT_REG_DWORD,
                std::ptr::null_mut(),
                (&mut value as *mut u32).cast(),
                &mut bytes,
            )
        };
        (status == 0).then_some(value)
    };
    let build = text("CurrentBuild").unwrap_or_else(|| "?".to_owned());
    let release = text("DisplayVersion").unwrap_or_default();
    match revision {
        Some(revision) => format!("Windows 10.0.{build}.{revision} ({release})"),
        None => format!("Windows 10.0.{build} ({release})"),
    }
}

#[cfg(test)]
mod version_tests {
    #[test]
    fn the_version_names_a_build() {
        let version = super::os_version();
        assert!(
            version.starts_with("Windows 10.0.") && !version.starts_with("Windows 10.0.?"),
            "{version}"
        );
    }
}

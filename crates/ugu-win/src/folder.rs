// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! The user's shell folders.

use std::path::PathBuf;

use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::UI::Shell::{
    FOLDERID_Documents, FOLDERID_LocalAppData, FOLDERID_RoamingAppData, KF_FLAG_DEFAULT,
    SHGetKnownFolderPath,
};
use windows::core::GUID;

fn known(id: &GUID) -> Option<PathBuf> {
    // SAFETY: the returned string is freed once copied, even on failure.
    unsafe {
        let text = SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, None).ok()?;
        let path = text.to_string();
        CoTaskMemFree(Some(text.0 as *const _));
        path.ok().map(PathBuf::from)
    }
}

/// `%APPDATA%`, which roams with the user's profile.
pub fn roaming_app_data() -> Option<PathBuf> {
    known(&FOLDERID_RoamingAppData)
}

/// `%LOCALAPPDATA%`, which stays on this PC.
pub fn local_app_data() -> Option<PathBuf> {
    known(&FOLDERID_LocalAppData)
}

/// The user's Documents folder, wherever it was moved.
pub fn documents() -> Option<PathBuf> {
    known(&FOLDERID_Documents)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_folders_exist() {
        for folder in [roaming_app_data(), local_app_data(), documents()] {
            let folder = folder.expect("a known folder");
            assert!(folder.is_absolute() && folder.is_dir(), "{folder:?}");
        }
    }
}

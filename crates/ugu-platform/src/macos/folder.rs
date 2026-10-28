// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! The user's folders as Foundation names them.

use std::path::{Path, PathBuf};

use objc2_foundation::{NSFileManager, NSSearchPathDirectory, NSSearchPathDomainMask};

fn user(directory: NSSearchPathDirectory) -> Option<PathBuf> {
    NSFileManager::defaultManager()
        .URLsForDirectory_inDomains(directory, NSSearchPathDomainMask::UserDomainMask)
        .firstObject()?
        .to_file_path()
}

/// `~/Library/Application Support`. macOS has no folder that roams apart
/// from one that stays, so this is also [`local_app_data`].
pub fn roaming_app_data() -> Option<PathBuf> {
    user(NSSearchPathDirectory::ApplicationSupportDirectory)
}

pub fn local_app_data() -> Option<PathBuf> {
    user(NSSearchPathDirectory::ApplicationSupportDirectory)
}

pub fn documents() -> Option<PathBuf> {
    user(NSSearchPathDirectory::DocumentDirectory)
}

/// Opens `folder` in the Finder.
pub fn show(folder: &Path) -> Result<(), String> {
    let status = std::process::Command::new("/usr/bin/open")
        .arg(folder)
        .status()
        .map_err(|error| error.to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("cannot open {}: {status}", folder.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_folders_exist() {
        for folder in [roaming_app_data(), local_app_data(), documents()] {
            let folder = folder.expect("a user folder");
            assert!(folder.is_absolute() && folder.is_dir(), "{folder:?}");
        }
    }
}

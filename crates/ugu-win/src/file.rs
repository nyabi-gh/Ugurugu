// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! Putting a finished file in place of another.

use std::io;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;

use windows_sys::Win32::Storage::FileSystem::{
    MOVEFILE_WRITE_THROUGH, MoveFileExW, REPLACEFILE_IGNORE_MERGE_ERRORS, ReplaceFileW,
};

fn wide(path: &Path) -> Vec<u16> {
    path.as_os_str().encode_wide().chain(Some(0)).collect()
}

/// Moves `replacement` to `target`. An existing target is replaced with
/// `ReplaceFileW`, which keeps its attributes and security; a new one is a
/// write-through move that fails if the target appeared meanwhile.
///
/// On error the target keeps its old content. `ReplaceFileW` can fail after
/// renaming files only when given a backup name, and none is given.
pub fn replace_file(replacement: &Path, target: &Path) -> io::Result<()> {
    let replacement = wide(replacement);
    let target_wide = wide(target);
    // SAFETY: both strings are NUL-terminated and outlive the calls.
    let done = unsafe {
        if target.exists() {
            ReplaceFileW(
                target_wide.as_ptr(),
                replacement.as_ptr(),
                std::ptr::null(),
                REPLACEFILE_IGNORE_MERGE_ERRORS,
                std::ptr::null(),
                std::ptr::null(),
            )
        } else {
            MoveFileExW(
                replacement.as_ptr(),
                target_wide.as_ptr(),
                MOVEFILE_WRITE_THROUGH,
            )
        }
    };
    if done == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::windows::fs::OpenOptionsExt;

    struct Folder(std::path::PathBuf);

    impl Folder {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!("ugu-win-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn file(&self, name: &str, content: &[u8]) -> std::path::PathBuf {
            let path = self.0.join(name);
            std::fs::write(&path, content).unwrap();
            path
        }
    }

    impl Drop for Folder {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn an_existing_file_is_replaced_and_a_new_one_created() {
        let folder = Folder::new("replace");
        let target = folder.file("doc.ugu2", b"old");
        let replacement = folder.file("doc.tmp", b"new");
        replace_file(&replacement, &target).unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"new");
        assert!(!replacement.exists());

        let fresh = folder.0.join("fresh.ugu2");
        let replacement = folder.file("fresh.tmp", b"first");
        replace_file(&replacement, &fresh).unwrap();
        assert_eq!(std::fs::read(&fresh).unwrap(), b"first");
    }

    #[test]
    #[expect(
        clippy::permissions_set_readonly_false,
        reason = "Windows only: this clears the read-only attribute so the folder can be removed"
    )]
    fn a_read_only_target_is_left_as_it_was() {
        let folder = Folder::new("readonly");
        let target = folder.file("doc.ugu2", b"old");
        let mut permissions = std::fs::metadata(&target).unwrap().permissions();
        permissions.set_readonly(true);
        std::fs::set_permissions(&target, permissions.clone()).unwrap();
        let replacement = folder.file("doc.tmp", b"new");
        assert!(replace_file(&replacement, &target).is_err());
        assert_eq!(std::fs::read(&target).unwrap(), b"old");
        permissions.set_readonly(false);
        std::fs::set_permissions(&target, permissions).unwrap();
    }

    #[test]
    fn a_target_open_without_sharing_is_left_as_it_was() {
        let folder = Folder::new("locked");
        let target = folder.file("doc.ugu2", b"old");
        let replacement = folder.file("doc.tmp", b"new");
        // Another program holding the file open, sharing nothing.
        let held = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&target)
            .unwrap();
        let error = replace_file(&replacement, &target).unwrap_err();
        drop(held);
        assert!(error.raw_os_error().is_some(), "{error}");
        assert_eq!(std::fs::read(&target).unwrap(), b"old");
        assert_eq!(std::fs::read(&replacement).unwrap(), b"new");
    }
}

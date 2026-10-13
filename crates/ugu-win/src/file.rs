// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! Putting a finished file in place of another.

use std::io;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;

use windows::Win32::Foundation::{CloseHandle, ERROR_INVALID_PARAMETER, STILL_ACTIVE};
use windows::Win32::System::Threading::{
    GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows_sys::Win32::Foundation::ERROR_UNABLE_TO_MOVE_REPLACEMENT_2;
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
/// On error the target keeps its old content and `replacement` is left in
/// place. The old file passes through a backup beside `replacement`: without
/// one, `ReplaceFileW` can fail after the old file is gone.
pub fn replace_file(replacement: &Path, target: &Path) -> io::Result<()> {
    if !target.exists() {
        return move_file(replacement, target);
    }
    let backup = replacement.with_extension("old");
    let (replacement_wide, target_wide, backup_wide) =
        (wide(replacement), wide(target), wide(&backup));
    // SAFETY: all strings are NUL-terminated and outlive the call.
    let replaced = unsafe {
        ReplaceFileW(
            target_wide.as_ptr(),
            replacement_wide.as_ptr(),
            backup_wide.as_ptr(),
            REPLACEFILE_IGNORE_MERGE_ERRORS,
            std::ptr::null(),
            std::ptr::null(),
        )
    };
    if replaced != 0 {
        let _ = std::fs::remove_file(&backup);
        return Ok(());
    }
    let error = io::Error::last_os_error();
    // The old file is the backup now and nothing is at `target`.
    if error.raw_os_error() == Some(ERROR_UNABLE_TO_MOVE_REPLACEMENT_2 as i32)
        && move_file(&backup, target).is_err()
    {
        return Err(io::Error::other(format!(
            "{error}; the old file is kept as {}",
            backup.display()
        )));
    }
    Err(error)
}

/// A write-through move that fails if `to` exists.
fn move_file(from: &Path, to: &Path) -> io::Result<()> {
    let (from, to) = (wide(from), wide(to));
    // SAFETY: both strings are NUL-terminated and outlive the call.
    if unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), MOVEFILE_WRITE_THROUGH) } == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// Removes the temporary files that processes no longer running left
/// beside `target` (`ugu_io::save::others_temporaries`), and says how many.
pub fn remove_stale_temporaries(others: Vec<(std::path::PathBuf, u32)>) -> usize {
    others
        .into_iter()
        .filter(|(path, pid)| !running(*pid) && std::fs::remove_file(path).is_ok())
        .count()
}

/// Whether process `pid` is still running. One that cannot be looked at
/// counts as running, so its files are left alone.
pub fn running(pid: u32) -> bool {
    // SAFETY: the handle is used only while open and closed after.
    unsafe {
        let process = match OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
            Ok(process) => process,
            Err(error) => return error.code() != ERROR_INVALID_PARAMETER.to_hresult(),
        };
        let mut code = 0;
        let looked = GetExitCodeProcess(process, &mut code);
        let _ = CloseHandle(process);
        looked.is_err() || code == STILL_ACTIVE.0 as u32
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::windows::fs::OpenOptionsExt;

    #[test]
    fn this_process_runs_and_an_ended_one_does_not() {
        assert!(running(std::process::id()));
        let mut child = std::process::Command::new("cmd")
            .args(["/c", "exit"])
            .spawn()
            .unwrap();
        let pid = child.id();
        child.wait().unwrap();
        // Its id may be taken again later, but not this soon.
        assert!(!running(pid));
    }

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
        let target = folder.file("doc.ugurugu", b"old");
        let replacement = folder.file("doc.tmp", b"new");
        replace_file(&replacement, &target).unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"new");
        assert!(!replacement.exists());
        assert!(!replacement.with_extension("old").exists());

        let fresh = folder.0.join("fresh.ugurugu");
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
        let target = folder.file("doc.ugurugu", b"old");
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
        let target = folder.file("doc.ugurugu", b"old");
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

    #[test]
    fn a_replacement_that_cannot_move_leaves_both_files() {
        let folder = Folder::new("held");
        let target = folder.file("doc.ugurugu", b"old");
        let replacement = folder.file("doc.tmp", b"new");
        // A scanner reading the new file, not letting it be renamed.
        let held = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(&replacement)
            .unwrap();
        let failed = replace_file(&replacement, &target).is_err();
        drop(held);
        assert!(failed);
        assert_eq!(std::fs::read(&target).unwrap(), b"old");
        assert_eq!(std::fs::read(&replacement).unwrap(), b"new");
        assert!(!replacement.with_extension("old").exists());
    }
}

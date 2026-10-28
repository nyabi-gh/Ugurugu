// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! What macOS and Linux do alike.

pub mod clock {
    use std::ffi::{c_char, c_int, c_long};

    /// `struct tm` of `<time.h>`, as macOS and glibc lay it out.
    #[repr(C)]
    struct Tm {
        second: c_int,
        minute: c_int,
        hour: c_int,
        day: c_int,
        month: c_int,
        year: c_int,
        weekday: c_int,
        year_day: c_int,
        daylight_saving: c_int,
        offset: c_long,
        zone: *const c_char,
    }

    unsafe extern "C" {
        fn time(out: *mut i64) -> i64;
        fn localtime_r(time: *const i64, out: *mut Tm) -> *mut Tm;
    }

    /// The local date and time as `YYYYMMDD-hhmmss`.
    pub fn local_stamp() -> Option<String> {
        // SAFETY: all-zero is a valid `tm`, and both pointers are to live
        // values for the calls.
        let mut now = unsafe { std::mem::zeroed::<Tm>() };
        if unsafe { localtime_r(&time(std::ptr::null_mut()), &mut now) }.is_null() {
            return None;
        }
        Some(format!(
            "{:04}{:02}{:02}-{:02}{:02}{:02}",
            now.year + 1900,
            now.month + 1,
            now.day,
            now.hour,
            now.minute,
            now.second
        ))
    }

    #[cfg(test)]
    mod tests {
        #[test]
        fn the_stamp_is_a_date_and_a_time() {
            let stamp = super::local_stamp().unwrap();
            let (date, time) = stamp.split_once('-').unwrap();
            assert!(date.len() == 8 && date.starts_with("20"), "{stamp}");
            assert!(
                time.len() == 6 && time.bytes().all(|b| b.is_ascii_digit()),
                "{stamp}"
            );
        }
    }
}

pub mod file {
    //! Putting a finished file in place of another.

    use std::ffi::{CString, c_char, c_int};
    use std::fs::File;
    use std::io;
    use std::os::unix::ffi::OsStrExt;
    use std::path::{Path, PathBuf};

    unsafe extern "C" {
        fn kill(pid: c_int, signal: c_int) -> c_int;
    }

    /// Moves `replacement`, already flushed to disk, to `target`. An existing
    /// target is replaced in one rename and its permissions are kept (on
    /// macOS its ACL and extended attributes too); a new one is a move that
    /// fails if the target appeared meanwhile. The folder is then flushed so
    /// that the rename survives a power cut.
    ///
    /// On error the target keeps its old content and `replacement` is left in
    /// place.
    pub fn replace_file(replacement: &Path, target: &Path) -> io::Result<()> {
        match std::fs::metadata(target) {
            Ok(old) => {
                std::fs::set_permissions(replacement, old.permissions())?;
                #[cfg(target_os = "macos")]
                copy_acl_and_attributes(target, replacement)?;
                std::fs::rename(replacement, target)?;
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                rename_new(replacement, target)?;
            }
            Err(error) => return Err(error),
        }
        let folder = match target.parent() {
            Some(folder) if !folder.as_os_str().is_empty() => folder,
            _ => Path::new("."),
        };
        File::open(folder)?.sync_all()
    }

    fn c_path(path: &Path) -> io::Result<CString> {
        CString::new(path.as_os_str().as_bytes())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "a path holds a NUL"))
    }

    /// Without the old file's modes and times: those of the new one are its
    /// own (`COPYFILE_STAT` would give it the old modification time).
    #[cfg(target_os = "macos")]
    fn copy_acl_and_attributes(from: &Path, to: &Path) -> io::Result<()> {
        use std::os::fd::AsRawFd;

        const COPYFILE_ACL: u32 = 1 << 0;
        const COPYFILE_XATTR: u32 = 1 << 2;
        unsafe extern "C" {
            fn fcopyfile(from: c_int, to: c_int, state: *mut std::ffi::c_void, flags: u32)
            -> c_int;
        }
        let (from, to) = (File::open(from)?, File::options().write(true).open(to)?);
        // SAFETY: both descriptors are open for the call; a null state is
        // allowed.
        if unsafe {
            fcopyfile(
                from.as_raw_fd(),
                to.as_raw_fd(),
                std::ptr::null_mut(),
                COPYFILE_ACL | COPYFILE_XATTR,
            )
        } != 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    /// A rename that fails if `to` exists.
    #[cfg(target_os = "macos")]
    fn rename_new(from: &Path, to: &Path) -> io::Result<()> {
        const RENAME_EXCL: u32 = 0x4;
        unsafe extern "C" {
            fn renamex_np(from: *const c_char, to: *const c_char, flags: u32) -> c_int;
        }
        let (from, to) = (c_path(from)?, c_path(to)?);
        // SAFETY: both strings are NUL-terminated and outlive the call.
        if unsafe { renamex_np(from.as_ptr(), to.as_ptr(), RENAME_EXCL) } != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    /// A rename that fails if `to` exists.
    #[cfg(not(target_os = "macos"))]
    fn rename_new(from: &Path, to: &Path) -> io::Result<()> {
        const AT_FDCWD: c_int = -100;
        const RENAME_NOREPLACE: std::ffi::c_uint = 1;
        unsafe extern "C" {
            fn renameat2(
                from_dir: c_int,
                from: *const c_char,
                to_dir: c_int,
                to: *const c_char,
                flags: std::ffi::c_uint,
            ) -> c_int;
        }
        let (from, to) = (c_path(from)?, c_path(to)?);
        // SAFETY: both strings are NUL-terminated and outlive the call.
        if unsafe {
            renameat2(
                AT_FDCWD,
                from.as_ptr(),
                AT_FDCWD,
                to.as_ptr(),
                RENAME_NOREPLACE,
            )
        } != 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    /// Removes the temporary files that processes no longer running left
    /// beside `target` (`ugu_io::save::others_temporaries`), and says how many.
    pub fn remove_stale_temporaries(others: Vec<(PathBuf, u32)>) -> usize {
        others
            .into_iter()
            .filter(|(path, pid)| !running(*pid) && std::fs::remove_file(path).is_ok())
            .count()
    }

    /// Whether process `pid` is still running. One that cannot be looked at
    /// counts as running, so its files are left alone.
    pub fn running(pid: u32) -> bool {
        const ESRCH: i32 = 3;
        let Ok(pid) = c_int::try_from(pid) else {
            return true;
        };
        // SAFETY: signal 0 only checks that the process exists.
        let signalled = unsafe { kill(pid, 0) } == 0;
        signalled || io::Error::last_os_error().raw_os_error() != Some(ESRCH)
    }

    #[cfg(test)]
    mod tests {
        use std::os::unix::fs::PermissionsExt;

        use super::*;

        struct Folder(PathBuf);

        impl Folder {
            fn new(name: &str) -> Self {
                let path = std::env::temp_dir()
                    .join(format!("ugu-platform-{name}-{}", std::process::id()));
                let _ = std::fs::remove_dir_all(&path);
                std::fs::create_dir_all(&path).unwrap();
                Self(path)
            }
        }

        impl Drop for Folder {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }

        #[test]
        fn a_replaced_file_keeps_its_permissions_and_takes_the_new_content() {
            let folder = Folder::new("replace");
            let (target, new) = (folder.0.join("a.ugurugu"), folder.0.join("a.tmp"));
            std::fs::write(&target, "old").unwrap();
            std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o600)).unwrap();
            std::fs::write(&new, "new").unwrap();
            replace_file(&new, &target).unwrap();
            assert_eq!(std::fs::read_to_string(&target).unwrap(), "new");
            let mode = std::fs::metadata(&target).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
            assert!(!new.exists());
        }

        #[test]
        fn a_new_target_is_made_but_one_that_appears_is_not_replaced() {
            let folder = Folder::new("new");
            let (target, new) = (folder.0.join("a.ugurugu"), folder.0.join("a.tmp"));
            std::fs::write(&new, "new").unwrap();
            rename_new(&new, &target).unwrap();
            assert_eq!(std::fs::read_to_string(&target).unwrap(), "new");
            std::fs::write(&new, "newer").unwrap();
            assert_eq!(
                rename_new(&new, &target).unwrap_err().kind(),
                io::ErrorKind::AlreadyExists
            );
            assert_eq!(std::fs::read_to_string(&target).unwrap(), "new");
            assert!(new.exists());
        }

        #[test]
        fn this_process_runs_and_a_finished_one_does_not() {
            assert!(running(std::process::id()));
            let mut child = std::process::Command::new("true").spawn().unwrap();
            let pid = child.id();
            child.wait().unwrap();
            assert!(!running(pid));
        }
    }
}

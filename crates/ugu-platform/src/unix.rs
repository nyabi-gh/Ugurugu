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

pub mod crash {
    //! A note of a crash that Rust cannot catch, such as a bad access in a
    //! driver, written from a signal handler. The handler runs in a broken
    //! process, so it allocates nothing and calls only functions that are
    //! safe in a signal handler: the file name and the heading are made
    //! beforehand. Unlike the Windows note it names no module: macOS's own
    //! crash report, in `~/Library/Logs/DiagnosticReports`, has the stack.

    use std::ffi::{CString, c_int, c_void};
    use std::os::unix::ffi::OsStrExt;
    use std::path::Path;
    use std::sync::OnceLock;
    use std::sync::atomic::{AtomicBool, Ordering};

    const SIGNALS: [(c_int, &str); 5] = [
        (libc::SIGSEGV, "SIGSEGV"),
        (libc::SIGBUS, "SIGBUS"),
        (libc::SIGILL, "SIGILL"),
        (libc::SIGFPE, "SIGFPE"),
        (libc::SIGABRT, "SIGABRT"),
    ];

    struct Note {
        path: CString,
        heading: Vec<u8>,
        /// The handlers there were before, by [`SIGNALS`]: Rust's own tells
        /// a stack overflow from other bad accesses.
        previous: [libc::sigaction; SIGNALS.len()],
    }

    static NOTE: OnceLock<Note> = OnceLock::new();
    /// A crash that leads to another, as a stack overflow leads to an
    /// abort, keeps the note of the first.
    static WRITTEN: AtomicBool = AtomicBool::new(false);

    /// From now on, a bad access, an illegal instruction, an arithmetic
    /// fault or an abort writes `path` with `heading` and then the signal,
    /// its code and the address it names. The handler there was before
    /// runs after it.
    pub fn note_crashes(path: &Path, heading: &str) {
        let Ok(path) = CString::new(path.as_os_str().as_bytes()) else {
            return;
        };
        // SAFETY: all-zero is a valid `sigaction`, and each is only read
        // into.
        let mut previous: [libc::sigaction; SIGNALS.len()] = unsafe { std::mem::zeroed() };
        for ((signal, _), previous) in SIGNALS.iter().zip(&mut previous) {
            // SAFETY: a null new action only reads the current one.
            unsafe { libc::sigaction(*signal, std::ptr::null(), previous) };
        }
        let note = Note {
            path,
            heading: heading.as_bytes().to_vec(),
            previous,
        };
        if NOTE.set(note).is_err() {
            return;
        }
        // SAFETY: all-zero is a valid `sigaction`; the handler lives as long
        // as the process, and it runs on the stack Rust gives each thread it
        // starts for signals, so a stack overflow can still be noted.
        unsafe {
            let mut action: libc::sigaction = std::mem::zeroed();
            action.sa_sigaction = handler as *const () as usize;
            action.sa_flags = libc::SA_SIGINFO | libc::SA_ONSTACK;
            libc::sigemptyset(&mut action.sa_mask);
            for (signal, _) in SIGNALS {
                libc::sigaction(signal, &action, std::ptr::null_mut());
            }
        }
    }

    extern "C" fn handler(signal: c_int, info: *mut libc::siginfo_t, context: *mut c_void) {
        let Some(note) = NOTE.get() else {
            return;
        };
        let Some(index) = SIGNALS.iter().position(|(each, _)| *each == signal) else {
            return;
        };
        if !WRITTEN.swap(true, Ordering::SeqCst) {
            // SAFETY: the kernel passes a valid `siginfo_t` to an
            // `SA_SIGINFO` handler.
            let (code, address) = unsafe { ((*info).si_code, (*info).si_addr() as usize) };
            let mut text = Text::new();
            text.push_str("\r\nSignal ");
            text.push_decimal(signal as u64);
            text.push_str(" (");
            text.push_str(SIGNALS[index].1);
            text.push_str("), code ");
            text.push_decimal(code as u64);
            text.push_str(", address 0x");
            text.push_hex(address as u64, 16);
            text.push_str("\r\n");
            write(note, &text);
        }
        // SAFETY: the previous action is put back so that it takes this
        // signal from now on, and called as it was set to be.
        unsafe { forward(&note.previous[index], signal, info, context) };
    }

    fn write(note: &Note, text: &Text) {
        // SAFETY: the path is NUL-terminated and each buffer's length is
        // given; open, write and close are safe in a signal handler.
        unsafe {
            let file = libc::open(
                note.path.as_ptr(),
                libc::O_WRONLY | libc::O_CREAT | libc::O_TRUNC | libc::O_CLOEXEC,
                0o644 as libc::c_uint,
            );
            if file >= 0 {
                for part in [note.heading.as_slice(), text.as_slice()] {
                    libc::write(file, part.as_ptr().cast(), part.len());
                }
                libc::close(file);
            }
        }
    }

    /// # Safety
    /// Called from the handler of `signal` with what it was given.
    unsafe fn forward(
        previous: &libc::sigaction,
        signal: c_int,
        info: *mut libc::siginfo_t,
        context: *mut c_void,
    ) {
        // SAFETY: as the caller says; the previous handler, if any, takes
        // the arguments its flags say.
        unsafe {
            libc::sigaction(signal, previous, std::ptr::null_mut());
            match previous.sa_sigaction {
                // The signal is blocked until the handler returns, and then
                // ends the process.
                libc::SIG_DFL => {
                    libc::raise(signal);
                }
                libc::SIG_IGN => {}
                with_info if previous.sa_flags & libc::SA_SIGINFO != 0 => {
                    let handler: extern "C" fn(c_int, *mut libc::siginfo_t, *mut c_void) =
                        std::mem::transmute(with_info);
                    handler(signal, info, context);
                }
                plain => {
                    let handler: extern "C" fn(c_int) = std::mem::transmute(plain);
                    handler(signal);
                }
            }
        }
    }

    /// Text on the stack.
    struct Text {
        bytes: [u8; 256],
        length: usize,
    }

    impl Text {
        fn new() -> Self {
            Self {
                bytes: [0; 256],
                length: 0,
            }
        }

        fn push(&mut self, byte: u8) {
            if self.length < self.bytes.len() {
                self.bytes[self.length] = byte;
                self.length += 1;
            }
        }

        fn push_str(&mut self, text: &str) {
            text.bytes().for_each(|byte| self.push(byte));
        }

        fn push_hex(&mut self, value: u64, digits: u32) {
            for shift in (0..digits).rev() {
                self.push(b"0123456789ABCDEF"[(value >> (shift * 4) & 0xF) as usize]);
            }
        }

        fn push_decimal(&mut self, value: u64) {
            let mut digits = [0u8; 20];
            let mut count = 0;
            let mut rest = value;
            loop {
                digits[count] = b'0' + (rest % 10) as u8;
                count += 1;
                rest /= 10;
                if rest == 0 {
                    break;
                }
            }
            digits[..count]
                .iter()
                .rev()
                .for_each(|&digit| self.push(digit));
        }

        fn as_slice(&self) -> &[u8] {
            &self.bytes[..self.length]
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn numbers_are_written_as_they_read() {
            let mut text = Text::new();
            text.push_decimal(0);
            text.push_str(" ");
            text.push_decimal(18_446_744_073_709_551_615);
            text.push_str(" 0x");
            text.push_hex(0xBEEF, 8);
            assert_eq!(text.as_slice(), b"0 18446744073709551615 0x0000BEEF");
        }

        /// Run as a child: notes crashes into the file named by the
        /// variable and writes through a null pointer.
        #[test]
        fn child_that_crashes() {
            let Some(path) = std::env::var_os("UGU_CRASH_CHILD") else {
                return;
            };
            note_crashes(Path::new(&path), "Ugurugu test\r\n");
            // SAFETY: not safe on purpose; this child exists to crash.
            unsafe { std::ptr::null_mut::<u8>().write_volatile(1) };
        }

        /// The bad access first reaches Rust's own handler, which tells it
        /// from a stack overflow, and then ends the child as it would have.
        #[test]
        fn a_crash_leaves_a_note_and_still_ends_the_process() {
            use std::os::unix::process::ExitStatusExt;

            let folder =
                std::env::temp_dir().join(format!("ugu-platform-crash-{}", std::process::id()));
            std::fs::create_dir_all(&folder).unwrap();
            let path = folder.join("crash.txt");
            let status = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "unix::crash::tests::child_that_crashes",
                    "--test-threads=1",
                ])
                .env("UGU_CRASH_CHILD", &path)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .unwrap();
            let text = std::fs::read_to_string(&path);
            let _ = std::fs::remove_dir_all(&folder);
            assert!(
                matches!(status.signal(), Some(libc::SIGSEGV | libc::SIGBUS)),
                "{status:?}"
            );
            let text = text.unwrap();
            assert!(text.starts_with("Ugurugu test\r\n\r\nSignal "), "{text}");
            assert!(text.contains(", address 0x0000000000000000"), "{text}");
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

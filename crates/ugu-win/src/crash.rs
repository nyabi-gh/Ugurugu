// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! A note of a crash that Rust cannot catch, such as an access violation in
//! a driver, written by the unhandled exception filter. The filter runs in a
//! broken process, so it allocates nothing: the file name and the heading
//! are made beforehand, and the note is UTF-16 so that module paths need no
//! conversion.

use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use std::sync::OnceLock;

use windows_sys::Win32::Foundation::{CloseHandle, GENERIC_WRITE, HMODULE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::Storage::FileSystem::{
    CREATE_ALWAYS, CreateFileW, FILE_ATTRIBUTE_NORMAL, WriteFile,
};
use windows_sys::Win32::System::Diagnostics::Debug::{
    EXCEPTION_POINTERS, SetUnhandledExceptionFilter,
};
use windows_sys::Win32::System::LibraryLoader::{
    GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
    GetModuleFileNameW, GetModuleHandleExW,
};

struct Note {
    /// Null-terminated.
    path: Vec<u16>,
    /// Starts with the byte order mark.
    heading: Vec<u16>,
}

static NOTE: OnceLock<Note> = OnceLock::new();

/// From now on, a crash no handler takes writes `path` (UTF-16) with
/// `heading` and then the exception code, the address and the module it is
/// in. Windows Error Reporting still runs after it.
pub fn note_crashes(path: &Path, heading: &str) {
    let note = Note {
        path: path.as_os_str().encode_wide().chain([0]).collect(),
        heading: [0xFEFF].into_iter().chain(heading.encode_utf16()).collect(),
    };
    if NOTE.set(note).is_ok() {
        // SAFETY: the filter is a function that lives as long as the
        // process.
        unsafe { SetUnhandledExceptionFilter(Some(filter)) };
    }
}

unsafe extern "system" fn filter(pointers: *const EXCEPTION_POINTERS) -> i32 {
    const CONTINUE_SEARCH: i32 = 0;
    let Some(note) = NOTE.get() else {
        return CONTINUE_SEARCH;
    };
    // SAFETY: Windows passes valid exception pointers to the filter.
    let (code, address) = unsafe {
        let record = &*(*pointers).ExceptionRecord;
        (
            record.ExceptionCode as u32,
            record.ExceptionAddress as usize,
        )
    };
    let mut text = Text::new();
    text.push_str("\r\nException 0x");
    text.push_hex(u64::from(code), 8);
    text.push_str(" at 0x");
    text.push_hex(address as u64, 16);
    let mut module: HMODULE = std::ptr::null_mut();
    // SAFETY: the address is only looked up, the handle is not counted, and
    // the name buffer's length is given.
    unsafe {
        let flags =
            GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT;
        if GetModuleHandleExW(flags, address as *const u16, &mut module) != 0 {
            let mut name = [0u16; 260];
            let length = GetModuleFileNameW(module, name.as_mut_ptr(), name.len() as u32) as usize;
            text.push_str("\r\nIn ");
            text.push_wide(&name[..length.min(name.len())]);
            text.push_str(" +0x");
            text.push_hex((address - module as usize) as u64, 8);
        }
    }
    text.push_str("\r\n");
    // SAFETY: the path is null-terminated and every buffer's length is given.
    unsafe {
        let file = CreateFileW(
            note.path.as_ptr(),
            GENERIC_WRITE,
            0,
            std::ptr::null(),
            CREATE_ALWAYS,
            FILE_ATTRIBUTE_NORMAL,
            std::ptr::null_mut(),
        );
        if file != INVALID_HANDLE_VALUE {
            let mut written = 0;
            for part in [note.heading.as_slice(), text.as_slice()] {
                WriteFile(
                    file,
                    part.as_ptr().cast(),
                    (part.len() * 2) as u32,
                    &mut written,
                    std::ptr::null_mut(),
                );
            }
            CloseHandle(file);
        }
    }
    CONTINUE_SEARCH
}

/// UTF-16 text on the stack.
struct Text {
    units: [u16; 512],
    length: usize,
}

impl Text {
    fn new() -> Self {
        Self {
            units: [0; 512],
            length: 0,
        }
    }

    fn push(&mut self, unit: u16) {
        if self.length < self.units.len() {
            self.units[self.length] = unit;
            self.length += 1;
        }
    }

    fn push_str(&mut self, text: &str) {
        for byte in text.bytes() {
            self.push(u16::from(byte));
        }
    }

    fn push_wide(&mut self, text: &[u16]) {
        for &unit in text {
            self.push(unit);
        }
    }

    fn push_hex(&mut self, value: u64, digits: u32) {
        for index in (0..digits).rev() {
            let digit = ((value >> (index * 4)) & 0xF) as u8;
            self.push(u16::from(if digit < 10 {
                b'0' + digit
            } else {
                b'A' + digit - 10
            }));
        }
    }

    fn as_slice(&self) -> &[u16] {
        &self.units[..self.length]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_has_the_digits_asked_for() {
        let mut text = Text::new();
        text.push_hex(0xC000_0005, 8);
        text.push_str(" ");
        text.push_hex(0x1F, 4);
        assert_eq!(
            String::from_utf16(text.as_slice()).unwrap(),
            "C0000005 001F"
        );
    }

    /// Run as a child: notes crashes into the file named by the variable and
    /// writes through a null pointer.
    #[test]
    fn child_that_crashes() {
        let Some(path) = std::env::var_os("UGU_CRASH_CHILD") else {
            return;
        };
        note_crashes(Path::new(&path), "Ugurugu test\r\n");
        // SAFETY: not safe on purpose; this child exists to crash.
        unsafe { std::ptr::null_mut::<u8>().write_volatile(1) };
    }

    #[test]
    fn a_crash_leaves_a_note() {
        let folder = std::env::temp_dir().join(format!("ugu-crash-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let path = folder.join("crash.txt");
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "crash::tests::child_that_crashes",
                "--test-threads=1",
            ])
            .env("UGU_CRASH_CHILD", &path)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .unwrap();
        assert!(!status.success());
        let bytes = std::fs::read(&path).unwrap();
        let units: Vec<u16> = bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u16::from_le_bytes(*pair))
            .collect();
        let text = String::from_utf16(&units[1..]).unwrap();
        assert!(text.starts_with("Ugurugu test\r\n"), "{text}");
        assert!(text.contains("Exception 0xC0000005"), "{text}");
        assert!(text.contains("\r\nIn "), "{text}");
        std::fs::remove_dir_all(&folder).unwrap();
    }
}

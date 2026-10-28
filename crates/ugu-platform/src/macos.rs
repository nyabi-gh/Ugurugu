// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! The macOS services. Those not written for macOS yet are the placeholders
//! of [`crate::unsupported`].

use std::ffi::{CStr, c_char, c_int, c_void};

pub mod clipboard;
pub mod dialog;
pub mod folder;
pub mod locale;
pub mod memory;

pub use crate::unsupported::{
    association, clock, crash, file, instance, placement, recent, session,
};

unsafe extern "C" {
    fn sysctlbyname(
        name: *const c_char,
        old: *mut c_void,
        old_length: *mut usize,
        new: *mut c_void,
        new_length: usize,
    ) -> c_int;
}

/// The bytes of sysctl `name`, `None` when the kernel has no such value.
fn sysctl(name: &CStr) -> Option<Vec<u8>> {
    let mut length = 0;
    // SAFETY: a null buffer asks for the length only.
    if unsafe {
        sysctlbyname(
            name.as_ptr(),
            std::ptr::null_mut(),
            &mut length,
            std::ptr::null_mut(),
            0,
        )
    } != 0
    {
        return None;
    }
    let mut value = vec![0u8; length];
    // SAFETY: the buffer holds the `length` bytes given.
    if unsafe {
        sysctlbyname(
            name.as_ptr(),
            value.as_mut_ptr().cast(),
            &mut length,
            std::ptr::null_mut(),
            0,
        )
    } != 0
    {
        return None;
    }
    value.truncate(length);
    Some(value)
}

/// A string value, without its terminating NUL.
fn sysctl_text(name: &CStr) -> Option<String> {
    let value = sysctl(name)?;
    let text = CStr::from_bytes_until_nul(&value).ok()?;
    Some(text.to_string_lossy().into_owned())
}

fn sysctl_u64(name: &CStr) -> Option<u64> {
    Some(u64::from_ne_bytes(sysctl(name)?.try_into().ok()?))
}

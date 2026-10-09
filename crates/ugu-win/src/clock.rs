// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

use std::sync::OnceLock;

use windows_sys::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};

/// A `QueryPerformanceCounter` reading, the clock pointer input is stamped with.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Ticks(pub u64);

impl Ticks {
    pub fn now() -> Self {
        let mut value = 0i64;
        // SAFETY: the pointer is valid; the call cannot fail on Windows XP or later.
        unsafe { QueryPerformanceCounter(&mut value) };
        Self(value as u64)
    }

    pub fn minus_millis(self, millis: u32) -> Self {
        Self(
            self.0
                .saturating_sub(u64::from(millis) * frequency() / 1000),
        )
    }

    pub fn seconds_since(self, earlier: Ticks) -> f64 {
        self.0.saturating_sub(earlier.0) as f64 / frequency() as f64
    }
}

fn frequency() -> u64 {
    static FREQUENCY: OnceLock<u64> = OnceLock::new();
    *FREQUENCY.get_or_init(|| {
        let mut value = 0i64;
        // SAFETY: the pointer is valid; the call cannot fail on Windows XP or later.
        unsafe { QueryPerformanceFrequency(&mut value) };
        value.max(1) as u64
    })
}

/// `since_unix_epoch` as the user's locale writes a short date and a time
/// without seconds, in local time.
pub fn local_text(since_unix_epoch: std::time::Duration) -> Option<String> {
    use windows_sys::Win32::Foundation::{FILETIME, SYSTEMTIME};
    use windows_sys::Win32::Globalization::{
        DATE_SHORTDATE, GetDateFormatEx, GetTimeFormatEx, TIME_NOSECONDS,
    };
    use windows_sys::Win32::System::Time::{FileTimeToSystemTime, SystemTimeToTzSpecificLocalTime};
    // FILETIME counts 100 ns from 1601.
    const FROM_1601: u128 = 116_444_736_000_000_000;
    let ticks = u64::try_from(since_unix_epoch.as_nanos() / 100 + FROM_1601).ok()?;
    let file = FILETIME {
        dwLowDateTime: ticks as u32,
        dwHighDateTime: (ticks >> 32) as u32,
    };
    let mut utc = SYSTEMTIME::default();
    let mut local = SYSTEMTIME::default();
    let mut date = [0u16; 80];
    let mut time = [0u16; 80];
    // SAFETY: every pointer is to a live value of the right type, the
    // buffers' lengths are given, and a null locale is the user's.
    let (date_length, time_length) = unsafe {
        if FileTimeToSystemTime(&file, &mut utc) == 0
            || SystemTimeToTzSpecificLocalTime(std::ptr::null(), &utc, &mut local) == 0
        {
            return None;
        }
        (
            GetDateFormatEx(
                std::ptr::null(),
                DATE_SHORTDATE,
                &local,
                std::ptr::null(),
                date.as_mut_ptr(),
                date.len() as i32,
                std::ptr::null(),
            ),
            GetTimeFormatEx(
                std::ptr::null(),
                TIME_NOSECONDS,
                &local,
                std::ptr::null(),
                time.as_mut_ptr(),
                time.len() as i32,
            ),
        )
    };
    // The lengths count the closing null.
    let text = |buffer: &[u16], length: i32| {
        let length = usize::try_from(length).ok().filter(|&length| length > 0)?;
        Some(String::from_utf16_lossy(&buffer[..length - 1]))
    };
    Some(format!(
        "{} {}",
        text(&date, date_length)?,
        text(&time, time_length)?
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_time_is_written_with_its_year() {
        // 2026-07-01 12:00 UTC: the same date in every time zone.
        let text = local_text(std::time::Duration::from_secs(1_782_907_200)).unwrap();
        assert!(text.contains("26"), "{text}");
        assert!(local_text(std::time::Duration::from_secs(u64::MAX)).is_none());
    }
}

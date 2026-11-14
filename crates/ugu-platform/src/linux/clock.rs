// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 NewfolderGames

//! Monotonic and local wall-clock times on Linux.
//!
//! Input protocols on Linux (`zwp_input_timestamps_v1`, `zwp_tablet_v2`, and
//! XInput2) stamp events on `CLOCK_MONOTONIC`, so [`now`] reads the raw kernel
//! `CLOCK_MONOTONIC` counter directly rather than an offset from process start.

use std::time::Duration;

use ugu_platform_types::clock::Ticks;

pub use crate::unix::clock::local_stamp;

/// Current kernel `CLOCK_MONOTONIC` time in nanoseconds.
pub fn now() -> Ticks {
    let mut ts = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: `ts` points to a live `timespec` on the stack, and
    // `CLOCK_MONOTONIC` is always valid on Linux.
    unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts) };
    from_timespec(ts.tv_sec, ts.tv_nsec)
}

/// Converts a `CLOCK_MONOTONIC` `(seconds, nanoseconds)` pair into [`Ticks`].
pub fn from_timespec(seconds: i64, nanoseconds: i64) -> Ticks {
    let secs = u64::try_from(seconds).unwrap_or(0);
    let nanos = u64::try_from(nanoseconds).unwrap_or(0);
    Ticks(secs.saturating_mul(1_000_000_000).saturating_add(nanos))
}

/// Converts a millisecond `CLOCK_MONOTONIC` timestamp (whose upper bits have
/// wrapped at 32 bits, as in Wayland protocol events without
/// `zwp_input_timestamps_v1` or XInput2 server timestamps) into [`Ticks`] near
/// `reference`.
pub fn from_wrapped_millis(millis: u32, reference: Ticks) -> Ticks {
    let ref_ms = reference.0 / 1_000_000;
    let low_ref = ref_ms as u32;
    let delta = millis.wrapping_sub(low_ref) as i32;
    let unwrapped_ms = if delta >= 0 {
        ref_ms.saturating_add(delta as u64)
    } else {
        ref_ms.saturating_sub(delta.unsigned_abs() as u64)
    };
    Ticks(unwrapped_ms.saturating_mul(1_000_000))
}

pub fn minus_millis(ticks: Ticks, millis: u32) -> Ticks {
    Ticks(ticks.0.saturating_sub(u64::from(millis) * 1_000_000))
}

pub fn seconds_since(ticks: Ticks, earlier: Ticks) -> f64 {
    Duration::from_nanos(ticks.0.saturating_sub(earlier.0)).as_secs_f64()
}

/// Formats `since_unix_epoch` as `YYYY-MM-DD HH:MM` in local time without
/// touching process-global `LC_TIME` state.
pub fn local_text(since_unix_epoch: Duration) -> Option<String> {
    let secs = libc::time_t::try_from(since_unix_epoch.as_secs()).ok()?;
    // SAFETY: all-zero is a valid `libc::tm`, and both pointers outlive the
    // thread-safe `localtime_r` call.
    let mut local: libc::tm = unsafe { std::mem::zeroed() };
    let converted = unsafe { libc::localtime_r(&secs, &mut local) };
    if converted.is_null() {
        return None;
    }
    let year = i64::from(local.tm_year) + 1900;
    Some(format!(
        "{year:04}-{:02}-{:02} {:02}:{:02}",
        local.tm_mon + 1,
        local.tm_mday,
        local.tm_hour,
        local.tm_min,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn monotonic_clock_advances_in_nanoseconds() {
        let first = now();
        std::thread::sleep(Duration::from_millis(2));
        let second = now();
        assert!(second.0 > first.0);
        let elapsed = seconds_since(second, first);
        assert!((0.001..5.0).contains(&elapsed), "{elapsed}");
        assert_eq!(minus_millis(Ticks(10_000_000), 3), Ticks(7_000_000));
    }

    #[test]
    fn wrapped_millisecond_stamps_follow_the_reference_epoch() {
        // Reference past one 32-bit millisecond wrap (~49.7 days).
        let wrap_ns = (1u64 << 32) * 1_000_000;
        let reference = Ticks(wrap_ns + 500 * 1_000_000);
        assert_eq!(
            from_wrapped_millis(490, reference),
            Ticks(wrap_ns + 490 * 1_000_000)
        );
        assert_eq!(
            from_wrapped_millis(u32::MAX - 9, Ticks(wrap_ns + 5 * 1_000_000)),
            Ticks(wrap_ns - 10 * 1_000_000)
        );
    }

    #[test]
    fn a_time_is_written_with_its_year_and_no_seconds() {
        // 2026-07-01 12:00 UTC: the same year in every time zone.
        let text = local_text(Duration::from_secs(1_782_907_200)).unwrap();
        assert!(text.contains("2026"), "{text}");
        assert_eq!(text.matches(':').count(), 1, "{text}");
        assert!(local_text(Duration::from_secs(u64::MAX)).is_none());
    }
}

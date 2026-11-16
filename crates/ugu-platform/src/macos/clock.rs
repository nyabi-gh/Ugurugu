// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

use std::time::Duration;

use objc2_foundation::{NSDate, NSDateFormatter, NSDateFormatterStyle};
use ugu_platform_types::clock::Ticks;

pub use crate::unsupported::clock::{local_stamp, minus_millis, now, seconds_since};

unsafe extern "C" {
    fn clock_gettime_nsec_np(clock: u32) -> u64;
}

/// `<time.h>`'s; the clock `mach_absolute_time` and AppKit's event times count.
const CLOCK_UPTIME_RAW: u32 = 8;

/// The ticks of an AppKit event's `timestamp`, which counts seconds of
/// uptime: as long before [`now`] as it is before this moment.
pub(crate) fn from_uptime(seconds: f64) -> Ticks {
    // SAFETY: no pointers; an unknown clock would return 0.
    let uptime = unsafe { clock_gettime_nsec_np(CLOCK_UPTIME_RAW) };
    let age = uptime.saturating_sub((seconds * 1e9) as u64);
    Ticks(now().0.saturating_sub(age))
}

/// `since_unix_epoch` as the user's locale writes a short date and a time
/// without seconds, in local time.
pub fn local_text(since_unix_epoch: Duration) -> Option<String> {
    let formatter = NSDateFormatter::new();
    formatter.setDateStyle(NSDateFormatterStyle::ShortStyle);
    formatter.setTimeStyle(NSDateFormatterStyle::ShortStyle);
    let date = NSDate::dateWithTimeIntervalSince1970(since_unix_epoch.as_secs_f64());
    Some(formatter.stringFromDate(&date).to_string())
}

#[cfg(test)]
mod tests {
    #[test]
    fn an_event_time_is_as_old_as_its_uptime_says() {
        // `now` counts from its first call and cannot go before it.
        super::now();
        std::thread::sleep(std::time::Duration::from_millis(100));
        let uptime = std::time::Duration::from_nanos(
            // SAFETY: as in `from_uptime`.
            unsafe { super::clock_gettime_nsec_np(super::CLOCK_UPTIME_RAW) },
        );
        let before = super::now();
        let event = super::from_uptime(uptime.as_secs_f64() - 0.05);
        // At least as old; older by however long this thread was held up.
        let age = super::seconds_since(before, event);
        assert!((0.049..0.5).contains(&age), "{age}");
    }

    #[test]
    fn a_time_is_written_with_its_year() {
        // 2026-10-28 12:00 UTC: the same year in every time zone.
        let text = super::local_text(std::time::Duration::from_secs(1_793_188_800)).unwrap();
        assert!(text.contains("26"), "{text}");
        assert!(!text.contains(":00:"), "no seconds: {text}");
    }
}

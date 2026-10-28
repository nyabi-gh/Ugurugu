// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

use std::time::Duration;

use objc2_foundation::{NSDate, NSDateFormatter, NSDateFormatterStyle};

pub use crate::unsupported::clock::{local_stamp, minus_millis, now, seconds_since};

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
    fn a_time_is_written_with_its_year() {
        // 2026-10-28 12:00 UTC: the same year in every time zone.
        let text = super::local_text(std::time::Duration::from_secs(1_793_188_800)).unwrap();
        assert!(text.contains("26"), "{text}");
        assert!(!text.contains(":00:"), "no seconds: {text}");
    }
}

// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! The services on an OS that has no implementation yet, so that the app
//! builds there. Each reports that it is not supported, or that there is
//! nothing; values that need a window cannot be made at all. Files are
//! replaced as on every Unix.

use std::fmt;

/// What a service without an implementation returns.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Unsupported;

impl fmt::Display for Unsupported {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "not supported on {}", std::env::consts::OS)
    }
}

impl std::error::Error for Unsupported {}

pub mod association {
    use std::path::Path;

    pub const CLASSES: &str = "";

    pub fn register(_app: &Path, _name: &str, _classes: &str) -> Result<(), String> {
        Err(super::Unsupported.to_string())
    }

    pub fn unregister(_classes: &str) {}
}

pub mod clipboard {
    use ugu_platform_types::clipboard::{Encoded, Image, ReadError};

    /// Never changes: there is no change count to read.
    pub fn sequence() -> u32 {
        0
    }

    pub fn write_image(_image: Image) -> Result<u32, String> {
        Err(super::Unsupported.to_string())
    }

    pub fn read_encoded(_limit: usize) -> Result<Option<Encoded>, ReadError> {
        Err(ReadError::System(super::Unsupported.to_string()))
    }
}

/// Nanoseconds of the standard library's monotonic clock since its first
/// reading here. Which clock each OS stamps its input with is chosen with
/// its pointer input.
pub mod clock {
    use std::sync::OnceLock;
    use std::time::{Duration, Instant};

    use ugu_platform_types::clock::Ticks;

    pub fn now() -> Ticks {
        static START: OnceLock<Instant> = OnceLock::new();
        let since = START.get_or_init(Instant::now).elapsed();
        Ticks(u64::try_from(since.as_nanos()).unwrap_or(u64::MAX))
    }

    pub fn minus_millis(ticks: Ticks, millis: u32) -> Ticks {
        Ticks(ticks.0.saturating_sub(u64::from(millis) * 1_000_000))
    }

    pub fn seconds_since(ticks: Ticks, earlier: Ticks) -> f64 {
        Duration::from_nanos(ticks.0.saturating_sub(earlier.0)).as_secs_f64()
    }

    pub fn local_stamp() -> Option<String> {
        None
    }

    pub fn local_text(_since_unix_epoch: Duration) -> Option<String> {
        None
    }
}

pub mod crash {
    use std::path::Path;

    pub fn note_crashes(_path: &Path, _heading: &str) {}
}

#[cfg(not(target_os = "macos"))]
pub mod dialog {
    use std::path::PathBuf;

    use ugu_platform_types::dialog::Dialog;

    pub fn pick(_owner: isize, _dialog: &Dialog) -> Result<Option<PathBuf>, super::Unsupported> {
        Err(super::Unsupported)
    }
}

pub mod file {
    pub use crate::unix::file::{remove_stale_temporaries, replace_file};
}

#[cfg(not(target_os = "macos"))]
pub mod folder {
    use std::path::{Path, PathBuf};

    pub fn roaming_app_data() -> Option<PathBuf> {
        None
    }

    pub fn local_app_data() -> Option<PathBuf> {
        None
    }

    pub fn documents() -> Option<PathBuf> {
        None
    }

    pub fn show(_folder: &Path) -> Result<(), String> {
        Err(super::Unsupported.to_string())
    }
}

pub mod instance {
    use std::path::{Path, PathBuf};

    pub enum StartLock {}

    impl StartLock {
        pub fn take(_name: &str) -> Option<Self> {
            None
        }
    }

    pub fn hand_over(_class: &str, _key: &str, _document: Option<&Path>) -> bool {
        false
    }

    /// # Safety
    /// None; unsafe to match the OSes that have it.
    pub unsafe fn bring_to_front(_hwnd: isize) {}

    /// Receives nothing: no start hands a document over here.
    pub struct DocumentReceiver;

    impl DocumentReceiver {
        /// # Safety
        /// None; unsafe to match the OSes that have it.
        pub unsafe fn install(
            _hwnd: isize,
            _key: &str,
            _receive: impl FnMut(Option<PathBuf>) -> bool + 'static,
        ) -> Result<Self, &'static str> {
            Ok(Self)
        }
    }
}

pub mod locale {
    pub fn preferred_ui_languages() -> Vec<String> {
        Vec::new()
    }

    /// The OS's name: there is no version to read yet.
    pub fn windows_version() -> String {
        std::env::consts::OS.to_owned()
    }
}

pub mod memory {
    use ugu_platform_types::memory::Memory;

    pub fn status() -> Option<Memory> {
        None
    }
}

pub mod placement {
    use ugu_platform_types::placement::Placement;

    pub fn get(_hwnd: isize) -> Option<Placement> {
        None
    }

    /// # Safety
    /// None; unsafe to match the OSes that have it.
    pub unsafe fn restore_to(_hwnd: isize, _placement: &Placement) -> bool {
        false
    }
}

pub mod recent {
    use std::path::Path;

    pub fn add(_path: &Path) {}
}

pub mod session {
    pub enum SessionEnd {}

    impl SessionEnd {
        /// # Safety
        /// None; unsafe to match the OSes that have it.
        pub unsafe fn install(
            _hwnd: isize,
            _ending: impl FnMut() + 'static,
        ) -> Result<Self, &'static str> {
            Err("not supported on this OS")
        }
    }
}

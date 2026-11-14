// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! Input and OS services for the app and the tools: the shared types and the
//! running OS's implementation of each service, so that nothing above this
//! crate names an OS crate.

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(unix)]
mod unix;
#[cfg(not(windows))]
mod unsupported;

#[cfg(target_os = "linux")]
use linux as os;
#[cfg(target_os = "macos")]
use macos as os;
#[cfg(windows)]
use ugu_win as os;
#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
use unsupported as os;

pub mod association {
    pub use crate::os::association::{CLASSES, register, unregister};
}

pub mod clipboard {
    #[cfg(target_os = "linux")]
    pub use crate::os::clipboard::{pixel_sha256, png_pixel_sha256};
    pub use crate::os::clipboard::{read_encoded, sequence, write_image};
    pub use ugu_platform_types::clipboard::{Encoded, Image, ReadError};
}

pub mod clock {
    #[cfg(target_os = "linux")]
    pub use crate::os::clock::{from_timespec, from_wrapped_millis};
    pub use crate::os::clock::{local_stamp, local_text, minus_millis, now, seconds_since};
    pub use ugu_platform_types::clock::Ticks;
}

pub mod crash {
    pub use crate::os::crash::note_crashes;
}

pub mod dialog {
    pub use crate::os::dialog::pick;
    pub use ugu_platform_types::dialog::{Dialog, FileType, with_extension_of};
}

pub mod file {
    pub use crate::os::file::{remove_stale_temporaries, replace_file};
}

pub mod folder {
    pub use crate::os::folder::{documents, local_app_data, roaming_app_data, show};
}

pub mod instance {
    #[cfg(target_os = "linux")]
    pub use crate::os::instance::poll_current_thread_receivers;
    pub use crate::os::instance::{DocumentReceiver, StartLock, bring_to_front, hand_over};
}

pub mod locale {
    pub use crate::os::locale::{os_version, preferred_ui_languages};
}

pub mod memory {
    pub use crate::os::memory::status;
    pub use ugu_platform_types::memory::Memory;
}

pub mod placement {
    #[cfg(windows)]
    pub use crate::os::placement::{get, restore_to};
    pub use ugu_platform_types::placement::Placement;
}

pub mod pointer {
    #[cfg(windows)]
    pub use crate::os::pointer::{PointerInput, enable_mouse_in_pointer};
    pub use ugu_platform_types::pointer::{
        Buttons, PointerError, PointerEvent, PointerId, PointerKind, PointerSample,
    };
}

pub mod recent {
    pub use crate::os::recent::add;
}

#[cfg(target_os = "macos")]
pub mod menu {
    pub use crate::os::menu::Menus;
    pub use ugu_platform_types::menu::{
        Action, Bar, EditRole, Entry, Key, Menu, MenuRole, SystemItem,
    };
}

/// The requests macOS makes of the app as a whole.
#[cfg(target_os = "macos")]
pub mod requests {
    pub use crate::os::requests::listen;
}

pub mod session {
    #[cfg(target_os = "macos")]
    pub use crate::os::session::QuitRequests;
    pub use crate::os::session::SessionEnd;
}

#[cfg(target_os = "linux")]
pub mod window {
    pub use crate::os::window::{
        LinuxWindow, drain_pen_events, pen_in_proximity, set_scale_factor, set_tablet_cursor,
        shrink_square_rgba,
    };
}

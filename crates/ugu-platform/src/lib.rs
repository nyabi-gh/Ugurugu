// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! Input and OS services for the app and the tools: the shared types and the
//! running OS's implementation of each service, so that nothing above this
//! crate names an OS crate.

#[cfg(unix)]
mod unix;
#[cfg(not(windows))]
mod unsupported;

#[cfg(windows)]
use ugu_win as os;
#[cfg(not(windows))]
use unsupported as os;

pub mod association {
    pub use crate::os::association::{CLASSES, register, unregister};
}

pub mod clipboard {
    pub use crate::os::clipboard::{read_encoded, sequence, write_image};
    pub use ugu_platform_types::clipboard::{Encoded, Image, ReadError};
}

pub mod clock {
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
    pub use crate::os::instance::{DocumentReceiver, StartLock, bring_to_front, hand_over};
}

pub mod locale {
    pub use crate::os::locale::{preferred_ui_languages, windows_version};
}

pub mod memory {
    pub use crate::os::memory::status;
    pub use ugu_platform_types::memory::Memory;
}

pub mod placement {
    pub use crate::os::placement::{get, restore_to};
    pub use ugu_platform_types::placement::Placement;
}

pub mod pointer {
    pub use crate::os::pointer::{PointerInput, enable_mouse_in_pointer};
    pub use ugu_platform_types::pointer::{
        Buttons, PointerError, PointerEvent, PointerId, PointerKind, PointerSample,
    };
}

pub mod recent {
    pub use crate::os::recent::add;
}

pub mod session {
    pub use crate::os::session::SessionEnd;
}

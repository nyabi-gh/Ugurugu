// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 NewfolderGames

//! Native Linux OS services (`xdg-desktop-portal`, Wayland/X11 clipboard and
//! window handles, `/proc/meminfo`, `CLOCK_MONOTONIC`, and XDG folders).

pub mod clipboard;
pub mod clock;
pub mod dialog;
pub mod folder;
pub mod instance;
pub mod locale;
pub mod memory;
pub mod session;
pub(crate) mod wayland_tablet;
pub mod window;
pub(crate) mod x11_tablet;

pub use crate::unsupported::{association, crash, file, recent};

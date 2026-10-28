// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! Windows input and OS services. GPU presentation lives in `ugu-render`.

#![cfg(windows)]

pub mod association;
pub mod clipboard;
pub mod clock;
pub mod crash;
pub mod dialog;
pub mod file;
pub mod folder;
pub mod instance;
pub mod locale;
pub mod memory;
pub mod placement;
pub mod pointer;
pub mod recent;
pub mod session;

// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! The macOS services. Those not written for macOS yet are the placeholders
//! of [`crate::unsupported`].

pub mod clipboard;
pub mod dialog;
pub mod folder;

pub use crate::unsupported::{
    association, clock, crash, file, instance, locale, memory, placement, recent, session,
};

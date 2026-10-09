// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! The `.ugurugu` file format (docs/rust/adr-operations-and-format.md section 4)
//! and image export.

pub mod format;
pub mod gif;
pub mod image;
pub mod import;
pub mod read;
pub mod save;
pub mod write;

#[cfg(test)]
mod tests;

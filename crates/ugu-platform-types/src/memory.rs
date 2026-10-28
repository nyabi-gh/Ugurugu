// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

/// Memory as Windows reports it now, in bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Memory {
    /// Installed physical memory.
    pub total: u64,
    /// Physical memory that can be taken without paging anything out.
    pub available: u64,
    /// What can still be committed before allocations fail.
    pub commit_available: u64,
}

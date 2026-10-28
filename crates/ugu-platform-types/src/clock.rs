// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

/// A reading of the clock pointer input is stamped with, in that clock's
/// own units: `QueryPerformanceCounter` on Windows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Ticks(pub u64);

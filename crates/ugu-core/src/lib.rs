// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! Document model. M0 holds only the layer operations whose meaning
//! docs/rust/adr-operations-and-format.md fixes, and tests that pin that
//! meaning down with a reference evaluator.

pub mod ops;

#[cfg(test)]
mod semantics;

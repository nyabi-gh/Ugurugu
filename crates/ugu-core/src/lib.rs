// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! Document model: the layer tree and its validation, and the layer
//! operations whose meaning docs/rust/adr-operations-and-format.md fixes,
//! pinned down by tests with a reference evaluator.

pub mod document;
pub mod ops;
pub mod store;

#[cfg(test)]
mod semantics;

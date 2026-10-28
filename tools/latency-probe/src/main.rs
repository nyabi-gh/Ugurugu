// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! Input-to-display latency of a drawing app, measured from outside it with
//! `SendInput` and PresentMon, so on Windows only (see `probe`).

#[cfg(windows)]
mod probe;

#[cfg(windows)]
fn main() -> std::process::ExitCode {
    probe::main()
}

#[cfg(not(windows))]
fn main() -> std::process::ExitCode {
    eprintln!("latency-probe runs only on Windows");
    std::process::ExitCode::FAILURE
}

// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! Input-to-display latency of a drawing app, measured from outside it: with
//! `SendInput` and PresentMon on Windows (see `probe`), with `CGEventPost` and
//! ScreenCaptureKit on macOS (see `macos`).

#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod probe;
#[cfg(any(windows, target_os = "macos"))]
mod stats;

#[cfg(windows)]
fn main() -> std::process::ExitCode {
    probe::main()
}

#[cfg(target_os = "macos")]
fn main() -> std::process::ExitCode {
    macos::main()
}

#[cfg(not(any(windows, target_os = "macos")))]
fn main() -> std::process::ExitCode {
    eprintln!("latency-probe runs only on Windows and macOS");
    std::process::ExitCode::FAILURE
}

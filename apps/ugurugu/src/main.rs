// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod budget;
mod cache;
mod canvas;
mod clipboard;
mod export;
mod files;
mod i18n;
mod icons;
mod ime_probe;
mod input;
mod latency;
mod layout;
mod recovery;
mod render;
mod settings;
mod shortcuts;
mod theme;
mod ui;
mod widgets;

use std::io::IsTerminal;
use std::process::ExitCode;

use tracing_subscriber::EnvFilter;
use winit::event_loop::EventLoop;

fn main() -> ExitCode {
    // Logging must never block the render thread on a slow or full output.
    let (log_writer, _log_flush) = tracing_appender::non_blocking(std::io::stdout());
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_env("UGURUGU_LOG")
                .unwrap_or_else(|_| EnvFilter::new("info,wgpu_core=warn,wgpu_hal=warn")),
        )
        .with_ansi(std::io::stdout().is_terminal())
        .with_writer(log_writer)
        .init();
    tracing::debug!("starting");

    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            tracing::error!(error, "exiting after a fatal error");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let early_gpu = render::open_gpu_early();
    let settings = settings::Store::load();
    i18n::choose(settings.get().language);
    ugu_win::pointer::enable_mouse_in_pointer()
        .map_err(|error| format!("cannot route the mouse through pointer input: {error}"))?;
    let event_loop = EventLoop::<app::UiEvent>::with_user_event()
        .build()
        .map_err(|error| format!("cannot create the event loop: {error}"))?;
    let mut app = app::App::new(event_loop.create_proxy(), early_gpu, settings);
    event_loop
        .run_app(&mut app)
        .map_err(|error| format!("event loop failed: {error}"))?;
    match app.fatal_error() {
        Some(error) => Err(error.to_owned()),
        None => Ok(()),
    }
}

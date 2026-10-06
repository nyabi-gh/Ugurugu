// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;

use tracing_subscriber::EnvFilter;
use winit::event_loop::EventLoop;

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_env("UGURUGU_LOG")
                .unwrap_or_else(|_| EnvFilter::new("info,wgpu_core=warn,wgpu_hal=warn")),
        )
        .init();

    let event_loop = match EventLoop::new() {
        Ok(event_loop) => event_loop,
        Err(error) => {
            tracing::error!(%error, "cannot create the event loop");
            std::process::exit(1);
        }
    };
    let mut app = app::App::default();
    if let Err(error) = event_loop.run_app(&mut app) {
        tracing::error!(%error, "event loop failed");
        std::process::exit(1);
    }
    if let Some(error) = app.fatal_error() {
        tracing::error!(error, "exiting after a fatal error");
        std::process::exit(1);
    }
}

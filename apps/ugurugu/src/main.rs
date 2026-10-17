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
mod logs;
mod recovery;
mod render;
mod settings;
mod shortcuts;
mod theme;
mod ui;
mod update;
mod widgets;

use std::process::ExitCode;

use winit::event_loop::EventLoop;

fn main() -> ExitCode {
    // Velopack's install, update and uninstall hooks start this program with
    // arguments of their own and end it here; a normal start goes on.
    velopack::VelopackApp::build()
        .on_after_install_fast_callback(|_| associate())
        .on_after_update_fast_callback(|_| associate())
        .on_before_uninstall_fast_callback(|_| {
            ugu_win::association::unregister(ugu_win::association::CLASSES)
        })
        .run();
    let document = app::document_in(std::env::args_os().skip(1));
    // One window, like Clip Studio Paint: a later start hands its document
    // to the window open, or just brings it to the front. The lock is held
    // until this start's own window can take later ones.
    let start_lock = ugu_win::instance::StartLock::take(app::START_LOCK);
    if ugu_win::instance::hand_over(app::WINDOW_CLASS, &app::instance_key(), document.as_deref()) {
        return ExitCode::SUCCESS;
    }
    let _logging = logs::start();
    tracing::debug!("starting");

    match run(document, start_lock) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            tracing::error!(error, "exiting after a fatal error");
            ExitCode::FAILURE
        }
    }
}

/// Opens `.ugurugu` files with this executable.
fn associate() {
    if let Ok(app) = std::env::current_exe() {
        let _ =
            ugu_win::association::register(&app, "Ugurugu document", ugu_win::association::CLASSES);
    }
}

fn run(
    document: Option<std::path::PathBuf>,
    start_lock: Option<ugu_win::instance::StartLock>,
) -> Result<(), String> {
    let early_gpu = render::open_gpu_early();
    let settings = settings::Store::load();
    i18n::choose(settings.get().language);
    theme::read_fonts_early();
    ugu_win::pointer::enable_mouse_in_pointer()
        .map_err(|error| format!("cannot route the mouse through pointer input: {error}"))?;
    let event_loop = EventLoop::<app::UiEvent>::with_user_event()
        .build()
        .map_err(|error| format!("cannot create the event loop: {error}"))?;
    let mut app = app::App::new(
        event_loop.create_proxy(),
        early_gpu,
        settings,
        document,
        start_lock,
    );
    event_loop
        .run_app(&mut app)
        .map_err(|error| format!("event loop failed: {error}"))?;
    match app.fatal_error() {
        Some(error) => Err(error.to_owned()),
        None => Ok(()),
    }
}

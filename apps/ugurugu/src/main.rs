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
mod placement;
mod recovery;
mod render;
mod settings;
mod shortcuts;
mod theme;
mod ui;
mod update;
mod widgets;
#[cfg(not(windows))]
mod winit_pointer;

use std::process::ExitCode;

use winit::event_loop::EventLoop;

fn main() -> ExitCode {
    // Velopack's install, update and uninstall hooks start this program with
    // arguments of their own and end it here; a normal start goes on.
    with_install_hooks(velopack::VelopackApp::build()).run();
    let document = app::document_in(std::env::args_os().skip(1));
    // One window, like Clip Studio Paint: a later start hands its document
    // to the window open, or just brings it to the front. The lock is held
    // until this start's own window can take later ones.
    let start_lock = ugu_platform::instance::StartLock::take(app::START_LOCK);
    if ugu_platform::instance::hand_over(
        app::WINDOW_CLASS,
        &app::instance_key(),
        document.as_deref(),
    ) {
        return ExitCode::SUCCESS;
    }
    #[cfg(not(windows))]
    let running = recovery::default_root().map(|root| recovery::take_running(&root));
    #[cfg(not(windows))]
    if matches!(running, Some(Ok(None))) {
        eprintln!("Ugurugu is already running with this recovery folder");
        return ExitCode::SUCCESS;
    }
    let _logging = logs::start();
    tracing::debug!("starting");
    #[cfg(not(windows))]
    if let Some(Err(error)) = &running {
        tracing::warn!(%error, "cannot take the running lock; a second window may start");
    }

    match run(document, start_lock) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            tracing::error!(error, "exiting after a fatal error");
            ExitCode::FAILURE
        }
    }
}

#[cfg(windows)]
fn with_install_hooks(velopack: velopack::VelopackApp<'_>) -> velopack::VelopackApp<'_> {
    velopack
        .on_after_install_fast_callback(|_| associate())
        .on_after_update_fast_callback(|_| associate())
        .on_before_uninstall_fast_callback(|_| {
            ugu_platform::association::unregister(ugu_platform::association::CLASSES)
        })
}

/// Only the Windows package has the hooks.
#[cfg(not(windows))]
fn with_install_hooks(velopack: velopack::VelopackApp<'_>) -> velopack::VelopackApp<'_> {
    velopack
}

/// Opens `.ugurugu` files with this executable.
#[cfg(windows)]
fn associate() {
    if let Ok(app) = std::env::current_exe() {
        let _ = ugu_platform::association::register(
            &app,
            "Ugurugu document",
            ugu_platform::association::CLASSES,
        );
    }
}

fn run(
    document: Option<std::path::PathBuf>,
    start_lock: Option<ugu_platform::instance::StartLock>,
) -> Result<(), String> {
    let early_gpu = render::open_gpu_early();
    let settings = settings::Store::load();
    i18n::choose(settings.get().language);
    theme::read_fonts_early();
    #[cfg(windows)]
    ugu_platform::pointer::enable_mouse_in_pointer()
        .map_err(|error| format!("cannot route the mouse through pointer input: {error}"))?;
    let mut event_loop = EventLoop::<app::UiEvent>::with_user_event();
    // winit's menu quits on Cmd+Q without asking about unsaved changes;
    // without it Cmd+Q reaches the app's own Quit.
    #[cfg(target_os = "macos")]
    winit::platform::macos::EventLoopBuilderExtMacOS::with_default_menu(&mut event_loop, false);
    let event_loop = event_loop
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

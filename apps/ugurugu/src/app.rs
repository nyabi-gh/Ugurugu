// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! The UI thread: owns the window and its message loop and forwards input.
//!
//! It never waits for the render thread while the window can receive
//! messages, because DXGI may send messages to the window during swap chain
//! calls; the render thread reports its exit through the event loop instead.

use std::sync::Arc;
use std::sync::mpsc::{self, Sender};
use std::thread::JoinHandle;

use egui_winit::accesskit_winit;
use ugu_platform::instance::{DocumentReceiver, StartLock};
use ugu_platform::pointer::PointerInput;
use ugu_platform::session::SessionEnd;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoopProxy};
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
use winit::window::{Window, WindowId};

use crate::render::{
    EarlyGpu, EarlyInstance, EguiState, Links, RenderThread, SurfaceSource, ToRender, TreeSink,
    WindowChange, WindowSink,
};
use crate::settings::Store;

/// Every window's class, which later starts look for to hand a document to.
pub const WINDOW_CLASS: &str = "Ugurugu 3";
/// Taken by every start; see [`ugu_platform::instance`].
pub const START_LOCK: &str = r"Local\Ugurugu 3 start";
/// How long the session end waits for unsaved work to be written. Windows
/// offers to end apps that take longer than about five seconds.
const SESSION_END_WAIT: std::time::Duration = std::time::Duration::from_secs(4);

/// Which window a start may hand its document to: one of the same
/// executable with the same settings and recovery folders, so that test runs
/// and the installed app keep apart.
pub fn instance_key() -> String {
    let path = |path: Option<std::path::PathBuf>| {
        path.map(|path| path.display().to_string())
            .unwrap_or_default()
    };
    [
        path(std::env::current_exe().ok()),
        path(crate::settings::default_path()),
        path(crate::recovery::default_root()),
    ]
    .join("\n")
}

pub enum UiEvent {
    /// The render thread ended, with the error that ended it if any.
    RenderStopped(Option<String>),
    /// The render thread needs a new surface for the window.
    NeedSurface(Sender<Result<(wgpu::Instance, wgpu::Surface<'static>), String>>),
    /// A screen reader connected, left, or asked for an action.
    Accessibility(accesskit_winit::Event),
    /// egui's accessibility tree changed.
    AccessibilityTree(egui::accesskit::TreeUpdate),
    Window(WindowChange),
}

impl From<accesskit_winit::Event> for UiEvent {
    fn from(event: accesskit_winit::Event) -> Self {
        Self::Accessibility(event)
    }
}

pub struct App {
    proxy: EventLoopProxy<UiEvent>,
    session: Option<Session>,
    fatal_error: Option<String>,
    /// The GPU being opened since start-up, until the first surface and
    /// render thread take them.
    early_instance: Option<EarlyInstance>,
    early_gpu: Option<EarlyGpu>,
    /// Handed to the render thread, which shows and changes them.
    settings: Option<Store>,
    /// The document named on the command line, opened by the first window.
    document: Option<std::path::PathBuf>,
    /// Released once the window takes documents from later starts.
    start_lock: Option<StartLock>,
}

struct Session {
    // Dropped in this order, the reverse of installing: the document and
    // pointer subclasses (comctl32) sit on top of AccessKit's, which replaces
    // the window procedure directly, and all go before the window.
    _documents: DocumentReceiver,
    /// Keeps unsaved work for recovery when Windows ends the session.
    _session_end: Option<SessionEnd>,
    pointer: PointerInput,
    /// UI Automation provider; it must live on the thread that owns the
    /// window, so egui's tree updates are sent here from the render thread.
    accessibility: accesskit_winit::Adapter,
    window: Arc<Window>,
    to_render: Sender<ToRender>,
    render_thread: Option<JoinHandle<()>>,
}

impl App {
    pub fn new(
        proxy: EventLoopProxy<UiEvent>,
        early: (EarlyInstance, EarlyGpu),
        settings: Store,
        document: Option<std::path::PathBuf>,
        start_lock: Option<StartLock>,
    ) -> Self {
        Self {
            proxy,
            session: None,
            fatal_error: None,
            early_instance: Some(early.0),
            early_gpu: Some(early.1),
            settings: Some(settings),
            document,
            start_lock,
        }
    }

    pub fn fatal_error(&self) -> Option<&str> {
        self.fatal_error.as_deref()
    }
}

/// The window's size before it has a place of its own, and whether it opens
/// maximized: 1600 by 1000, made smaller on a screen of `monitor` (logical)
/// to leave room for the taskbar and the title bar. A screen too small for
/// 1600 by 1000 gets the window maximized, as the panels need all of it
/// (evidence.md 91), with the smaller size to go back to.
fn first_place(monitor: Option<[f64; 2]>) -> (winit::dpi::LogicalSize<f64>, bool) {
    let [width, height] = monitor.map_or([1600.0, 1000.0], |[width, height]| {
        [
            (width * 0.9).min(1600.0).round(),
            (height * 0.8).min(1000.0).round(),
        ]
    });
    let smaller = width < 1600.0 || height < 1000.0;
    (winit::dpi::LogicalSize::new(width, height), smaller)
}

fn hwnd_of(window: &Window) -> Result<isize, String> {
    let handle = window
        .window_handle()
        .map_err(|error| format!("the window has no handle: {error}"))?;
    match handle.as_raw() {
        RawWindowHandle::Win32(handle) => Ok(handle.hwnd.get()),
        _ => Err("the window is not a Win32 window".to_owned()),
    }
}

impl Session {
    fn create(
        event_loop: &ActiveEventLoop,
        proxy: EventLoopProxy<UiEvent>,
        early_gpu: Option<EarlyGpu>,
        settings: Store,
        open_at_start: Option<std::path::PathBuf>,
    ) -> Result<Self, String> {
        use winit::platform::windows::{IconExtWindows, WindowAttributesExtWindows};
        // Resource 1, put in by build.rs.
        let icon = winit::window::Icon::from_resource(1, None).ok();
        let (first_size, first_maximized) =
            first_place(event_loop.primary_monitor().map(|monitor| {
                let size = monitor.size().to_logical::<f64>(monitor.scale_factor());
                [size.width, size.height]
            }));
        let attributes = Window::default_attributes()
            .with_title("Ugurugu")
            .with_class_name(WINDOW_CLASS)
            .with_window_icon(icon.clone())
            .with_taskbar_icon(icon)
            .with_inner_size(first_size)
            // AccessKit must be attached before the window is first shown.
            .with_visible(false);
        let window = Arc::new(
            event_loop
                .create_window(attributes)
                .map_err(|error| format!("cannot create the window: {error}"))?,
        );
        let accessibility =
            accesskit_winit::Adapter::with_event_loop_proxy(event_loop, &window, proxy.clone());
        // SAFETY: the window is alive and owned by this thread, and `Session`
        // drops the subclass before the window.
        let pointer = unsafe { PointerInput::install(hwnd_of(&window)?) }
            .map_err(|error| format!("cannot receive pointer input: {error}"))?;
        // Where it last closed: placed while hidden, then maximized and shown
        // through winit, which keeps track of both.
        if let Some(placement) = settings.get().window {
            // SAFETY: the window is alive and owned by this thread.
            if unsafe { ugu_platform::placement::restore_to(hwnd_of(&window)?, &placement) } {
                window.set_maximized(placement.maximized);
            }
        } else if first_maximized {
            window.set_maximized(true);
        }
        window.set_visible(true);
        tracing::debug!("window shown");

        let egui_ctx = egui::Context::default();
        // Ctrl+Plus, Minus and 0 zoom the canvas, never the interface.
        egui_ctx.options_mut(|options| options.zoom_with_keyboard = false);
        crate::theme::apply(&egui_ctx);
        crate::theme::set_accent(&egui_ctx, settings.get().accent);
        // `UGURUGU_UI_SCALE` draws the interface at that percentage whatever
        // the display's, to check a 100% display's pixels on any PC.
        if let Some(percent) = std::env::var("UGURUGU_UI_SCALE")
            .ok()
            .and_then(|value| value.parse::<f32>().ok())
            .filter(|percent| (50.0..=400.0).contains(percent))
        {
            egui_ctx.set_zoom_factor(percent / 100.0 / window.scale_factor() as f32);
        }
        let egui_state = EguiState::new(egui_winit::State::new(
            egui_ctx.clone(),
            egui::ViewportId::ROOT,
            event_loop,
            Some(window.scale_factor() as f32),
            window.theme(),
            None,
        ));
        let surface_proxy = proxy.clone();
        let surface_source: SurfaceSource = Box::new(move || {
            let (reply, answer) = mpsc::channel();
            surface_proxy
                .send_event(UiEvent::NeedSurface(reply))
                .map_err(|_| "the window is closing".to_owned())?;
            answer
                .recv()
                .map_err(|_| "the window closed before it had a new surface".to_owned())?
        });
        let tree_proxy = proxy.clone();
        let tree_sink: TreeSink = Box::new(move |update| {
            let _ = tree_proxy.send_event(UiEvent::AccessibilityTree(update));
        });
        let window_proxy = proxy.clone();
        let window_sink: WindowSink = Box::new(move |change| {
            let _ = window_proxy.send_event(UiEvent::Window(change));
        });
        let (to_render, messages) = mpsc::channel();
        let to_self = to_render.clone();
        let hwnd = hwnd_of(&window)?;
        let to_open = to_render.clone();
        let open = move |path: Option<std::path::PathBuf>| {
            let taken = match path {
                Some(path) => to_open.send(ToRender::Open(path)).is_ok(),
                None => true,
            };
            if taken {
                // SAFETY: the receiver calls this on the window's thread.
                unsafe { ugu_platform::instance::bring_to_front(hwnd) };
            }
            taken
        };
        // SAFETY: as for the pointer subclass above.
        let documents = unsafe { DocumentReceiver::install(hwnd, &instance_key(), open) }
            .map_err(|error| format!("cannot receive documents from later starts: {error}"))?;
        let to_keep = to_render.clone();
        let ending = move || {
            let (written, on_disk) = mpsc::channel();
            if to_keep.send(ToRender::SessionEnding(written)).is_ok() {
                // Dropped unanswered when there is nothing to write to.
                match on_disk.recv_timeout(SESSION_END_WAIT) {
                    Ok(()) => tracing::info!("unsaved work kept for recovery as the session ends"),
                    Err(mpsc::RecvTimeoutError::Timeout) => {
                        tracing::warn!("unsaved work was not on disk in time as the session ended");
                    }
                    Err(mpsc::RecvTimeoutError::Disconnected) => {}
                }
            }
        };
        // SAFETY: as for the pointer subclass above.
        let session_end = unsafe { SessionEnd::install(hwnd, ending) }
            .inspect_err(|error| tracing::warn!(error, "cannot hear the session end"))
            .ok();
        let render_window = window.clone();
        let render_thread = std::thread::Builder::new()
            .name("render".to_owned())
            .spawn(move || {
                // A panic must still end the event loop, or the window stays
                // open with nothing to answer it.
                let ran = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    RenderThread::create(
                        render_window,
                        Links {
                            surface_source,
                            tree_sink,
                            window_sink,
                            to_self,
                            hwnd,
                        },
                        egui_ctx,
                        egui_state,
                        open_at_start,
                        early_gpu,
                        settings,
                    )
                    .and_then(|render| render.run(&messages))
                }));
                let error = match ran {
                    Ok(result) => result.err(),
                    Err(panic) => Some(format!(
                        "the render thread panicked: {}",
                        panic_message(panic.as_ref())
                    )),
                };
                let _ = proxy.send_event(UiEvent::RenderStopped(error));
            })
            .map_err(|error| format!("cannot start the render thread: {error}"))?;

        Ok(Self {
            _documents: documents,
            _session_end: session_end,
            pointer,
            accessibility,
            window,
            to_render,
            render_thread: Some(render_thread),
        })
    }

    fn forward_pointer_input(&self) {
        let events = self.pointer.drain();
        if !events.is_empty() {
            let _ = self.to_render.send(ToRender::Pointer(events));
        }
    }
}

impl ApplicationHandler<UiEvent> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.session.is_some() {
            return;
        }
        let Some(settings) = self.settings.take() else {
            return;
        };
        match Session::create(
            event_loop,
            self.proxy.clone(),
            self.early_gpu.take(),
            settings,
            self.document.take(),
        ) {
            Ok(session) => {
                self.session = Some(session);
                self.start_lock = None;
            }
            Err(error) => {
                self.fatal_error = Some(error);
                event_loop.exit();
            }
        }
    }

    fn window_event(&mut self, _: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        let Some(session) = self.session.as_ref() else {
            if matches!(event, WindowEvent::CloseRequested) {
                tracing::warn!("close requested before the window was ready; not taken");
            }
            return;
        };
        session.forward_pointer_input();
        let message = match event {
            // Closing waits for the render thread to report back.
            WindowEvent::CloseRequested => {
                tracing::info!("close requested");
                ToRender::CloseRequested
            }
            event => ToRender::Window(event),
        };
        let _ = session.to_render.send(message);
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: UiEvent) {
        let error = match event {
            UiEvent::NeedSurface(reply) => {
                if let Some(session) = self.session.as_ref() {
                    let early = self.early_instance.take().and_then(EarlyInstance::take);
                    let _ = reply.send(RenderThread::create_surface(&session.window, early));
                }
                return;
            }
            UiEvent::Accessibility(event) => {
                if let Some(session) = self.session.as_ref() {
                    let message = match event.window_event {
                        accesskit_winit::WindowEvent::InitialTreeRequested => {
                            ToRender::Accessibility(true)
                        }
                        accesskit_winit::WindowEvent::AccessibilityDeactivated => {
                            ToRender::Accessibility(false)
                        }
                        accesskit_winit::WindowEvent::ActionRequested(request) => {
                            ToRender::AccessibilityAction(request)
                        }
                    };
                    let _ = session.to_render.send(message);
                }
                return;
            }
            UiEvent::AccessibilityTree(update) => {
                if let Some(session) = self.session.as_mut() {
                    session.accessibility.update_if_active(|| update);
                }
                return;
            }
            UiEvent::Window(change) => {
                if let Some(session) = self.session.as_ref() {
                    match change {
                        WindowChange::Title(title) => session.window.set_title(&title),
                        WindowChange::Cursor(Some(icon)) => {
                            session.window.set_cursor(icon);
                            session.window.set_cursor_visible(true);
                        }
                        WindowChange::Cursor(None) => session.window.set_cursor_visible(false),
                    }
                }
                return;
            }
            UiEvent::RenderStopped(error) => error,
        };
        if let Some(mut session) = self.session.take() {
            if let Some(thread) = session.render_thread.take() {
                // The thread already finished; this only collects it.
                let _ = thread.join();
            }
            drop(session);
        }
        if error.is_some() {
            self.fatal_error = error;
        }
        tracing::info!("the window closes");
        event_loop.exit();
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(session) = self.session.as_ref() {
            session.forward_pointer_input();
        }
        event_loop.set_control_flow(ControlFlow::Wait);
    }
}

fn panic_message(payload: &(dyn std::any::Any + Send)) -> &str {
    payload
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
        .unwrap_or("no message")
}

/// The document named on the command line as an absolute path in Windows's
/// own form: the shell, which starts the file dialogs in the document's
/// folder, reads neither `/` nor a relative path.
pub fn document_argument(argument: std::ffi::OsString) -> std::path::PathBuf {
    let path = std::path::PathBuf::from(argument);
    std::path::absolute(&path).unwrap_or(path)
}

/// The first argument that is not an option (one starting with `-`, until a
/// `--`), as a document; options this version does not have are passed by.
pub fn document_in(
    arguments: impl IntoIterator<Item = std::ffi::OsString>,
) -> Option<std::path::PathBuf> {
    let mut options = true;
    arguments
        .into_iter()
        .find(|argument| {
            if options && argument == "--" {
                options = false;
                return false;
            }
            !(options && argument.as_encoded_bytes().starts_with(b"-"))
        })
        .map(document_argument)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn options_are_not_taken_for_a_document() {
        let document = |arguments: &[&str]| document_in(arguments.iter().map(Into::into));
        assert_eq!(document(&[]), None);
        assert_eq!(document(&["--help"]), None);
        let file = r"C:\Drawings\cat.ugurugu";
        assert_eq!(document(&["--later-option", file]), Some(file.into()));
        assert_eq!(
            document(&["--", "-dash.ugurugu"]),
            Some(document_argument("-dash.ugurugu".into()))
        );
    }

    #[test]
    fn the_first_window_fits_a_small_screen_and_fills_it() {
        let place = |monitor| {
            let (size, maximized) = first_place(monitor);
            ([size.width, size.height], maximized)
        };
        assert_eq!(place(None), ([1600.0, 1000.0], false));
        assert_eq!(place(Some([2560.0, 1440.0])), ([1600.0, 1000.0], false));
        // 1920 by 1080 at 150% and at 100%, and 1366 by 768 at 100%.
        assert_eq!(place(Some([1280.0, 720.0])), ([1152.0, 576.0], true));
        assert_eq!(place(Some([1920.0, 1080.0])), ([1600.0, 864.0], true));
        assert_eq!(place(Some([1366.0, 768.0])), ([1229.0, 614.0], true));
    }

    #[test]
    fn a_document_argument_is_made_absolute_in_windows_form() {
        assert_eq!(
            document_argument(r"C:/Pictures/cats\tabby.ugurugu".into()),
            std::path::PathBuf::from(r"C:\Pictures\cats\tabby.ugurugu")
        );
        let relative = document_argument("tabby.ugurugu".into());
        assert_eq!(
            relative,
            std::env::current_dir().unwrap().join("tabby.ugurugu")
        );
    }
}

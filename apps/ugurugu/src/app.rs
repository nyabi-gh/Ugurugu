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

use ugu_win::composition::{self, SurfaceChild};
use ugu_win::pointer::PointerInput;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoopProxy};
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
use winit::window::{Window, WindowId};

use crate::render::{CanvasSurface, RenderThread, ToRender};

/// How the window's swap chains are laid out, chosen with `UGURUGU_PRESENT`
/// while measuring which layout DWM shows without composing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PresentLayout {
    /// One swap chain for the whole client area.
    Window,
    /// As `Window`, with the rounded corners turned off.
    SquareWindow,
    /// The canvas in its own child window swap chain.
    CanvasChild,
}

impl PresentLayout {
    fn from_env() -> Self {
        match std::env::var("UGURUGU_PRESENT").as_deref() {
            Ok("square") => Self::SquareWindow,
            Ok("child") => Self::CanvasChild,
            _ => Self::Window,
        }
    }
}

pub enum UiEvent {
    /// The render thread ended, with the error that ended it if any.
    RenderStopped(Option<String>),
}

pub struct App {
    proxy: EventLoopProxy<UiEvent>,
    session: Option<Session>,
    fatal_error: Option<String>,
}

struct Session {
    // Dropped first: the subclass must go before the window it is attached to.
    pointer: PointerInput,
    // Destroyed after the render thread dropped its swap chain, before the parent.
    _canvas_child: Option<SurfaceChild>,
    _window: Arc<Window>,
    to_render: Sender<ToRender>,
    render_thread: Option<JoinHandle<()>>,
}

impl App {
    pub fn new(proxy: EventLoopProxy<UiEvent>) -> Self {
        Self {
            proxy,
            session: None,
            fatal_error: None,
        }
    }

    pub fn fatal_error(&self) -> Option<&str> {
        self.fatal_error.as_deref()
    }
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
    ) -> Result<Self, String> {
        let attributes = Window::default_attributes()
            .with_title("Ugurugu")
            .with_inner_size(winit::dpi::LogicalSize::new(1600.0, 1000.0));
        let window = Arc::new(
            event_loop
                .create_window(attributes)
                .map_err(|error| format!("cannot create the window: {error}"))?,
        );
        let hwnd = hwnd_of(&window)?;
        // SAFETY: the window is alive and owned by this thread, and `Session`
        // drops the subclass before the window.
        let pointer = unsafe { PointerInput::install(hwnd) }
            .map_err(|error| format!("cannot receive pointer input: {error}"))?;
        let layout = PresentLayout::from_env();
        tracing::info!(?layout, "present layout");
        if layout == PresentLayout::SquareWindow {
            composition::set_square_corners(hwnd)
                .map_err(|error| format!("cannot square the window corners: {error}"))?;
        }
        let canvas_child = match layout {
            // SAFETY: the parent is alive on this thread, and `Session` drops
            // the child on this thread after the render thread has ended.
            PresentLayout::CanvasChild => Some(
                unsafe { SurfaceChild::create(hwnd) }
                    .map_err(|error| format!("cannot create the canvas window: {error}"))?,
            ),
            _ => None,
        };

        let egui_ctx = egui::Context::default();
        let egui_state = egui_winit::State::new(
            egui_ctx.clone(),
            egui::ViewportId::ROOT,
            event_loop,
            Some(window.scale_factor() as f32),
            window.theme(),
            None,
        );
        let (instance, surface) = RenderThread::create_surface(&window)?;
        let canvas_surface = match &canvas_child {
            Some(child) => Some(CanvasSurface::create(&instance, child.hwnd())?),
            None => None,
        };
        let (to_render, messages) = mpsc::channel();
        let render_window = window.clone();
        let render_thread = std::thread::Builder::new()
            .name("render".to_owned())
            .spawn(move || {
                let error = match RenderThread::create(
                    render_window,
                    &instance,
                    surface,
                    canvas_surface,
                    egui_ctx,
                    egui_state,
                ) {
                    Ok(render) => {
                        render.run(&messages);
                        None
                    }
                    Err(error) => Some(error),
                };
                let _ = proxy.send_event(UiEvent::RenderStopped(error));
            })
            .map_err(|error| format!("cannot start the render thread: {error}"))?;

        Ok(Self {
            pointer,
            _canvas_child: canvas_child,
            _window: window,
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
        match Session::create(event_loop, self.proxy.clone()) {
            Ok(session) => self.session = Some(session),
            Err(error) => {
                self.fatal_error = Some(error);
                event_loop.exit();
            }
        }
    }

    fn window_event(&mut self, _: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        let Some(session) = self.session.as_ref() else {
            return;
        };
        session.forward_pointer_input();
        let message = match event {
            // Closing waits for the render thread to report back.
            WindowEvent::CloseRequested => ToRender::Shutdown,
            event => ToRender::Window(event),
        };
        let _ = session.to_render.send(message);
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: UiEvent) {
        let UiEvent::RenderStopped(error) = event;
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
        event_loop.exit();
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(session) = self.session.as_ref() {
            session.forward_pointer_input();
        }
        event_loop.set_control_flow(ControlFlow::Wait);
    }
}

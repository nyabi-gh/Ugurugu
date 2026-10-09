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
use ugu_win::pointer::PointerInput;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoopProxy};
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
use winit::window::{Window, WindowId};

use crate::render::{
    EarlyGpu, EarlyInstance, EguiState, Links, RenderThread, SurfaceSource, ToRender, TreeSink,
};

pub enum UiEvent {
    /// The render thread ended, with the error that ended it if any.
    RenderStopped(Option<String>),
    /// The render thread needs a new surface for the window.
    NeedSurface(Sender<Result<(wgpu::Instance, wgpu::Surface<'static>), String>>),
    /// A screen reader connected, left, or asked for an action.
    Accessibility(accesskit_winit::Event),
    /// egui's accessibility tree changed.
    AccessibilityTree(egui::accesskit::TreeUpdate),
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
}

struct Session {
    // Dropped in this order, the reverse of installing: the pointer subclass
    // (comctl32) sits on top of AccessKit's, which replaces the window
    // procedure directly, and both go before the window.
    pointer: PointerInput,
    /// UI Automation provider; it must live on the thread that owns the
    /// window, so egui's tree updates are sent here from the render thread.
    accessibility: accesskit_winit::Adapter,
    window: Arc<Window>,
    to_render: Sender<ToRender>,
    render_thread: Option<JoinHandle<()>>,
}

impl App {
    pub fn new(proxy: EventLoopProxy<UiEvent>, early: (EarlyInstance, EarlyGpu)) -> Self {
        Self {
            proxy,
            session: None,
            fatal_error: None,
            early_instance: Some(early.0),
            early_gpu: Some(early.1),
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
        early_gpu: Option<EarlyGpu>,
    ) -> Result<Self, String> {
        let attributes = Window::default_attributes()
            .with_title("Ugurugu")
            .with_inner_size(winit::dpi::LogicalSize::new(1600.0, 1000.0))
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
        window.set_visible(true);
        tracing::debug!("window shown");

        let egui_ctx = egui::Context::default();
        crate::theme::apply(&egui_ctx);
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
        let (to_render, messages) = mpsc::channel();
        let to_self = to_render.clone();
        let hwnd = hwnd_of(&window)?;
        let open_at_start = std::env::args_os().nth(1).map(std::path::PathBuf::from);
        let render_window = window.clone();
        let render_thread = std::thread::Builder::new()
            .name("render".to_owned())
            .spawn(move || {
                let error = RenderThread::create(
                    render_window,
                    Links {
                        surface_source,
                        tree_sink,
                        to_self,
                        hwnd,
                    },
                    egui_ctx,
                    egui_state,
                    open_at_start,
                    early_gpu,
                )
                .and_then(|render| render.run(&messages))
                .err();
                let _ = proxy.send_event(UiEvent::RenderStopped(error));
            })
            .map_err(|error| format!("cannot start the render thread: {error}"))?;

        Ok(Self {
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
        match Session::create(event_loop, self.proxy.clone(), self.early_gpu.take()) {
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
            WindowEvent::CloseRequested => ToRender::CloseRequested,
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
        event_loop.exit();
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(session) = self.session.as_ref() {
            session.forward_pointer_input();
        }
        event_loop.set_control_flow(ControlFlow::Wait);
    }
}

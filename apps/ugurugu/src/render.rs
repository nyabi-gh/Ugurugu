// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! The render thread: owns the GPU, egui and the canvas, and paces frames.
//!
//! The UI thread only forwards window and pointer events here, so it never
//! waits on the GPU or the display. A frame waits for the swap chain first and
//! reads the input queued meanwhile afterwards, so it shows the newest input.

use std::sync::Arc;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

use ugu_render::ants::Ants;
use ugu_render::gpu::{AdapterChoice, Gpu};
use ugu_render::present::{Acquired, Presenter};
use ugu_render::view::{DocumentView, Placement};
use ugu_win::clock::Ticks;
use ugu_win::pointer::PointerEvent;
use winit::event::WindowEvent;
use winit::keyboard::ModifiersState;
use winit::window::Window;

use crate::cache::Rendered;
use crate::canvas::{self, Canvas};
use crate::clipboard::{Clipboard, ClipboardEvent};
use crate::files::{Action, FileEvent, Files};
use crate::ime_probe::ImeProbe;
use crate::input::{CanvasInput, InputRouter};
use crate::latency::LatencyLog;
use crate::settings::Store;
use crate::shortcuts::{self, Chord};
use crate::theme;
use crate::ui::{self, Panels};

pub enum ToRender {
    Window(WindowEvent),
    Pointer(Vec<PointerEvent>),
    /// A screen reader connected (`true`) or left.
    Accessibility(bool),
    AccessibilityAction(egui::accesskit::ActionRequest),
    /// The cache worker finished a render.
    Cache(Box<Rendered>),
    /// A file dialog or the file thread finished.
    File(Box<FileEvent>),
    /// The clipboard thread finished.
    Clipboard(Box<ClipboardEvent>),
    /// The user asked to close the window; unsaved changes come first.
    CloseRequested,
}

/// How long to sleep when nothing is scheduled; any message wakes it earlier.
const IDLE_WAKE: Duration = Duration::from_secs(1);
/// Display times are read back this often while presents await them.
const DISPLAY_POLL: Duration = Duration::from_millis(2);
const FRAME_WAIT_LIMIT: Duration = Duration::from_millis(100);
/// How long a swap chain made after a device loss may hold back the next
/// frame before it is made again, and how many times.
const STALL_LIMIT: Duration = Duration::from_secs(1);
const RECOVERY_ATTEMPTS: u32 = 3;

/// Makes a new instance and a surface for the window with it. Only the UI
/// thread may read the window handle, so this asks it and waits for the
/// answer.
pub type SurfaceSource =
    Box<dyn Fn() -> Result<(wgpu::Instance, wgpu::Surface<'static>), String> + Send>;

/// egui-winit's input state on its way to the render thread. With the
/// `accesskit` feature it can hold an AccessKit adapter, which is not `Send`.
/// This app keeps the adapter on the UI thread instead, so the state's own
/// adapter slot stays empty and nothing in it is tied to the UI thread.
pub struct EguiState(egui_winit::State);

impl EguiState {
    pub fn new(state: egui_winit::State) -> Self {
        assert!(
            state.accesskit.is_none(),
            "the AccessKit adapter belongs to the UI thread"
        );
        Self(state)
    }
}

// SAFETY: the only part that is not `Send`, the AccessKit adapter, is absent
// (checked in `new`) and never set: the render thread does not call
// `init_accesskit`.
unsafe impl Send for EguiState {}

/// Hands egui's accessibility tree updates to the UI thread, which owns the
/// UI Automation provider.
pub type TreeSink = Box<dyn Fn(egui::accesskit::TreeUpdate) + Send>;

/// The graphics instance, for the UI thread to make the first surface with.
pub struct EarlyInstance(Receiver<wgpu::Instance>);

/// The GPU opened while the window was being made.
pub struct EarlyGpu(std::thread::JoinHandle<Result<Gpu, String>>);

/// Starts opening the GPU on a thread of its own. Loading the drivers of
/// every adapter wgpu looks at takes most of start-up (m0-evidence section
/// 14) and needs no window.
pub fn open_gpu_early() -> (EarlyInstance, EarlyGpu) {
    let (instance_out, instance_in) = std::sync::mpsc::sync_channel(1);
    let gpu = std::thread::Builder::new()
        .name("gpu".to_owned())
        .spawn(move || {
            let instance = RenderThread::create_instance();
            tracing::debug!("graphics instance created");
            let _ = instance_out.send(instance.clone());
            Gpu::open_without_surface(&instance, AdapterChoice::from_env())
        })
        .expect("cannot start the GPU thread");
    (EarlyInstance(instance_in), EarlyGpu(gpu))
}

impl EarlyInstance {
    pub fn take(self) -> Option<wgpu::Instance> {
        self.0.recv().ok()
    }
}

/// How the render thread reaches the UI thread and the window.
pub struct Links {
    pub surface_source: SurfaceSource,
    pub tree_sink: TreeSink,
    /// The render thread's own message queue, for work finished elsewhere.
    pub to_self: Sender<ToRender>,
    /// The window handle, for dialogs to belong to.
    pub hwnd: isize,
}

pub struct RenderThread {
    window: Arc<Window>,
    egui_ctx: egui::Context,
    egui_state: egui_winit::State,
    surface_source: SurfaceSource,
    tree_sink: TreeSink,
    adapter_choice: AdapterChoice,
    display: Display,
    /// `UGURUGU_DIAGNOSTICS=1` enables test-only keys.
    diagnostics: bool,
    router: InputRouter,
    canvas: Canvas,
    ime: ImeProbe,
    panels: Panels,
    files: Files,
    settings: Store,
    clipboard: Clipboard,
    modifiers: ModifiersState,
    /// Copy, cut and paste chords since the last frame, which egui does not
    /// see as keys.
    clipboard_chords: Vec<Chord>,
    title: String,
    present_latency: LatencyLog,
    display_latency: LatencyLog,
    needs_frame: bool,
    repaint_at: Option<Instant>,
    /// Nothing is drawn, played or uploaded while the window is minimized.
    minimized: bool,
    /// Set after a lost device is replaced, until the new swap chain lets a
    /// second frame through. After a driver reset the display can still be
    /// resetting when the new swap chain is made, and such a swap chain shows
    /// one frame and never signals for another.
    recovery: Option<Recovery>,
}

struct Recovery {
    attempts: u32,
    frames: u32,
    stalled_since: Option<Instant>,
}

/// Everything made with one GPU device, replaced together when it is lost.
/// The canvas raster and egui's state live on the CPU and survive.
struct Display {
    gpu: Gpu,
    presenter: Presenter,
    egui_renderer: egui_wgpu::Renderer,
    canvas_view: DocumentView,
    ants: Ants,
    /// What the last presented frame shows, to skip frames that would
    /// present the same picture again.
    shown: Option<Shown>,
    /// A frame waited for but not presented, which the next frame uses
    /// instead of waiting for another.
    slot_held: bool,
}

#[derive(PartialEq)]
struct Shown {
    shapes: Vec<egui::epaint::ClippedShape>,
    pixels_per_point: f32,
    canvas_area: [u32; 4],
    placement: Placement,
    canvas_size: [u32; 2],
}

impl Display {
    fn open(
        instance: &wgpu::Instance,
        surface: wgpu::Surface<'static>,
        choice: AdapterChoice,
        size: [u32; 2],
    ) -> Result<Self, String> {
        Self::with_gpu(Gpu::open(instance, &surface, choice)?, surface, size)
    }

    /// Uses the GPU opened early if it can show the window, and otherwise
    /// opens one that can.
    fn open_with_early(
        early: EarlyGpu,
        instance: &wgpu::Instance,
        surface: wgpu::Surface<'static>,
        choice: AdapterChoice,
        size: [u32; 2],
    ) -> Result<Self, String> {
        match early.0.join() {
            Ok(Ok(gpu)) if gpu.can_show(&surface) => Self::with_gpu(gpu, surface, size),
            Ok(Ok(gpu)) => {
                tracing::warn!(adapter = %gpu.summary, "the adapter opened early cannot show the window");
                Self::open(instance, surface, choice, size)
            }
            Ok(Err(error)) => {
                tracing::warn!(error, "opening the GPU early failed");
                Self::open(instance, surface, choice, size)
            }
            Err(_) => Err("the GPU thread panicked".to_owned()),
        }
    }

    fn with_gpu(gpu: Gpu, surface: wgpu::Surface<'static>, size: [u32; 2]) -> Result<Self, String> {
        let presenter = Presenter::new(surface, &gpu.adapter, &gpu.device, size)?;
        let egui_renderer = egui_wgpu::Renderer::new(
            &gpu.device,
            presenter.format(),
            egui_wgpu::RendererOptions::default(),
        );
        let canvas_view = DocumentView::new(&gpu.device, presenter.format(), canvas::WORKSPACE);
        let ants = Ants::new(&gpu.device, presenter.format());
        Ok(Self {
            gpu,
            presenter,
            egui_renderer,
            canvas_view,
            ants,
            shown: None,
            slot_held: false,
        })
    }

    fn wait_for_frame(&mut self) -> bool {
        self.slot_held = self.slot_held || self.presenter.wait_for_frame(FRAME_WAIT_LIMIT);
        self.slot_held
    }
}

impl RenderThread {
    pub fn create_instance() -> wgpu::Instance {
        wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::DX12,
            flags: wgpu::InstanceFlags::from_build_config().with_env(),
            memory_budget_thresholds: wgpu::MemoryBudgetThresholds::default(),
            backend_options: ugu_render::present::backend_options(),
            display: None,
        })
    }

    /// Called on the UI thread, the only thread winit lets read the window
    /// handle. The first surface uses the instance the GPU was opened early
    /// with; later ones get a new instance: after a driver reset, swap chains
    /// made through an older instance's DXGI factory never reach the screen.
    pub fn create_surface(
        window: &Arc<Window>,
        early: Option<wgpu::Instance>,
    ) -> Result<(wgpu::Instance, wgpu::Surface<'static>), String> {
        let instance = early.unwrap_or_else(Self::create_instance);
        let surface = instance
            .create_surface(window.clone())
            .map_err(|error| format!("cannot create the window surface: {error}"))?;
        tracing::debug!("window surface created");
        Ok((instance, surface))
    }

    pub fn create(
        window: Arc<Window>,
        links: Links,
        egui_ctx: egui::Context,
        EguiState(egui_state): EguiState,
        open_at_start: Option<std::path::PathBuf>,
        early: Option<EarlyGpu>,
        settings: Store,
    ) -> Result<Self, String> {
        let Links {
            surface_source,
            tree_sink,
            to_self,
            hwnd,
        } = links;
        let adapter_choice = AdapterChoice::from_env();
        let size = window.inner_size();
        let (instance, surface) = surface_source()?;
        let size = [size.width, size.height];
        let display = match early {
            Some(early) => {
                Display::open_with_early(early, &instance, surface, adapter_choice, size)
            }
            None => Display::open(&instance, surface, adapter_choice, size),
        }?;
        let mut render = Self {
            window,
            egui_ctx,
            egui_state,
            surface_source,
            tree_sink,
            adapter_choice,
            display,
            diagnostics: std::env::var_os("UGURUGU_DIAGNOSTICS").is_some_and(|value| value == "1"),
            router: InputRouter::default(),
            canvas: Canvas::new(Files::new_canvas(), {
                let to_self = to_self.clone();
                move |rendered| {
                    let _ = to_self.send(ToRender::Cache(Box::new(rendered)));
                }
            }),
            modifiers: ModifiersState::empty(),
            clipboard_chords: Vec::new(),
            clipboard: Clipboard::new({
                let to_self = to_self.clone();
                move |event| {
                    let _ = to_self.send(ToRender::Clipboard(Box::new(event)));
                }
            }),
            files: {
                let mut files = Files::new(hwnd, move |event| {
                    let _ = to_self.send(ToRender::File(Box::new(event)));
                });
                files.start_recovery(crate::recovery::default_root());
                if let Some(path) = open_at_start {
                    files.open_path(path);
                }
                files
            },
            settings,
            title: String::new(),
            ime: ImeProbe::default(),
            panels: Panels::default(),
            present_latency: LatencyLog::default(),
            display_latency: LatencyLog::default(),
            needs_frame: true,
            repaint_at: None,
            minimized: false,
            recovery: None,
        };
        ui::apply_settings(
            &render.egui_ctx,
            &render.settings,
            &mut render.canvas,
            &mut render.files,
            &mut render.panels,
        );
        Ok(render)
    }

    /// Replaces everything made with a lost device. The next frame shows what
    /// was there, because the canvas raster and egui's state are on the CPU.
    fn recover(mut self) -> Result<Self, String> {
        let started = Instant::now();
        let size = self.window.inner_size();
        // The old swap chain still holds the window and cannot be moved to a
        // new device, so it goes before the window gets a new one.
        drop(self.display);
        let dropped = started.elapsed();
        let (instance, surface) = (self.surface_source)()?;
        let surfaced = started.elapsed();
        self.display = Display::open(
            &instance,
            surface,
            self.adapter_choice,
            [size.width, size.height],
        )?;
        self.canvas.upload_all();
        self.panels.layers.forget_textures();
        // egui sends only changes to its font atlas, so the new device gets
        // the whole atlas once.
        let atlas = self.egui_ctx.fonts(|fonts| fonts.image());
        let options = self
            .egui_ctx
            .tex_manager()
            .read()
            .meta(egui::TextureId::default())
            .map(|meta| meta.options)
            .unwrap_or_default();
        self.display.egui_renderer.update_texture(
            &self.display.gpu.device,
            &self.display.gpu.queue,
            egui::TextureId::default(),
            &egui::epaint::ImageDelta::full(atlas, options),
        );
        self.needs_frame = true;
        self.recovery = Some(Recovery {
            attempts: self
                .recovery
                .as_ref()
                .map_or(1, |recovery| recovery.attempts + 1),
            frames: 0,
            stalled_since: None,
        });
        let ms = |elapsed: Duration| elapsed.as_secs_f64() * 1000.0;
        tracing::info!(
            drop_ms = ms(dropped),
            surface_ms = ms(surfaced - dropped),
            total_ms = ms(started.elapsed()),
            "GPU device replaced"
        );
        Ok(self)
    }

    pub fn run(mut self, messages: &Receiver<ToRender>) -> Result<(), String> {
        loop {
            if self.display.gpu.check_removed() {
                self = self.recover()?;
            }
            let first = if self.needs_frame {
                messages.try_recv().map_err(|error| match error {
                    std::sync::mpsc::TryRecvError::Empty => RecvTimeoutError::Timeout,
                    std::sync::mpsc::TryRecvError::Disconnected => RecvTimeoutError::Disconnected,
                })
            } else {
                messages.recv_timeout(self.wake_timeout())
            };
            match first {
                Ok(message) => {
                    if !self.apply(message) {
                        break;
                    }
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
            if !self.apply_queued(messages) {
                break;
            }
            if self.minimized {
                self.needs_frame = false;
                self.repaint_at = None;
            }
            if self.repaint_at.is_some_and(|at| at <= Instant::now()) {
                self.needs_frame = true;
            }
            if self.needs_frame && self.display.wait_for_frame() {
                if self
                    .recovery
                    .as_ref()
                    .is_some_and(|recovery| recovery.frames > 0)
                {
                    self.recovery = None;
                }
                if !self.apply_queued(messages) {
                    break;
                }
                self.frame();
                if let Some(recovery) = self.recovery.as_mut() {
                    recovery.frames += 1;
                }
            } else if self.needs_frame
                && let Some(recovery) = self.recovery.as_mut()
                && recovery.frames > 0
                && recovery
                    .stalled_since
                    .get_or_insert_with(Instant::now)
                    .elapsed()
                    >= STALL_LIMIT
            {
                if recovery.attempts < RECOVERY_ATTEMPTS {
                    tracing::warn!(
                        attempt = recovery.attempts,
                        "the new swap chain shows nothing; replacing it again"
                    );
                    self = self.recover()?;
                } else {
                    tracing::error!("the display did not come back after a device loss");
                    self.recovery = None;
                }
            }
            self.collect_display_times();
            self.files.keep_recovery(&mut self.canvas, Instant::now());
            if self.files.should_close() {
                // Only a normal close drops the work kept for recovery.
                self.files.end();
                break;
            }
        }
        if let Some(summary) = self.display_latency.summary() {
            tracing::info!(%summary, "input to display latency");
        }
        if let Some(summary) = self.present_latency.summary() {
            tracing::info!(%summary, samples = self.canvas.sample_count(), "input to present-return latency");
        }
        Ok(())
    }

    fn wake_timeout(&self) -> Duration {
        let mut timeout = IDLE_WAKE;
        for at in [self.repaint_at, self.files.recovery_due()]
            .into_iter()
            .flatten()
        {
            timeout = timeout.min(at.saturating_duration_since(Instant::now()));
        }
        if self.display.presenter.has_pending_display_times() {
            timeout = timeout.min(DISPLAY_POLL);
        }
        timeout
    }

    /// Returns `false` once the UI thread asked to stop.
    fn apply_queued(&mut self, messages: &Receiver<ToRender>) -> bool {
        while let Ok(message) = messages.try_recv() {
            if !self.apply(message) {
                return false;
            }
        }
        true
    }

    fn apply(&mut self, message: ToRender) -> bool {
        match message {
            ToRender::CloseRequested => {
                self.files.request(Action::Close, &mut self.canvas);
                self.needs_frame = true;
            }
            ToRender::Clipboard(event) => {
                self.clipboard.handle(*event, &mut self.canvas);
                self.needs_frame = true;
            }
            ToRender::File(event) => {
                self.files.handle(*event, &mut self.canvas);
                self.needs_frame = true;
            }
            ToRender::Accessibility(active) => {
                if active {
                    self.egui_ctx.enable_accesskit();
                } else {
                    self.egui_ctx.disable_accesskit();
                }
                self.needs_frame = true;
            }
            ToRender::AccessibilityAction(request) => {
                self.egui_state.on_accesskit_action_request(request);
                self.needs_frame = true;
            }
            ToRender::Cache(rendered) => {
                self.canvas.adopt(*rendered);
                self.needs_frame = true;
            }
            ToRender::Pointer(events) => {
                let pixels_per_point = egui_winit::pixels_per_point(&self.egui_ctx, &self.window);
                let (modifiers, space) = self
                    .egui_ctx
                    .input(|input| (input.modifiers, input.key_down(egui::Key::Space)));
                let pan_held = space && !self.egui_ctx.text_edit_focused();
                self.canvas.set_modifiers(modifiers.shift, modifiers.alt);
                let canvas = &self.canvas;
                let egui_ctx = &self.egui_ctx;
                // Menus, popups and the selection bar float over the canvas
                // and take presses there; the canvas itself is under egui's
                // background layer.
                let under_egui = |[x, y]: [f64; 2]| {
                    let point = egui::pos2(x as f32, y as f32) / pixels_per_point;
                    egui_ctx
                        .layer_id_at(point)
                        .is_some_and(|layer| layer.order != egui::Order::Background)
                };
                let routed =
                    self.router
                        .route(events, pixels_per_point, modifiers, pan_held, |position| {
                            canvas.contains(position) && !under_egui(position)
                        });
                self.egui_state.egui_input_mut().events.extend(routed.egui);
                for input in routed.canvas {
                    // egui never sees canvas presses, so it would keep a
                    // text field focused and take the canvas's keys.
                    if matches!(input, CanvasInput::Begin(..)) {
                        self.egui_ctx.memory_mut(|memory| memory.stop_text_input());
                    }
                    self.canvas.apply(input);
                }
                self.needs_frame = true;
            }
            // egui-winit asks to repaint on every redraw; following that would
            // redraw forever, so a redraw request only schedules one frame.
            ToRender::Window(WindowEvent::RedrawRequested) => self.needs_frame = true,
            ToRender::Window(event) => {
                match &event {
                    WindowEvent::ModifiersChanged(modifiers) => {
                        self.modifiers = modifiers.state();
                    }
                    // egui-winit turns these into clipboard events, Ctrl+V
                    // only when the clipboard holds text, and an image from
                    // another app is pasted too.
                    WindowEvent::KeyboardInput { event, .. }
                        if event.state.is_pressed() && !event.repeat =>
                    {
                        self.clipboard_chords.extend(shortcuts::clipboard_chord(
                            &event.logical_key,
                            event.physical_key,
                            self.modifiers,
                        ));
                    }
                    _ => {}
                }
                if let WindowEvent::Resized(size) = &event {
                    // Windows reports a minimized window as zero-sized.
                    self.minimized = size.width == 0 || size.height == 0;
                    self.display
                        .presenter
                        .resize(&self.display.gpu.device, [size.width, size.height]);
                    self.display.shown = None;
                    self.needs_frame = true;
                }
                if self
                    .egui_state
                    .on_window_event(&self.window, &event)
                    .repaint
                {
                    self.needs_frame = true;
                }
            }
        }
        true
    }

    fn frame(&mut self) {
        let started = Instant::now();
        let input = self.egui_state.take_egui_input(&self.window);
        let mut canvas_area = [0; 4];
        let mut shown_ants = None;
        let typed = std::mem::take(&mut self.clipboard_chords);
        let Self {
            display,
            canvas,
            display_latency,
            ime,
            panels,
            files,
            clipboard,
            diagnostics,
            settings,
            ..
        } = self;
        let adapter_summary = &display.gpu.summary;
        let software = display.gpu.is_software();
        let mut remove_device = false;
        let output = self.egui_ctx.run_ui(input, |ui| {
            // Before the widgets run, so focus is what the key was pressed in.
            ui::shortcuts(ui.ctx(), canvas, files, clipboard, panels, &typed);
            files.confirm(ui.ctx(), canvas);
            files.ask_recovery(ui.ctx(), canvas);
            files.ask_animation(ui.ctx(), canvas);
            ui::dialogs(ui.ctx(), canvas, files, settings, panels, &typed);
            remove_device =
                *diagnostics && ui.ctx().input(|input| input.key_pressed(egui::Key::F9));
            let bar = |fill, x, y| {
                egui::Frame::new()
                    .fill(fill)
                    .inner_margin(egui::Margin::symmetric(x, y))
            };
            let dock = egui::Frame::new()
                .fill(theme::PANEL)
                .inner_margin(egui::Margin::same(8));
            egui::Panel::top("menu")
                .frame(bar(theme::CHROME, 6, 2))
                .show_separator_line(false)
                .show(ui, |ui| ui::menu_bar(ui, canvas, files, clipboard, panels));
            egui::Panel::top("quick access")
                .frame(bar(theme::CHROME, 10, 5))
                .show(ui, |ui| {
                    ui::quick_access(ui, canvas, files, clipboard, panels)
                });
            egui::Panel::bottom("status")
                .frame(bar(theme::STATUS, 8, 2))
                .show_separator_line(false)
                .show(ui, |ui| {
                    let warning =
                        software.then_some("No usable GPU: drawing with the slow software display");
                    let message = warning
                        .or(canvas.notice())
                        .map(|text| (text, true))
                        .or(files.message().map(|text| (text, false)));
                    let message = message.map(|(text, warn)| (text.to_owned(), warn));
                    if let Some(due) = files.message_due() {
                        ui.ctx()
                            .request_repaint_after(due.saturating_duration_since(Instant::now()));
                    }
                    let pointer = panels.pointer;
                    let export = files.export_status();
                    if export.is_some() {
                        // The progress moves on without input.
                        ui.ctx().request_repaint_after(Duration::from_millis(200));
                    }
                    let cancel = ui::status_bar(
                        ui,
                        canvas,
                        message.as_ref().map(|(text, warn)| (text.as_str(), *warn)),
                        export.as_deref(),
                        pointer,
                    );
                    if cancel {
                        files.cancel_export();
                    }
                    if *diagnostics {
                        ui.horizontal(|ui| {
                            ui.label(adapter_summary.as_str());
                            ui.separator();
                            ui.label(format!("samples {}", canvas.sample_count()));
                            if let Some(summary) = display_latency.summary() {
                                ui.separator();
                                ui.label(format!("input to display {summary}"));
                            }
                        });
                    }
                });
            egui::Panel::left("tool rail")
                .resizable(false)
                .exact_size(48.0)
                .frame(bar(theme::CHROME, 4, 8))
                .show(ui, |ui| ui::rail(ui, canvas, &panels.keys));
            let left = panels.shown;
            if left.tool_settings || left.color || left.color_history {
                egui::Panel::left("tool settings")
                    .resizable(true)
                    .default_size(260.0)
                    .size_range(150.0..=460.0)
                    .frame(dock)
                    .show(ui, |ui| {
                        if panels.shown.tool_settings {
                            ui::tool_settings(ui, canvas, panels);
                            ui.separator();
                        }
                        if panels.shown.color {
                            ui::color(ui, canvas, panels);
                        }
                        if panels.shown.color_history {
                            if left.color {
                                ui.separator();
                            }
                            ui::color_history(ui, canvas, panels);
                        }
                    });
            }
            let shown = panels.shown;
            if shown.wobble || shown.layers || *diagnostics {
                egui::Panel::right("docks")
                    .resizable(true)
                    .default_size(300.0)
                    .size_range(150.0..=460.0)
                    .frame(dock)
                    .show(ui, |ui| {
                        if shown.wobble {
                            ui::wobble(ui, canvas, panels);
                            ui.separator();
                        }
                        if *diagnostics {
                            ime.show(ui);
                            ui.separator();
                        }
                        if shown.layers {
                            ui::layers(ui, canvas, panels);
                        }
                    });
            }
            if panels.shown.animation_bar {
                egui::Panel::bottom("animation bar")
                    .frame(
                        egui::Frame::new()
                            .fill(theme::CHROME)
                            .inner_margin(egui::Margin {
                                left: 12,
                                right: 14,
                                top: 9,
                                bottom: 9,
                            }),
                    )
                    .show(ui, |ui| ui::animation_bar(ui, canvas, panels));
            }
            // No panel fill: the canvas is drawn under egui.
            egui::CentralPanel::default()
                .frame(egui::Frame::NONE)
                .show(ui, |ui| {
                    canvas_area = canvas.layout(ui);
                    shown_ants = ui::selection_overlay(ui, canvas);
                    ui::text_overlay(ui, canvas);
                    ui::pick_cursor(ui, canvas);
                    ui::selection_actions(ui, canvas, panels);
                    let ppp = f64::from(ui.ctx().pixels_per_point());
                    let area = ui.max_rect();
                    panels.pointer = ui
                        .input(|input| input.pointer.hover_pos())
                        .filter(|pointer| area.contains(*pointer))
                        .map(|pointer| {
                            canvas.document_point([
                                f64::from(pointer.x) * ppp,
                                f64::from(pointer.y) * ppp,
                            ])
                        });
                });
        });
        // Tools change through the panels, shortcuts and the canvas.
        let tools = self.canvas.session().tools();
        if tools != self.settings.get().tools {
            self.settings.change(|settings| settings.tools = tools);
        }
        let laid_out = Instant::now();
        let mut platform_output = output.platform_output;
        if let Some(update) = platform_output.accesskit_update.take() {
            (self.tree_sink)(update);
        }
        // egui-winit gives the IME the whole text field to avoid, so the
        // candidate window opens below the field; Windows apps open it below
        // the caret, which a text tool on the canvas also needs.
        if let Some(ime) = platform_output.ime.as_mut() {
            ime.rect = ime.cursor_rect;
        }
        self.egui_state
            .handle_platform_output(&self.window, platform_output);
        if remove_device {
            tracing::warn!("removing the GPU device for a recovery test");
            if let Err(error) = self.display.gpu.remove_for_test() {
                tracing::error!(error, "cannot remove the GPU device");
            }
        }
        self.repaint_at = output
            .viewport_output
            .get(&egui::ViewportId::ROOT)
            .and_then(|viewport| Instant::now().checked_add(viewport.repaint_delay));
        self.needs_frame = false;

        let title = self.files.title(&self.canvas);
        if title != self.title {
            self.window.set_title(&title);
            self.title = title;
        }
        if let Some(next) = self.canvas.tick(Instant::now()) {
            self.repaint_at = Some(self.repaint_at.map_or(next, |at| at.min(next)));
        }
        self.canvas.sync();
        let upload = self.canvas.take_upload();
        let textures = output.textures_delta;
        let shown = Shown {
            shapes: output.shapes,
            pixels_per_point: output.pixels_per_point,
            canvas_area: canvas_area.map(|edge| edge.max(0) as u32),
            placement: self.canvas.placement(),
            canvas_size: {
                let display = self.canvas.display();
                [u32::from(display.width()), u32::from(display.height())]
            },
        };
        let same_picture = upload.is_none()
            && shown_ants.is_none()
            && textures.is_empty()
            && self.display.shown.as_ref() == Some(&shown);
        if same_picture {
            // Nothing it shows waits on a present any more.
            self.router.take_oldest_unpresented();
            tracing::debug!(
                ui_ms = (laid_out - started).as_secs_f64() * 1000.0,
                "frame skipped: same picture"
            );
            return;
        }
        let pixels_per_point = shown.pixels_per_point;
        let canvas_area = shown.canvas_area;
        let placement = shown.placement;
        let primitives = self
            .egui_ctx
            .tessellate(shown.shapes.clone(), pixels_per_point);
        // egui-wgpu panics instead of returning an error when a buffer cannot
        // be made on a lost device, and a device can be lost at any point in
        // the frame. Such a panic is caught only when the device is lost; the
        // whole display is then replaced, so nothing half-updated is reused.
        let drawn = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let Display {
                gpu,
                presenter,
                egui_renderer,
                canvas_view,
                ants,
                ..
            } = &mut self.display;
            for (id, deltas) in &textures.set {
                for delta in deltas {
                    egui_renderer.update_texture(&gpu.device, &gpu.queue, *id, delta);
                }
            }
            if let Some(rect) = upload {
                tracing::debug!(?rect, "canvas uploaded");
            }
            canvas_view.update(&gpu.device, &gpu.queue, self.canvas.display(), upload);
            ants.show(
                &gpu.device,
                &gpu.queue,
                shown_ants.as_ref().map(|(selection, _, _)| selection),
                shown_ants
                    .as_ref()
                    .map_or(ugu_core::ops::Affine::IDENTITY, |(_, moved, _)| *moved),
            );
            let presented = match draw(
                &gpu.device,
                &gpu.queue,
                presenter,
                canvas_view,
                ants,
                shown_ants.map_or(0.0, |(_, _, phase)| phase),
                canvas_area,
                placement,
                egui_renderer,
                &primitives,
                pixels_per_point,
            ) {
                Some(frame) => {
                    let oldest_input = self.router.take_oldest_unpresented();
                    presenter.present(&gpu.queue, frame, oldest_input.map(|ticks| ticks.0));
                    if let Some(oldest_input) = oldest_input {
                        self.present_latency
                            .record(Ticks::now().seconds_since(oldest_input));
                    }
                    true
                }
                // The input stays marked unpresented and is drawn next time.
                None => {
                    self.repaint_at = Some(Instant::now() + FRAME_WAIT_LIMIT);
                    false
                }
            };

            for id in &textures.free {
                egui_renderer.free_texture(id);
            }
            presented
        }));
        match drawn {
            Ok(true) => {
                self.display.shown = Some(shown);
                self.display.slot_held = false;
            }
            Ok(false) => {}
            Err(panic) => {
                if !self.display.gpu.check_removed() {
                    std::panic::resume_unwind(panic);
                }
                tracing::warn!("frame abandoned on the lost GPU device");
                self.needs_frame = true;
            }
        }
        tracing::debug!(
            ui_ms = (laid_out - started).as_secs_f64() * 1000.0,
            rest_ms = laid_out.elapsed().as_secs_f64() * 1000.0,
            "frame"
        );
    }

    fn collect_display_times(&mut self) {
        for displayed in self.display.presenter.take_display_times() {
            tracing::trace!(
                input_qpc = displayed.input_qpc,
                present_qpc = displayed.present_qpc,
                display_qpc = displayed.display_qpc,
                "frame displayed"
            );
            self.display_latency
                .record(Ticks(displayed.display_qpc).seconds_since(Ticks(displayed.input_qpc)));
        }
    }
}

/// Draws the document into `canvas_area` as `placement` places it, then
/// egui over it, into the next buffer of `presenter`, and submits it.
/// Returns `None` when there is no buffer to draw into now.
#[expect(clippy::too_many_arguments, reason = "one frame's parts, used once")]
fn draw(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    presenter: &mut Presenter,
    canvas: &mut DocumentView,
    ants: &mut Ants,
    ants_phase: f32,
    canvas_area: [u32; 4],
    placement: Placement,
    egui_renderer: &mut egui_wgpu::Renderer,
    primitives: &[egui::ClippedPrimitive],
    pixels_per_point: f32,
) -> Option<wgpu::SurfaceTexture> {
    let frame = match presenter.acquire(device) {
        Acquired::Frame(frame) => frame,
        Acquired::Skip => return None,
    };
    let size = presenter.size();
    let screen = egui_wgpu::ScreenDescriptor {
        size_in_pixels: size,
        pixels_per_point,
    };
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    let mut commands =
        egui_renderer.update_buffers(device, queue, &mut encoder, primitives, &screen);
    let view = frame
        .texture
        .create_view(&wgpu::TextureViewDescriptor::default());
    {
        let mut pass = encoder
            .begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("frame"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            })
            .forget_lifetime();
        canvas.draw(queue, &mut pass, canvas_area, placement, size);
        ants.draw(
            queue,
            &mut pass,
            canvas_area,
            placement,
            pixels_per_point,
            ants_phase,
            size,
        );
        pass.set_viewport(0.0, 0.0, size[0] as f32, size[1] as f32, 0.0, 1.0);
        egui_renderer.render(&mut pass, primitives, &screen);
    }
    commands.push(encoder.finish());
    queue.submit(commands);
    Some(frame)
}

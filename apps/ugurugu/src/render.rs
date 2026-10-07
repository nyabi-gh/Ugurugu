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

use ugu_core::document::Document;
use ugu_render::gpu::{AdapterChoice, Gpu};
use ugu_render::present::{Acquired, Presenter};
use ugu_render::view::{DocumentView, Placement};
use ugu_win::clock::Ticks;
use ugu_win::pointer::PointerEvent;
use winit::event::WindowEvent;
use winit::window::Window;

use crate::cache::Rendered;
use crate::canvas::{self, Canvas};
use crate::ime_probe::ImeProbe;
use crate::input::{CanvasInput, InputRouter};
use crate::latency::LatencyLog;

pub enum ToRender {
    Window(WindowEvent),
    Pointer(Vec<PointerEvent>),
    /// A screen reader connected (`true`) or left.
    Accessibility(bool),
    AccessibilityAction(egui::accesskit::ActionRequest),
    /// The cache worker finished a canvas split.
    Cache(Box<Rendered>),
    Shutdown,
}

/// How long to sleep when nothing is scheduled; any message wakes it earlier.
const IDLE_WAKE: Duration = Duration::from_secs(1);
/// Display times are read back this often while presents await them.
const DISPLAY_POLL: Duration = Duration::from_millis(2);
const FRAME_WAIT_LIMIT: Duration = Duration::from_millis(100);

/// Makes a new surface for the window. Only the UI thread may read the window
/// handle, so this asks it and waits for the answer.
pub type SurfaceSource = Box<dyn Fn() -> Result<wgpu::Surface<'static>, String> + Send>;

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

pub struct RenderThread {
    window: Arc<Window>,
    egui_ctx: egui::Context,
    egui_state: egui_winit::State,
    instance: wgpu::Instance,
    surface_source: SurfaceSource,
    tree_sink: TreeSink,
    adapter_choice: AdapterChoice,
    display: Display,
    /// `UGURUGU_DIAGNOSTICS=1` enables test-only keys.
    diagnostics: bool,
    router: InputRouter,
    canvas: Canvas,
    ime: ImeProbe,
    present_latency: LatencyLog,
    display_latency: LatencyLog,
    needs_frame: bool,
    repaint_at: Option<Instant>,
}

/// Everything made with one GPU device, replaced together when it is lost.
/// The canvas raster and egui's state live on the CPU and survive.
struct Display {
    gpu: Gpu,
    presenter: Presenter,
    egui_renderer: egui_wgpu::Renderer,
    canvas_view: DocumentView,
}

impl Display {
    fn open(
        instance: &wgpu::Instance,
        surface: wgpu::Surface<'static>,
        choice: AdapterChoice,
        size: [u32; 2],
    ) -> Result<Self, String> {
        let gpu = Gpu::open(instance, &surface, choice)?;
        let presenter = Presenter::new(surface, &gpu.adapter, &gpu.device, size)?;
        let egui_renderer = egui_wgpu::Renderer::new(
            &gpu.device,
            presenter.format(),
            egui_wgpu::RendererOptions::default(),
        );
        let canvas_view = DocumentView::new(&gpu.device, presenter.format(), canvas::WORKSPACE);
        Ok(Self {
            gpu,
            presenter,
            egui_renderer,
            canvas_view,
        })
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
    /// handle.
    pub fn create_surface(
        instance: &wgpu::Instance,
        window: &Arc<Window>,
    ) -> Result<wgpu::Surface<'static>, String> {
        instance
            .create_surface(window.clone())
            .map_err(|error| format!("cannot create the window surface: {error}"))
    }

    pub fn create(
        window: Arc<Window>,
        instance: wgpu::Instance,
        surface_source: SurfaceSource,
        tree_sink: TreeSink,
        egui_ctx: egui::Context,
        EguiState(egui_state): EguiState,
        to_self: Sender<ToRender>,
    ) -> Result<Self, String> {
        let adapter_choice = AdapterChoice::from_env();
        let size = window.inner_size();
        let surface = surface_source()?;
        let display = Display::open(
            &instance,
            surface,
            adapter_choice,
            [size.width, size.height],
        )?;
        Ok(Self {
            window,
            egui_ctx,
            egui_state,
            instance,
            surface_source,
            tree_sink,
            adapter_choice,
            display,
            diagnostics: std::env::var_os("UGURUGU_DIAGNOSTICS").is_some_and(|value| value == "1"),
            router: InputRouter::default(),
            canvas: Canvas::new(Document::new([1024, 768]), move |rendered| {
                let _ = to_self.send(ToRender::Cache(Box::new(rendered)));
            }),
            ime: ImeProbe::default(),
            present_latency: LatencyLog::default(),
            display_latency: LatencyLog::default(),
            needs_frame: true,
            repaint_at: None,
        })
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
        let surface = (self.surface_source)()?;
        let surfaced = started.elapsed();
        self.display = Display::open(
            &self.instance,
            surface,
            self.adapter_choice,
            [size.width, size.height],
        )?;
        self.canvas.upload_all();
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
            if self.repaint_at.is_some_and(|at| at <= Instant::now()) {
                self.needs_frame = true;
            }
            if self.needs_frame && self.display.presenter.wait_for_frame(FRAME_WAIT_LIMIT) {
                if !self.apply_queued(messages) {
                    break;
                }
                self.frame();
            }
            self.collect_display_times();
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
        if let Some(at) = self.repaint_at {
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
            ToRender::Shutdown => return false,
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
                let pan_held = space && !self.egui_ctx.egui_wants_keyboard_input();
                let canvas = &self.canvas;
                let routed =
                    self.router
                        .route(events, pixels_per_point, modifiers, pan_held, |position| {
                            canvas.contains(position)
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
                if let WindowEvent::Resized(size) = &event {
                    self.display
                        .presenter
                        .resize(&self.display.gpu.device, [size.width, size.height]);
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
        let input = self.egui_state.take_egui_input(&self.window);
        let mut canvas_area = [0; 4];
        let Self {
            display,
            canvas,
            display_latency,
            ime,
            diagnostics,
            ..
        } = self;
        let adapter_summary = &display.gpu.summary;
        let software = display.gpu.is_software();
        let mut remove_device = false;
        let output = self.egui_ctx.run_ui(input, |ui| {
            // Before the widgets run, so focus is what the key was pressed in.
            ime.count_canvas_shortcuts(ui.ctx());
            remove_device =
                *diagnostics && ui.ctx().input(|input| input.key_pressed(egui::Key::F9));
            egui::Panel::bottom("status").show(ui, |ui| {
                ui.horizontal(|ui| {
                    if software {
                        ui.colored_label(
                            ui.visuals().warn_fg_color,
                            "No usable GPU: drawing with the slow software display",
                        );
                        ui.separator();
                    }
                    ui.label(adapter_summary.as_str());
                    ui.separator();
                    ui.label(format!("{:.0}%", canvas.scale() * 100.0));
                    ui.separator();
                    ui.label(format!("samples {}", canvas.sample_count()));
                    if let Some(summary) = display_latency.summary() {
                        ui.separator();
                        ui.label(format!("input to display {summary}"));
                    }
                    if let Some(notice) = canvas.notice() {
                        ui.separator();
                        ui.colored_label(ui.visuals().warn_fg_color, notice);
                    }
                });
            });
            egui::Panel::right("ime").show(ui, |ui| ime.show(ui));
            // No panel fill: the canvas is drawn under egui.
            egui::CentralPanel::default()
                .frame(egui::Frame::NONE)
                .show(ui, |ui| canvas_area = canvas.layout(ui));
        });
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

        self.canvas.sync();
        let pixels_per_point = output.pixels_per_point;
        let primitives = self.egui_ctx.tessellate(output.shapes, pixels_per_point);
        let textures = output.textures_delta;
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
            } = &mut self.display;
            for (id, deltas) in &textures.set {
                for delta in deltas {
                    egui_renderer.update_texture(&gpu.device, &gpu.queue, *id, delta);
                }
            }
            let upload = self.canvas.take_upload();
            canvas_view.update(&gpu.device, &gpu.queue, self.canvas.display(), upload);
            let canvas_area = canvas_area.map(|edge| edge.max(0) as u32);
            let placement = self.canvas.placement();

            match draw(
                &gpu.device,
                &gpu.queue,
                presenter,
                canvas_view,
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
                }
                // The input stays marked unpresented and is drawn next time.
                None => self.repaint_at = Some(Instant::now() + FRAME_WAIT_LIMIT),
            }

            for id in &textures.free {
                egui_renderer.free_texture(id);
            }
        }));
        if let Err(panic) = drawn {
            if !self.display.gpu.check_removed() {
                std::panic::resume_unwind(panic);
            }
            tracing::warn!("frame abandoned on the lost GPU device");
            self.needs_frame = true;
        }
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
        pass.set_viewport(0.0, 0.0, size[0] as f32, size[1] as f32, 0.0, 1.0);
        egui_renderer.render(&mut pass, primitives, &screen);
    }
    commands.push(encoder.finish());
    queue.submit(commands);
    Some(frame)
}

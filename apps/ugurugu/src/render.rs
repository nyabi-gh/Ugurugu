// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! The render thread: owns the GPU, egui and the canvas, and paces frames.
//!
//! The UI thread only forwards window and pointer events here, so it never
//! waits on the GPU or the display. A frame waits for the swap chain first and
//! reads the input queued meanwhile afterwards, so it shows the newest input.

use std::sync::Arc;
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

use ugu_render::present::{Acquired, Presenter};
use ugu_render::view::CanvasView;
use ugu_win::clock::Ticks;
use ugu_win::pointer::PointerEvent;
use winit::event::WindowEvent;
use winit::window::Window;

use crate::canvas::{self, ProbeCanvas};
use crate::input::InputRouter;
use crate::latency::LatencyLog;

pub enum ToRender {
    Window(WindowEvent),
    Pointer(Vec<PointerEvent>),
    Shutdown,
}

/// How long to sleep when nothing is scheduled; any message wakes it earlier.
const IDLE_WAKE: Duration = Duration::from_secs(1);
/// Display times are read back this often while presents await them.
const DISPLAY_POLL: Duration = Duration::from_millis(2);
const FRAME_WAIT_LIMIT: Duration = Duration::from_millis(100);

pub struct RenderThread {
    window: Arc<Window>,
    egui_ctx: egui::Context,
    egui_state: egui_winit::State,
    device: wgpu::Device,
    queue: wgpu::Queue,
    presenter: Presenter,
    egui_renderer: egui_wgpu::Renderer,
    canvas_view: CanvasView,
    adapter_summary: String,
    router: InputRouter,
    canvas: ProbeCanvas,
    present_latency: LatencyLog,
    display_latency: LatencyLog,
    needs_frame: bool,
    repaint_at: Option<Instant>,
}

impl RenderThread {
    /// The surface is created on the UI thread, the only thread winit lets
    /// read the window handle.
    pub fn create_surface(
        window: &Arc<Window>,
    ) -> Result<(wgpu::Instance, wgpu::Surface<'static>), String> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::DX12,
            flags: wgpu::InstanceFlags::from_build_config().with_env(),
            memory_budget_thresholds: wgpu::MemoryBudgetThresholds::default(),
            backend_options: ugu_render::present::backend_options(),
            display: None,
        });
        let surface = instance
            .create_surface(window.clone())
            .map_err(|error| format!("cannot create the window surface: {error}"))?;
        Ok((instance, surface))
    }

    pub fn create(
        window: Arc<Window>,
        instance: &wgpu::Instance,
        surface: wgpu::Surface<'static>,
        egui_ctx: egui::Context,
        egui_state: egui_winit::State,
    ) -> Result<Self, String> {
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            force_fallback_adapter: false,
            compatible_surface: Some(&surface),
            apply_limit_buckets: false,
        }))
        .map_err(|error| format!("no DX12 adapter can show the window: {error}"))?;
        let adapter_summary = egui_wgpu::adapter_info_summary(&adapter.get_info());
        tracing::info!(adapter = %adapter_summary, "selected GPU adapter");
        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))
                .map_err(|error| format!("cannot open the DX12 device: {error}"))?;

        let size = window.inner_size();
        let presenter = Presenter::new(surface, &adapter, &device, [size.width, size.height])?;
        let egui_renderer = egui_wgpu::Renderer::new(
            &device,
            presenter.format(),
            egui_wgpu::RendererOptions::default(),
        );
        let canvas_view = CanvasView::new(&device, presenter.format(), canvas::PAPER);
        Ok(Self {
            window,
            egui_ctx,
            egui_state,
            device,
            queue,
            presenter,
            egui_renderer,
            canvas_view,
            adapter_summary,
            router: InputRouter::default(),
            canvas: ProbeCanvas::default(),
            present_latency: LatencyLog::default(),
            display_latency: LatencyLog::default(),
            needs_frame: true,
            repaint_at: None,
        })
    }

    pub fn run(mut self, messages: &Receiver<ToRender>) {
        loop {
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
            if self.needs_frame && self.presenter.wait_for_frame(FRAME_WAIT_LIMIT) {
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
    }

    fn wake_timeout(&self) -> Duration {
        let mut timeout = IDLE_WAKE;
        if let Some(at) = self.repaint_at {
            timeout = timeout.min(at.saturating_duration_since(Instant::now()));
        }
        if self.presenter.has_pending_display_times() {
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
            ToRender::Pointer(events) => {
                let pixels_per_point = egui_winit::pixels_per_point(&self.egui_ctx, &self.window);
                let modifiers = self.egui_ctx.input(|input| input.modifiers);
                let canvas = &self.canvas;
                let routed = self
                    .router
                    .route(events, pixels_per_point, modifiers, |position| {
                        canvas.contains(position)
                    });
                self.egui_state.egui_input_mut().events.extend(routed.egui);
                for input in routed.canvas {
                    self.canvas.apply(input);
                }
                self.needs_frame = true;
            }
            // egui-winit asks to repaint on every redraw; following that would
            // redraw forever, so a redraw request only schedules one frame.
            ToRender::Window(WindowEvent::RedrawRequested) => self.needs_frame = true,
            ToRender::Window(event) => {
                if let WindowEvent::Resized(size) = &event {
                    self.presenter
                        .resize(&self.device, [size.width, size.height]);
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
            adapter_summary,
            canvas,
            display_latency,
            ..
        } = self;
        let output = self.egui_ctx.run_ui(input, |ui| {
            egui::Panel::bottom("status").show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(adapter_summary.as_str());
                    ui.separator();
                    ui.label(format!("samples {}", canvas.sample_count()));
                    if let Some(summary) = display_latency.summary() {
                        ui.separator();
                        ui.label(format!("input to display {summary}"));
                    }
                    if ui.button("Clear").clicked() {
                        canvas.clear();
                    }
                });
            });
            // No panel fill: the canvas is drawn under egui.
            egui::CentralPanel::default()
                .frame(egui::Frame::NONE)
                .show(ui, |ui| canvas_area = canvas.layout(ui));
        });
        self.egui_state
            .handle_platform_output(&self.window, output.platform_output);
        self.repaint_at = output
            .viewport_output
            .get(&egui::ViewportId::ROOT)
            .and_then(|viewport| Instant::now().checked_add(viewport.repaint_delay));
        self.needs_frame = false;

        let pixels_per_point = output.pixels_per_point;
        let primitives = self.egui_ctx.tessellate(output.shapes, pixels_per_point);
        let textures = output.textures_delta;
        for (id, deltas) in &textures.set {
            for delta in deltas {
                self.egui_renderer
                    .update_texture(&self.device, &self.queue, *id, delta);
            }
        }
        self.canvas_view
            .update(&self.device, &self.queue, self.canvas.raster_mut());
        let canvas_origin = [canvas_area[0].max(0) as u32, canvas_area[1].max(0) as u32];

        match draw(
            &self.device,
            &self.queue,
            &mut self.presenter,
            &mut self.canvas_view,
            canvas_origin,
            &mut self.egui_renderer,
            &primitives,
            pixels_per_point,
        ) {
            Some(frame) => {
                let oldest_input = self.router.take_oldest_unpresented();
                self.presenter
                    .present(&self.queue, frame, oldest_input.map(|ticks| ticks.0));
                if let Some(oldest_input) = oldest_input {
                    self.present_latency
                        .record(Ticks::now().seconds_since(oldest_input));
                }
            }
            // The input stays marked unpresented and is drawn next time.
            None => self.repaint_at = Some(Instant::now() + FRAME_WAIT_LIMIT),
        }

        for id in &textures.free {
            self.egui_renderer.free_texture(id);
        }
    }

    fn collect_display_times(&mut self) {
        for displayed in self.presenter.take_display_times() {
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

/// Draws the canvas with its top-left pixel at `canvas_origin`, then egui
/// over it, into the next buffer of `presenter`, and submits it. Returns
/// `None` when there is no buffer to draw into now.
#[expect(clippy::too_many_arguments, reason = "one frame's parts, used once")]
fn draw(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    presenter: &mut Presenter,
    canvas: &mut CanvasView,
    canvas_origin: [u32; 2],
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
        canvas.draw(queue, &mut pass, canvas_origin, size);
        pass.set_viewport(0.0, 0.0, size[0] as f32, size[1] as f32, 0.0, 1.0);
        egui_renderer.render(&mut pass, primitives, &screen);
    }
    commands.push(encoder.finish());
    queue.submit(commands);
    Some(frame)
}

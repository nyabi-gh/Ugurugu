// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! The render thread: owns the GPU, egui and the canvas, and paces frames.
//!
//! The UI thread only forwards window and pointer events here, so it never
//! waits on the GPU or the display. A frame waits for the swap chain first and
//! reads the input queued meanwhile afterwards, so it shows the newest input.

use std::num::NonZeroIsize;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

use ugu_render::present::{self, Acquired, Presenter};
use ugu_win::clock::Ticks;
use ugu_win::composition;
use ugu_win::pointer::PointerEvent;
use winit::event::WindowEvent;
use winit::raw_window_handle::{
    RawDisplayHandle, RawWindowHandle, Win32WindowHandle, WindowsDisplayHandle,
};
use winit::window::Window;

use crate::canvas::ProbeCanvas;
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

/// The surface of the canvas child window, made on the UI thread.
pub struct CanvasSurface {
    hwnd: isize,
    surface: wgpu::Surface<'static>,
}

impl CanvasSurface {
    pub fn create(instance: &wgpu::Instance, hwnd: isize) -> Result<Self, String> {
        let handle = Win32WindowHandle::new(
            NonZeroIsize::new(hwnd).ok_or("the canvas window has no handle")?,
        );
        // SAFETY: the UI thread destroys the child window only after the
        // render thread, which owns this surface, has ended.
        let surface = unsafe {
            instance.create_surface_unsafe(wgpu::SurfaceTargetUnsafe::RawHandle {
                raw_display_handle: Some(RawDisplayHandle::Windows(WindowsDisplayHandle::new())),
                raw_window_handle: RawWindowHandle::Win32(handle),
            })
        }
        .map_err(|error| format!("cannot create the canvas surface: {error}"))?;
        Ok(Self { hwnd, surface })
    }
}

/// The canvas swap chain in its own child window.
struct ChildCanvas {
    hwnd: isize,
    presenter: Presenter,
    renderer: egui_wgpu::Renderer,
    /// Where the window was last placed, in parent client physical pixels.
    placed: Option<[i32; 4]>,
}

pub struct RenderThread {
    window: Arc<Window>,
    egui_ctx: egui::Context,
    egui_state: egui_winit::State,
    device: wgpu::Device,
    queue: wgpu::Queue,
    presenter: Presenter,
    egui_renderer: egui_wgpu::Renderer,
    child: Option<ChildCanvas>,
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
        canvas_surface: Option<CanvasSurface>,
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
        let child = match canvas_surface {
            Some(canvas) => {
                let presenter = Presenter::new(canvas.surface, &adapter, &device, [1, 1])?;
                let renderer = egui_wgpu::Renderer::new(
                    &device,
                    presenter.format(),
                    egui_wgpu::RendererOptions::default(),
                );
                Some(ChildCanvas {
                    hwnd: canvas.hwnd,
                    presenter,
                    renderer,
                    placed: None,
                })
            }
            None => None,
        };
        Ok(Self {
            window,
            egui_ctx,
            egui_state,
            device,
            queue,
            presenter,
            egui_renderer,
            child,
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
            if self.needs_frame && self.wait_for_frame() {
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

    fn wait_for_frame(&self) -> bool {
        match &self.child {
            Some(child) => {
                present::wait_for_frames(&[&self.presenter, &child.presenter], FRAME_WAIT_LIMIT)
            }
            None => present::wait_for_frames(&[&self.presenter], FRAME_WAIT_LIMIT),
        }
    }

    /// The presenter that shows the canvas, and so the input.
    fn canvas_presenter(&mut self) -> &mut Presenter {
        match &mut self.child {
            Some(child) => &mut child.presenter,
            None => &mut self.presenter,
        }
    }

    fn wake_timeout(&mut self) -> Duration {
        let mut timeout = IDLE_WAKE;
        if let Some(at) = self.repaint_at {
            timeout = timeout.min(at.saturating_duration_since(Instant::now()));
        }
        if self.canvas_presenter().has_pending_display_times() {
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
        let in_child = self.child.is_some();
        let mut child_canvas = None;
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
            egui::CentralPanel::default().show(ui, |ui| {
                let (rect, shapes) = canvas.layout(ui);
                if in_child {
                    child_canvas = Some((rect, shapes));
                } else {
                    ui.painter_at(rect)
                        .extend(shapes.into_iter().map(|mut shape| {
                            shape.translate(rect.min.to_vec2());
                            shape
                        }));
                }
            });
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
        let renderers = std::iter::once(&mut self.egui_renderer)
            .chain(self.child.as_mut().map(|child| &mut child.renderer));
        for renderer in renderers {
            for (id, deltas) in &textures.set {
                for delta in deltas {
                    renderer.update_texture(&self.device, &self.queue, *id, delta);
                }
            }
        }

        // The canvas goes first, so its present is the first one after input.
        let mut canvas_shown = true;
        if let (Some(child), Some((rect, shapes))) = (self.child.as_mut(), child_canvas) {
            let primitives =
                child.layout(&self.device, &self.egui_ctx, rect, shapes, pixels_per_point);
            match draw(
                &self.device,
                &self.queue,
                &mut child.presenter,
                &mut child.renderer,
                &primitives,
                pixels_per_point,
            ) {
                Some(frame) => present_input(
                    &self.queue,
                    &mut child.presenter,
                    frame,
                    &mut self.router,
                    &mut self.present_latency,
                ),
                None => canvas_shown = false,
            }
        }
        match draw(
            &self.device,
            &self.queue,
            &mut self.presenter,
            &mut self.egui_renderer,
            &primitives,
            pixels_per_point,
        ) {
            Some(frame) if in_child => self.presenter.present(&self.queue, frame, None),
            Some(frame) => present_input(
                &self.queue,
                &mut self.presenter,
                frame,
                &mut self.router,
                &mut self.present_latency,
            ),
            None => canvas_shown &= in_child,
        }
        if !canvas_shown {
            // The input stays marked unpresented and is drawn next time.
            self.repaint_at = Some(Instant::now() + FRAME_WAIT_LIMIT);
        }

        let renderers = std::iter::once(&mut self.egui_renderer)
            .chain(self.child.as_mut().map(|child| &mut child.renderer));
        for renderer in renderers {
            for id in &textures.free {
                renderer.free_texture(id);
            }
        }
    }

    fn collect_display_times(&mut self) {
        for displayed in self.canvas_presenter().take_display_times() {
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

impl ChildCanvas {
    /// Places the window over the canvas area and returns the canvas drawn
    /// relative to the window. `shapes` are relative to `rect`, in points.
    fn layout(
        &mut self,
        device: &wgpu::Device,
        egui_ctx: &egui::Context,
        rect: egui::Rect,
        shapes: Vec<egui::Shape>,
        pixels_per_point: f32,
    ) -> Vec<egui::ClippedPrimitive> {
        let edge = |value: f32| (value * pixels_per_point).round() as i32;
        let placed = [
            edge(rect.left()),
            edge(rect.top()),
            edge(rect.right()),
            edge(rect.bottom()),
        ];
        let size = [
            (placed[2] - placed[0]).max(1) as u32,
            (placed[3] - placed[1]).max(1) as u32,
        ];
        if self.placed != Some(placed) {
            composition::place_child(self.hwnd, placed);
            self.presenter.resize(device, size);
            self.placed = Some(placed);
        }
        // The window snaps to whole pixels; keep the canvas where egui put it.
        let offset = rect.min - egui::pos2(placed[0] as f32, placed[1] as f32) / pixels_per_point;
        let clip_rect = egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(size[0] as f32, size[1] as f32) / pixels_per_point,
        );
        let clipped = shapes
            .into_iter()
            .map(|mut shape| {
                shape.translate(offset);
                egui::epaint::ClippedShape { clip_rect, shape }
            })
            .collect();
        egui_ctx.tessellate(clipped, pixels_per_point)
    }
}

/// Renders `primitives` into the next buffer of `presenter` and submits it.
/// Returns `None` when there is no buffer to draw into now.
fn draw(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    presenter: &mut Presenter,
    renderer: &mut egui_wgpu::Renderer,
    primitives: &[egui::ClippedPrimitive],
    pixels_per_point: f32,
) -> Option<wgpu::SurfaceTexture> {
    let frame = match presenter.acquire(device) {
        Acquired::Frame(frame) => frame,
        Acquired::Skip => return None,
    };
    let screen = egui_wgpu::ScreenDescriptor {
        size_in_pixels: presenter.size(),
        pixels_per_point,
    };
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    let mut commands = renderer.update_buffers(device, queue, &mut encoder, primitives, &screen);
    let view = frame
        .texture
        .create_view(&wgpu::TextureViewDescriptor::default());
    {
        let mut pass = encoder
            .begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("egui"),
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
        renderer.render(&mut pass, primitives, &screen);
    }
    commands.push(encoder.finish());
    queue.submit(commands);
    Some(frame)
}

/// Presents the frame that shows the canvas, stamped with the oldest input
/// it shows.
fn present_input(
    queue: &wgpu::Queue,
    presenter: &mut Presenter,
    frame: wgpu::SurfaceTexture,
    router: &mut InputRouter,
    present_latency: &mut LatencyLog,
) {
    let oldest_input = router.take_oldest_unpresented();
    presenter.present(queue, frame, oldest_input.map(|ticks| ticks.0));
    if let Some(oldest_input) = oldest_input {
        present_latency.record(Ticks::now().seconds_since(oldest_input));
    }
}

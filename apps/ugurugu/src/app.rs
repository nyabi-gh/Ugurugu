// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

use std::num::NonZeroU32;
use std::sync::Arc;
use std::time::Instant;

use egui_wgpu::winit::Painter;
use egui_wgpu::{RendererOptions, WgpuConfiguration, WgpuSetup, WgpuSetupCreateNew};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow};
use winit::window::{Window, WindowId};

#[derive(Default)]
pub struct App {
    session: Option<Session>,
    fatal_error: Option<String>,
}

struct Session {
    window: Arc<Window>,
    egui_ctx: egui::Context,
    egui_state: egui_winit::State,
    painter: Painter,
    adapter_summary: String,
    repaint_at: Option<Instant>,
}

impl App {
    pub fn fatal_error(&self) -> Option<&str> {
        self.fatal_error.as_deref()
    }

    fn fail(&mut self, event_loop: &ActiveEventLoop, error: String) {
        self.fatal_error = Some(error);
        self.session = None;
        event_loop.exit();
    }
}

fn wgpu_configuration() -> WgpuConfiguration {
    let mut setup = WgpuSetupCreateNew::without_display_handle();
    // WGPU_BACKEND is ignored on purpose: DX12 is the only compiled backend.
    setup.instance_descriptor.backends = wgpu::Backends::DX12;
    setup.power_preference = wgpu::PowerPreference::HighPerformance;
    WgpuConfiguration {
        wgpu_setup: WgpuSetup::CreateNew(setup),
        ..Default::default()
    }
}

impl Session {
    fn create(event_loop: &ActiveEventLoop) -> Result<Self, String> {
        let attributes = Window::default_attributes()
            .with_title("Ugurugu")
            .with_inner_size(winit::dpi::LogicalSize::new(1600.0, 1000.0));
        let window = Arc::new(
            event_loop
                .create_window(attributes)
                .map_err(|error| format!("cannot create the window: {error}"))?,
        );

        let egui_ctx = egui::Context::default();
        let mut painter = pollster::block_on(Painter::new(
            egui_ctx.clone(),
            wgpu_configuration(),
            false,
            RendererOptions::default(),
        ));
        pollster::block_on(painter.set_window(egui::ViewportId::ROOT, Some(window.clone())))
            .map_err(|error| format!("cannot initialise DX12 rendering: {error}"))?;
        let render_state = painter
            .render_state()
            .ok_or_else(|| "DX12 rendering has no render state".to_owned())?;
        let adapter_summary = egui_wgpu::adapter_info_summary(&render_state.adapter.get_info());
        tracing::info!(adapter = %adapter_summary, "selected GPU adapter");

        let egui_state = egui_winit::State::new(
            egui_ctx.clone(),
            egui::ViewportId::ROOT,
            event_loop,
            Some(window.scale_factor() as f32),
            window.theme(),
            painter.max_texture_side(),
        );

        Ok(Self {
            window,
            egui_ctx,
            egui_state,
            painter,
            adapter_summary,
            repaint_at: Some(Instant::now()),
        })
    }

    fn redraw(&mut self) {
        let input = self.egui_state.take_egui_input(&self.window);
        let adapter_summary = &self.adapter_summary;
        let output = self.egui_ctx.run_ui(input, |ui| {
            egui::Panel::bottom("status").show(ui, |ui| {
                ui.label(adapter_summary);
            });
        });

        self.egui_state
            .handle_platform_output(&self.window, output.platform_output);
        let primitives = self
            .egui_ctx
            .tessellate(output.shapes, output.pixels_per_point);
        let mut textures_delta = output.textures_delta;
        self.painter.paint_and_update_textures(
            egui::ViewportId::ROOT,
            output.pixels_per_point,
            [0.0, 0.0, 0.0, 1.0],
            &primitives,
            &mut textures_delta,
            Vec::new(),
            &self.window,
        );

        self.repaint_at = output
            .viewport_output
            .get(&egui::ViewportId::ROOT)
            .and_then(|viewport| Instant::now().checked_add(viewport.repaint_delay));
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.session.is_some() {
            return;
        }
        match Session::create(event_loop) {
            Ok(session) => self.session = Some(session),
            Err(error) => self.fail(event_loop, error),
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        let Some(session) = self.session.as_mut() else {
            return;
        };
        let response = session.egui_state.on_window_event(&session.window, &event);
        if response.repaint {
            session.window.request_redraw();
        }

        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let (Some(width), Some(height)) =
                    (NonZeroU32::new(size.width), NonZeroU32::new(size.height))
                {
                    session
                        .painter
                        .on_window_resized(egui::ViewportId::ROOT, width, height);
                }
            }
            WindowEvent::RedrawRequested => session.redraw(),
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let Some(session) = self.session.as_ref() else {
            return;
        };
        match session.repaint_at {
            Some(at) if at <= Instant::now() => {
                session.window.request_redraw();
                event_loop.set_control_flow(ControlFlow::Wait);
            }
            Some(at) => event_loop.set_control_flow(ControlFlow::WaitUntil(at)),
            None => event_loop.set_control_flow(ControlFlow::Wait),
        }
    }
}

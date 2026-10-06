// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! The benchmark operations on Vello GPU over wgpu DX12.
//!
//! Every operation waits for the GPU to finish, so its time is when the
//! result is ready, as for the CPU renderers. Layers stay in GPU textures.

use vello_cpu::kurbo::{Affine, BezPath, Rect};
use vello_cpu::peniko::{BlendMode, Compose, ImageQuality, ImageSampler, Mix};
use vello_gpu::{ClearSettings, RectU16, RenderSize, TargetInit, TextureBindings, TextureId};

use crate::backend::{Backend, Painter, draw, vello_blend, vello_color};
use crate::scene::{Blend, CLIP_BASE, CLIPPED, Draw, LAYER_BLENDS};

impl Painter for vello_gpu::Scene {
    fn set_paint(&mut self, paint: vello_cpu::PaintType) {
        vello_gpu::Scene::set_paint(self, paint);
    }
    fn set_stroke(&mut self, stroke: vello_cpu::kurbo::Stroke) {
        vello_gpu::Scene::set_stroke(self, stroke);
    }
    fn fill_path(&mut self, path: &BezPath) {
        vello_gpu::Scene::fill_path(self, path);
    }
    fn stroke_path(&mut self, path: &BezPath) {
        vello_gpu::Scene::stroke_path(self, path);
    }
}

const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

pub struct GpuImage {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
}

pub struct VelloGpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    renderer: vello_gpu::Renderer,
    resources: vello_gpu::Resources,
    scene: vello_gpu::Scene,
    depth: wgpu::TextureView,
    size: u16,
}

impl VelloGpu {
    /// `level` is the SIMD level of the CPU-side preparation.
    pub fn new(size: u32, level: vello_cpu::Level) -> Result<Self, String> {
        let size = u16::try_from(size).map_err(|_| "Vello GPU images are at most 65535 wide")?;
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::DX12,
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            ..wgpu::RequestAdapterOptions::default()
        }))
        .map_err(|error| format!("no DX12 adapter: {error}"))?;
        println!("adapter {}", adapter.get_info().name);
        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))
                .map_err(|error| format!("cannot open the device: {error}"))?;
        let (renderer, resources) = vello_gpu::Renderer::new(
            &device,
            &vello_gpu::RenderTargetConfig {
                format: FORMAT,
                width: size,
                height: size,
            },
        );
        let render_size = RenderSize {
            width: size,
            height: size,
        };
        let depth = vello_gpu::Renderer::create_depth_texture_view(&device, &render_size);
        Ok(Self {
            device,
            queue,
            renderer,
            resources,
            scene: vello_gpu::Scene::new_with(size, size, level),
            depth,
            size,
        })
    }

    /// Renders the scene into `target`, waits for the GPU and clears the scene.
    fn render(&mut self, target: &GpuImage, bindings: &TextureBindings, init: TargetInit<'_>) {
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        self.renderer
            .render(
                &self.scene,
                &mut self.resources,
                &self.device,
                &self.queue,
                &mut encoder,
                &RenderSize {
                    width: self.size,
                    height: self.size,
                },
                &target.view,
                Some(&self.depth),
                bindings,
                init,
            )
            .expect("Vello GPU renders the scene");
        self.queue.submit([encoder.finish()]);
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("the GPU finishes");
        self.scene.reset();
    }

    fn fill_image(&mut self, id: u64, quality: ImageQuality) {
        self.scene.set_paint(vello_cpu::Image {
            image: vello_cpu::ImageSource::external_texture(
                TextureId(id),
                RectU16::new(0, 0, self.size, self.size),
                true,
            ),
            sampler: ImageSampler::default().with_quality(quality),
        });
        let size = f64::from(self.size);
        self.scene.fill_rect(&Rect::new(0.0, 0.0, size, size));
    }
}

impl Backend for VelloGpu {
    type Image = GpuImage;

    fn new_image(&self, size: u32) -> Self::Image {
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: size,
                height: size,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        GpuImage { texture, view }
    }

    fn rasterize(&mut self, draws: &[Draw], target: &mut Self::Image) {
        for item in draws {
            draw(&mut self.scene, item);
        }
        self.render(
            target,
            &TextureBindings::new(),
            TargetInit::Clear(ClearSettings::default()),
        );
    }

    fn composite(&mut self, layers: &mut [Self::Image], out: &mut Self::Image) {
        let mut bindings = TextureBindings::new();
        for (index, layer) in layers.iter().enumerate() {
            bindings.insert(TextureId(index as u64), layer.view.clone());
        }
        let size = f64::from(self.size);
        self.scene.set_paint(vello_color([255, 255, 255, 255]));
        self.scene.fill_rect(&Rect::new(0.0, 0.0, size, size));
        for (index, blend) in LAYER_BLENDS.into_iter().enumerate().take(CLIP_BASE) {
            if blend == Blend::Normal {
                self.fill_image(index as u64, ImageQuality::Low);
            } else {
                self.scene.push_blend_layer(vello_blend(blend));
                self.fill_image(index as u64, ImageQuality::Low);
                self.scene.pop_layer();
            }
        }
        self.scene
            .push_blend_layer(vello_blend(LAYER_BLENDS[CLIP_BASE]));
        self.fill_image(CLIP_BASE as u64, ImageQuality::Low);
        self.scene
            .push_blend_layer(BlendMode::new(Mix::Normal, Compose::SrcAtop));
        self.fill_image(CLIPPED as u64, ImageQuality::Low);
        self.scene.pop_layer();
        self.scene.pop_layer();
        self.render(out, &bindings, TargetInit::Clear(ClearSettings::default()));
    }

    fn transform(&mut self, source: &mut Self::Image, affine: [f32; 6], out: &mut Self::Image) {
        let mut bindings = TextureBindings::new();
        bindings.insert(TextureId(0), source.view.clone());
        self.scene.set_transform(Affine::new(affine.map(f64::from)));
        self.fill_image(0, ImageQuality::Medium);
        self.scene.reset_transform();
        self.render(out, &bindings, TargetInit::Clear(ClearSettings::default()));
    }

    fn draw_segment(&mut self, segment: &Draw, _dirty: [u32; 4], target: &mut Self::Image) {
        draw(&mut self.scene, segment);
        self.render(target, &TextureBindings::new(), TargetInit::SrcOver);
    }

    fn bytes(&self, image: &Self::Image) -> Vec<u8> {
        let [width, height] = [image.texture.width(), image.texture.height()];
        let row = (width * 4).next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: u64::from(row * height),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        encoder.copy_texture_to_buffer(
            image.texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(row),
                    rows_per_image: None,
                },
            },
            image.texture.size(),
        );
        self.queue.submit([encoder.finish()]);
        buffer.slice(..).map_async(wgpu::MapMode::Read, |_| {});
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("the GPU finishes");
        let mapped = buffer.slice(..).get_mapped_range().expect("mapped");
        let mut pixels = Vec::with_capacity((width * height * 4) as usize);
        for y in 0..height {
            let start = (y * row) as usize;
            pixels.extend_from_slice(&mapped[start..start + (width * 4) as usize]);
        }
        pixels
    }
}

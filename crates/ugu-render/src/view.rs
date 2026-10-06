// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! Shows the CPU canvas on the GPU: changed pixels are uploaded as they are,
//! and each screen pixel reads exactly one canvas pixel over the paper.

use crate::raster::{CanvasRaster, PixelRect};

const SHADER: &str = r"
struct View {
    origin: vec2<f32>,
    paper: vec4<f32>,
}

@group(0) @binding(0) var<uniform> view: View;
@group(0) @binding(1) var canvas: texture_2d<f32>;

@vertex
fn vertex(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    // One triangle over the viewport.
    let corner = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    return vec4<f32>(corner * vec2<f32>(2.0, -2.0) + vec2<f32>(-1.0, 1.0), 0.0, 1.0);
}

@fragment
fn fragment(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let ink = textureLoad(canvas, vec2<i32>(position.xy - view.origin), 0);
    return ink + view.paper * (1.0 - ink.a);
}
";

/// `View` in the shader: origin, padding to 16 bytes, paper.
fn uniform_bytes(origin: [f32; 2], paper: [f32; 4]) -> [u8; 32] {
    let values = [
        origin[0], origin[1], 0.0, 0.0, paper[0], paper[1], paper[2], paper[3],
    ];
    let mut bytes = [0; 32];
    for (chunk, value) in bytes.as_chunks_mut::<4>().0.iter_mut().zip(values) {
        chunk.copy_from_slice(&value.to_ne_bytes());
    }
    bytes
}

pub struct CanvasView {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    uniform: wgpu::Buffer,
    texture: wgpu::Texture,
    bind_group: wgpu::BindGroup,
    paper: [f32; 4],
    origin: Option<[u32; 2]>,
}

impl CanvasView {
    /// `format` is the target's format; `paper` is opaque straight RGBA.
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat, paper: [u8; 4]) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("canvas view"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("canvas view"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("canvas view"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("canvas view"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vertex"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fragment"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("canvas view"),
            size: 32,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let texture = Self::create_texture(device, [1, 1]);
        let bind_group = Self::create_bind_group(device, &layout, &uniform, &texture);
        Self {
            pipeline,
            layout,
            uniform,
            texture,
            bind_group,
            paper: paper.map(|value| f32::from(value) / 255.0),
            origin: None,
        }
    }

    fn create_texture(device: &wgpu::Device, [width, height]: [u32; 2]) -> wgpu::Texture {
        device.create_texture(&wgpu::TextureDescriptor {
            label: Some("canvas"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            // Not sRGB: the canvas holds encoded values, as the swap chain does.
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        })
    }

    fn create_bind_group(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        uniform: &wgpu::Buffer,
        texture: &wgpu::Texture,
    ) -> wgpu::BindGroup {
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("canvas view"),
            layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
            ],
        })
    }

    /// Uploads what changed in `raster` since the last call; a new size
    /// replaces the texture and uploads everything.
    pub fn update(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        raster: &mut CanvasRaster,
    ) {
        let [width, height] = raster.size();
        let mut dirty = raster.take_dirty();
        if self.texture.width() != width || self.texture.height() != height {
            self.texture = Self::create_texture(device, [width, height]);
            self.bind_group =
                Self::create_bind_group(device, &self.layout, &self.uniform, &self.texture);
            dirty = Some([0, 0, width, height]);
        }
        let Some([left, top, right, bottom]): Option<PixelRect> = dirty else {
            return;
        };
        let row = width * 4;
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.texture,
                mip_level: 0,
                origin: wgpu::Origin3d {
                    x: left,
                    y: top,
                    z: 0,
                },
                aspect: wgpu::TextureAspect::All,
            },
            raster.pixels(),
            wgpu::TexelCopyBufferLayout {
                offset: u64::from(top * row + left * 4),
                bytes_per_row: Some(row),
                rows_per_image: None,
            },
            wgpu::Extent3d {
                width: right - left,
                height: bottom - top,
                depth_or_array_layers: 1,
            },
        );
    }

    /// Draws the canvas with its top-left pixel at `origin` of a target of
    /// `target_size`, clipped to the target.
    pub fn draw(
        &mut self,
        queue: &wgpu::Queue,
        pass: &mut wgpu::RenderPass<'_>,
        origin: [u32; 2],
        target_size: [u32; 2],
    ) {
        let width = self
            .texture
            .width()
            .min(target_size[0].saturating_sub(origin[0]));
        let height = self
            .texture
            .height()
            .min(target_size[1].saturating_sub(origin[1]));
        if width == 0 || height == 0 {
            return;
        }
        if self.origin != Some(origin) {
            queue.write_buffer(
                &self.uniform,
                0,
                &uniform_bytes(origin.map(|value| value as f32), self.paper),
            );
            self.origin = Some(origin);
        }
        pass.set_viewport(
            origin[0] as f32,
            origin[1] as f32,
            width as f32,
            height as f32,
            0.0,
            1.0,
        );
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.draw(0..3, 0..1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::raster::StrokeStyle;

    const PAPER: [u8; 4] = [245, 240, 230, 255];

    fn device() -> Option<(wgpu::Device, wgpu::Queue)> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::DX12,
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });
        let adapter =
            pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
                .ok()?;
        pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).ok()
    }

    /// Draws `raster` at `origin` of a `target`-sized image and reads it back.
    fn render(raster: &mut CanvasRaster, origin: [u32; 2], target: [u32; 2]) -> Option<Vec<u8>> {
        let (device, queue) = device()?;
        let format = wgpu::TextureFormat::Rgba8Unorm;
        let mut view = CanvasView::new(&device, format, PAPER);
        view.update(&device, &queue, raster);
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: target[0],
                height: target[1],
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let row = (target[0] * 4).next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: u64::from(row * target[1]),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        {
            let target_view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &target_view,
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
            });
            view.draw(&queue, &mut pass, origin, target);
        }
        encoder.copy_texture_to_buffer(
            texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(row),
                    rows_per_image: None,
                },
            },
            texture.size(),
        );
        queue.submit([encoder.finish()]);
        buffer.slice(..).map_async(wgpu::MapMode::Read, |_| {});
        device.poll(wgpu::PollType::wait_indefinitely()).ok()?;
        let mapped = buffer.slice(..).get_mapped_range().ok()?;
        let mut pixels = Vec::new();
        for y in 0..target[1] {
            let start = (y * row) as usize;
            pixels.extend_from_slice(&mapped[start..start + (target[0] * 4) as usize]);
        }
        Some(pixels)
    }

    #[test]
    fn each_screen_pixel_shows_one_canvas_pixel_over_the_paper() {
        let mut raster = CanvasRaster::new([20, 12]);
        let style = StrokeStyle {
            width: 3.0,
            color: [200, 30, 10, 160],
        };
        raster.dot([4.0, 3.0], style);
        raster.segment([4.0, 3.0], [17.0, 9.0], style);
        let (origin, target) = ([5, 3], [32, 24]);
        let Some(shown) = render(&mut raster, origin, target) else {
            eprintln!("skipped: no DX12 adapter");
            return;
        };
        let canvas = raster.pixels();
        for y in 0..target[1] {
            for x in 0..target[0] {
                let at = ((y * target[0] + x) * 4) as usize;
                let inside = (origin[0]..origin[0] + 20).contains(&x)
                    && (origin[1]..origin[1] + 12).contains(&y);
                let expected: [u8; 4] = if inside {
                    let source = (((y - origin[1]) * 20 + x - origin[0]) * 4) as usize;
                    let ink = &canvas[source..source + 4];
                    let cover = 255 - u32::from(ink[3]);
                    std::array::from_fn(|channel| {
                        (u32::from(ink[channel]) + (u32::from(PAPER[channel]) * cover + 127) / 255)
                            .min(255) as u8
                    })
                } else {
                    [0, 0, 0, 255]
                };
                for channel in 0..4 {
                    assert!(
                        shown[at + channel].abs_diff(expected[channel]) <= 1,
                        "pixel {x},{y} shows {:?}, expected {expected:?}",
                        &shown[at..at + 4]
                    );
                }
            }
        }
    }
}

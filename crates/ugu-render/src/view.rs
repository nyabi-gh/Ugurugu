// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! Shows the document on the GPU: changed document pixels are uploaded as
//! they are, and the shader maps each screen pixel to a document pixel by
//! the view's scale and offset. At 100% and above a screen pixel shows one
//! document pixel exactly; below, neighbouring pixels are blended.

use vello_cpu::Pixmap;

use crate::raster::PixelRect;

const SHADER: &str = r"
struct View {
    // Screen pixels of the document's top-left corner, and screen pixels
    // per document pixel.
    offset: vec2<f32>,
    scale: f32,
    blended: f32,
    size: vec2<f32>,
    _pad: vec2<f32>,
    workspace: vec4<f32>,
}

@group(0) @binding(0) var<uniform> view: View;
@group(0) @binding(1) var document: texture_2d<f32>;
@group(0) @binding(2) var blend: sampler;

@vertex
fn vertex(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    // One triangle over the viewport.
    let corner = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    return vec4<f32>(corner * vec2<f32>(2.0, -2.0) + vec2<f32>(-1.0, 1.0), 0.0, 1.0);
}

@fragment
fn fragment(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let at = (position.xy - view.offset) / view.scale;
    if (any(at < vec2<f32>(0.0)) || any(at >= view.size)) {
        return view.workspace;
    }
    var ink: vec4<f32>;
    if (view.blended > 0.5) {
        ink = textureSampleLevel(document, blend, at / view.size, 0.0);
    } else {
        ink = textureLoad(document, vec2<i32>(floor(at)), 0);
    }
    // A transparent background shows a checkerboard of 8 screen pixels.
    let cell = vec2<i32>(floor(position.xy / 8.0));
    let light = select(0.8, 1.0, ((cell.x + cell.y) & 1) == 0);
    return ink + vec4<f32>(light, light, light, 1.0) * (1.0 - ink.a);
}
";

/// Where the document sits on screen.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placement {
    /// Target pixels of the document's top-left corner.
    pub offset: [f32; 2],
    /// Target pixels per document pixel.
    pub scale: f32,
}

fn uniform_bytes(placement: Placement, size: [u32; 2], workspace: [f32; 4]) -> [u8; 48] {
    let values = [
        placement.offset[0],
        placement.offset[1],
        placement.scale,
        if placement.scale < 1.0 { 1.0 } else { 0.0 },
        size[0] as f32,
        size[1] as f32,
        0.0,
        0.0,
        workspace[0],
        workspace[1],
        workspace[2],
        workspace[3],
    ];
    let mut bytes = [0; 48];
    for (chunk, value) in bytes.as_chunks_mut::<4>().0.iter_mut().zip(values) {
        chunk.copy_from_slice(&value.to_ne_bytes());
    }
    bytes
}

pub struct DocumentView {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    uniform: wgpu::Buffer,
    sampler: wgpu::Sampler,
    texture: wgpu::Texture,
    bind_group: wgpu::BindGroup,
    workspace: [f32; 4],
    uploaded: Option<(Placement, [u32; 2])>,
}

impl DocumentView {
    /// `format` is the target's format; `workspace`, opaque straight RGBA, is
    /// shown around the document.
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat, workspace: [u8; 4]) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("document view"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("document view"),
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
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("document view"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("document view"),
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
            label: Some("document view"),
            size: 48,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("document view"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..wgpu::SamplerDescriptor::default()
        });
        let texture = Self::create_texture(device, [1, 1]);
        let bind_group = Self::create_bind_group(device, &layout, &uniform, &texture, &sampler);
        Self {
            pipeline,
            layout,
            uniform,
            sampler,
            texture,
            bind_group,
            workspace: workspace.map(|value| f32::from(value) / 255.0),
            uploaded: None,
        }
    }

    fn create_texture(device: &wgpu::Device, [width, height]: [u32; 2]) -> wgpu::Texture {
        device.create_texture(&wgpu::TextureDescriptor {
            label: Some("document"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            // Not sRGB: the document holds encoded values, as the swap chain
            // does.
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
        sampler: &wgpu::Sampler,
    ) -> wgpu::BindGroup {
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("document view"),
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
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(sampler),
                },
            ],
        })
    }

    /// Uploads `rect` of `document` (premultiplied RGBA8); a new size
    /// replaces the texture and uploads everything.
    pub fn update(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        document: &Pixmap,
        rect: Option<PixelRect>,
    ) {
        let (width, height) = (u32::from(document.width()), u32::from(document.height()));
        let mut rect = rect;
        if self.texture.width() != width || self.texture.height() != height {
            self.texture = Self::create_texture(device, [width, height]);
            self.bind_group = Self::create_bind_group(
                device,
                &self.layout,
                &self.uniform,
                &self.texture,
                &self.sampler,
            );
            self.uploaded = None;
            rect = Some([0, 0, width, height]);
        }
        let Some([left, top, right, bottom]) = rect else {
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
            document.data_as_u8_slice(),
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

    /// Draws into `area` (left, top, right, bottom target pixels) of a
    /// target of `target_size`, the document placed by `placement`.
    pub fn draw(
        &mut self,
        queue: &wgpu::Queue,
        pass: &mut wgpu::RenderPass<'_>,
        area: [u32; 4],
        placement: Placement,
        target_size: [u32; 2],
    ) {
        let right = area[2].min(target_size[0]);
        let bottom = area[3].min(target_size[1]);
        if right <= area[0] || bottom <= area[1] {
            return;
        }
        let size = [self.texture.width(), self.texture.height()];
        if self.uploaded != Some((placement, size)) {
            queue.write_buffer(
                &self.uniform,
                0,
                &uniform_bytes(placement, size, self.workspace),
            );
            self.uploaded = Some((placement, size));
        }
        pass.set_viewport(
            area[0] as f32,
            area[1] as f32,
            (right - area[0]) as f32,
            (bottom - area[1]) as f32,
            0.0,
            1.0,
        );
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.draw(0..3, 0..1);
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    const WORKSPACE: [u8; 4] = [60, 62, 66, 255];

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

    /// Draws `document` into the whole of a `target`-sized image and reads
    /// it back.
    fn render(document: &Pixmap, placement: Placement, target: [u32; 2]) -> Option<Vec<u8>> {
        render_with(target, |device, queue, format| {
            let mut view = DocumentView::new(device, format, WORKSPACE);
            view.update(device, queue, document, None);
            move |queue: &wgpu::Queue, pass: &mut wgpu::RenderPass<'_>| {
                view.draw(queue, pass, [0, 0, target[0], target[1]], placement, target);
            }
        })
    }

    /// Clears a `target`-sized RGBA8 image to black, runs what `prepare`
    /// makes in one render pass over it and reads it back; `None` without a
    /// DX12 adapter.
    pub(crate) fn render_with<D: FnOnce(&wgpu::Queue, &mut wgpu::RenderPass<'_>)>(
        target: [u32; 2],
        prepare: impl FnOnce(&wgpu::Device, &wgpu::Queue, wgpu::TextureFormat) -> D,
    ) -> Option<Vec<u8>> {
        let (device, queue) = device()?;
        let format = wgpu::TextureFormat::Rgba8Unorm;
        let draw = prepare(&device, &queue, format);
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
            draw(&queue, &mut pass);
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

    /// An opaque document with a distinct colour per pixel.
    fn document() -> Pixmap {
        let mut pixmap = Pixmap::new(20, 12);
        for (index, pixel) in pixmap.data_as_u8_slice_mut().chunks_mut(4).enumerate() {
            pixel.copy_from_slice(&[(index * 7 % 256) as u8, (index * 13 % 256) as u8, 90, 255]);
        }
        pixmap
    }

    #[test]
    fn at_whole_scales_each_screen_pixel_shows_one_document_pixel() {
        let document = document();
        for scale in [1.0, 3.0] {
            let placement = Placement {
                offset: [5.0, 3.0],
                scale,
            };
            let target = [80, 48];
            let Some(shown) = render(&document, placement, target) else {
                eprintln!("skipped: no DX12 adapter");
                return;
            };
            let source = document.data_as_u8_slice();
            for y in 0..target[1] {
                for x in 0..target[0] {
                    let at = ((y * target[0] + x) * 4) as usize;
                    let document_x = ((x as f32 + 0.5 - 5.0) / scale).floor();
                    let document_y = ((y as f32 + 0.5 - 3.0) / scale).floor();
                    let inside =
                        (0.0..20.0).contains(&document_x) && (0.0..12.0).contains(&document_y);
                    let expected: [u8; 4] = if inside {
                        let from = ((document_y as usize) * 20 + document_x as usize) * 4;
                        source[from..from + 4].try_into().unwrap()
                    } else {
                        WORKSPACE
                    };
                    assert_eq!(
                        &shown[at..at + 4],
                        &expected,
                        "scale {scale}, screen pixel {x},{y}"
                    );
                }
            }
        }
    }

    #[test]
    fn below_100_percent_neighbouring_pixels_blend() {
        let mut document = Pixmap::new(4, 4);
        for (index, pixel) in document.data_as_u8_slice_mut().chunks_mut(4).enumerate() {
            let white = (index % 4 + index / 4) % 2 == 0;
            pixel.copy_from_slice(&if white { [255; 4] } else { [0, 0, 0, 255] });
        }
        let placement = Placement {
            offset: [0.0, 0.0],
            scale: 0.5,
        };
        let Some(shown) = render(&document, placement, [2, 2]) else {
            eprintln!("skipped: no DX12 adapter");
            return;
        };
        for pixel in shown.chunks(4) {
            assert!((100..=155).contains(&pixel[0]), "{pixel:?}");
        }
    }
}

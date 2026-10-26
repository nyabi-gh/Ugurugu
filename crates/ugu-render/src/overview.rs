// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! The navigator's small copy of the canvas: each of its pixels is the mean
//! of the canvas pixels under it, over the checkerboard the canvas shows
//! through transparency. It is drawn on the GPU from the texture the canvas
//! shows, again only where that texture changed. Sampling the texture as
//! the canvas does below 100% drops thin lines, and a mip chain blurs them
//! (evidence.md 96).

use crate::raster::PixelRect;
use crate::view::DocumentView;

const SHADER: &str = r"
struct Overview {
    // Canvas pixels per overview pixel.
    factor: vec2<f32>,
    // The canvas pixel of the texture's top left corner.
    origin: vec2<f32>,
    size: vec2<f32>,
    // Canvas pixels per texture pixel.
    shrink: f32,
    _pad: f32,
}

@group(0) @binding(0) var<uniform> overview: Overview;
@group(0) @binding(1) var shown: texture_2d<f32>;

@vertex
fn vertex(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let corner = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    return vec4<f32>(corner * vec2<f32>(2.0, -2.0) + vec2<f32>(-1.0, 1.0), 0.0, 1.0);
}

@fragment
fn fragment(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let cell = floor(position.xy);
    let low = clamp((cell * overview.factor - overview.origin) / overview.shrink, vec2<f32>(0.0), overview.size);
    let high = clamp(((cell + 1.0) * overview.factor - overview.origin) / overview.shrink, vec2<f32>(0.0), overview.size);
    if (any(high <= low)) {
        discard;
    }
    let first = vec2<i32>(floor(low));
    let last = vec2<i32>(ceil(high));
    var sum = vec4<f32>(0.0);
    var weights = 0.0;
    for (var y = first.y; y < last.y; y++) {
        let along_y = min(high.y, f32(y + 1)) - max(low.y, f32(y));
        for (var x = first.x; x < last.x; x++) {
            let weight = along_y * (min(high.x, f32(x + 1)) - max(low.x, f32(x)));
            sum += textureLoad(shown, vec2<i32>(x, y), 0) * weight;
            weights += weight;
        }
    }
    let ink = sum / weights;
    // The canvas's checkerboard of 8 screen pixels.
    let checker = vec2<i32>(floor(position.xy / 8.0));
    let light = select(0.8, 1.0, ((checker.x + checker.y) & 1) == 0);
    return ink + vec4<f32>(light, light, light, 1.0) * (1.0 - ink.a);
}
";

/// The part of the canvas a texture holds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Part {
    /// The canvas's size in pixels.
    pub canvas: [u32; 2],
    /// The canvas pixel at the texture's top left corner.
    pub origin: [u32; 2],
    /// Canvas pixels per texture pixel.
    pub shrink: u32,
}

impl Part {
    /// All of a `canvas`-sized canvas at its size.
    pub fn whole(canvas: [u32; 2]) -> Self {
        Self {
            canvas,
            origin: [0, 0],
            shrink: 1,
        }
    }
}

pub struct Overview {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    uniform: wgpu::Buffer,
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    /// The texture last drawn from and its bindings.
    source: Option<(wgpu::Texture, wgpu::BindGroup)>,
    written: Option<[u8; 32]>,
}

impl Overview {
    /// An overview of `size` pixels, opaque RGBA8 holding encoded values as
    /// the swap chain does, all of them drawn at the first `draw`.
    pub fn new(device: &wgpu::Device, size: [u32; 2]) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("overview"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("overview"),
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
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("overview"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("overview"),
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
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("overview"),
            size: 32,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let (texture, view) = Self::create_texture(device, size);
        Self {
            pipeline,
            layout,
            uniform,
            texture,
            view,
            source: None,
            written: None,
        }
    }

    fn create_texture(device: &wgpu::Device, size: [u32; 2]) -> (wgpu::Texture, wgpu::TextureView) {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("overview"),
            size: wgpu::Extent3d {
                width: size[0].max(1),
                height: size[1].max(1),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        (texture, view)
    }

    /// Makes the overview `size` pixels, to be drawn whole at the next
    /// `draw`; the pipeline stays, as making it again would cost each frame
    /// of a panel's resize a shader compile.
    pub fn resize(&mut self, device: &wgpu::Device, size: [u32; 2]) {
        (self.texture, self.view) = Self::create_texture(device, size);
    }

    pub fn size(&self) -> [u32; 2] {
        [self.texture.width(), self.texture.height()]
    }

    pub fn view(&self) -> &wgpu::TextureView {
        &self.view
    }

    #[cfg(test)]
    fn texture(&self) -> &wgpu::Texture {
        &self.texture
    }

    /// Draws the overview pixels over `changed`, a rectangle of `shown`'s
    /// texture, or over all of it. That texture holds `part` of the canvas;
    /// overview pixels it does not reach stay as they were.
    pub fn draw(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        shown: &DocumentView,
        part: Part,
        changed: Option<PixelRect>,
    ) {
        let Part {
            canvas,
            origin,
            shrink,
        } = part;
        let source = shown.texture();
        let size = [source.width(), source.height()];
        let out = self.size();
        let factor: [f64; 2] =
            std::array::from_fn(|axis| f64::from(canvas[axis]) / f64::from(out[axis]));
        let [left, top, right, bottom] = changed.unwrap_or([0, 0, size[0], size[1]]);
        let cell =
            |texture: u32, axis: usize| f64::from(origin[axis] + texture * shrink) / factor[axis];
        let first = [cell(left, 0), cell(top, 1)].map(|edge| edge.floor() as u32);
        let last: [u32; 2] = std::array::from_fn(|axis| {
            let edge = [right, bottom][axis];
            (cell(edge, axis).ceil() as u32).min(out[axis])
        });
        if last[0] <= first[0] || last[1] <= first[1] {
            return;
        }
        if self
            .source
            .as_ref()
            .is_none_or(|(texture, _)| texture != source)
        {
            let view = source.create_view(&wgpu::TextureViewDescriptor::default());
            let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("overview"),
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: self.uniform.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(&view),
                    },
                ],
            });
            self.source = Some((source.clone(), bind_group));
        }
        let values = [
            factor[0] as f32,
            factor[1] as f32,
            origin[0] as f32,
            origin[1] as f32,
            size[0] as f32,
            size[1] as f32,
            shrink as f32,
            0.0,
        ];
        let mut bytes = [0; 32];
        for (chunk, value) in bytes.as_chunks_mut::<4>().0.iter_mut().zip(values) {
            chunk.copy_from_slice(&value.to_ne_bytes());
        }
        if self.written != Some(bytes) {
            queue.write_buffer(&self.uniform, 0, &bytes);
            self.written = Some(bytes);
        }
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("overview"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_scissor_rect(first[0], first[1], last[0] - first[0], last[1] - first[1]);
            pass.set_pipeline(&self.pipeline);
            if let Some((_, bind_group)) = &self.source {
                pass.set_bind_group(0, bind_group, &[]);
            }
            pass.draw(0..3, 0..1);
        }
        queue.submit([encoder.finish()]);
    }
}

#[cfg(test)]
mod tests {
    use vello_cpu::Pixmap;

    use super::*;
    use crate::view::tests::{read, with_device};

    const WORKSPACE: [u8; 4] = [60, 62, 66, 255];

    fn opaque(size: [u16; 2], pixel: impl Fn(u16, u16) -> [u8; 4]) -> Pixmap {
        let mut pixmap = Pixmap::new(size[0], size[1]);
        for (index, each) in pixmap.data_as_u8_slice_mut().chunks_mut(4).enumerate() {
            let index = index as u16;
            each.copy_from_slice(&pixel(index % size[0], index / size[0]));
        }
        pixmap
    }

    fn at(pixels: &[u8], width: u32, [x, y]: [u32; 2]) -> [u8; 4] {
        let start = ((y * width + x) * 4) as usize;
        pixels[start..start + 4].try_into().unwrap()
    }

    #[test]
    fn each_pixel_is_the_mean_of_the_canvas_pixels_under_it() {
        // Three canvas pixels per overview pixel across and two down, with
        // values whose means are whole.
        let canvas = opaque([9, 4], |x, y| [(x * 30) as u8, (y * 60) as u8, 7, 255]);
        let drawn = with_device(|device, queue| {
            let mut view = DocumentView::new(device, wgpu::TextureFormat::Rgba8Unorm, WORKSPACE);
            view.update(device, queue, &canvas, None);
            let mut overview = Overview::new(device, [3, 2]);
            overview.draw(device, queue, &view, Part::whole([9, 4]), None);
            read(device, queue, overview.texture())
        });
        let Some(Some(drawn)) = drawn else {
            eprintln!("skipped: no DX12 adapter");
            return;
        };
        for y in 0..2 {
            for x in 0..3 {
                assert_eq!(
                    at(&drawn, 3, [x, y]),
                    [(x * 90 + 30) as u8, (y * 120 + 30) as u8, 7, 255],
                    "overview pixel {x},{y}"
                );
            }
        }
    }

    #[test]
    fn transparency_shows_the_canvas_checkerboard() {
        let canvas = Pixmap::new(32, 8);
        let drawn = with_device(|device, queue| {
            let mut view = DocumentView::new(device, wgpu::TextureFormat::Rgba8Unorm, WORKSPACE);
            view.update(device, queue, &canvas, None);
            let mut overview = Overview::new(device, [16, 4]);
            overview.draw(device, queue, &view, Part::whole([32, 8]), None);
            read(device, queue, overview.texture())
        });
        let Some(Some(drawn)) = drawn else {
            eprintln!("skipped: no DX12 adapter");
            return;
        };
        assert_eq!(at(&drawn, 16, [0, 0]), [255, 255, 255, 255]);
        assert_eq!(at(&drawn, 16, [8, 0]), [204, 204, 204, 255]);
    }

    #[test]
    fn a_change_redraws_only_the_pixels_over_it() {
        let before = opaque([8, 8], |_, _| [10, 10, 10, 255]);
        let after = opaque([8, 8], |_, _| [200, 200, 200, 255]);
        let drawn = with_device(|device, queue| {
            let mut view = DocumentView::new(device, wgpu::TextureFormat::Rgba8Unorm, WORKSPACE);
            view.update(device, queue, &before, None);
            let mut overview = Overview::new(device, [4, 4]);
            overview.draw(device, queue, &view, Part::whole([8, 8]), None);
            // Every pixel changes, but only the top left 3x3 is said to.
            view.update(device, queue, &after, Some([0, 0, 8, 8]));
            overview.draw(
                device,
                queue,
                &view,
                Part::whole([8, 8]),
                Some([0, 0, 3, 3]),
            );
            read(device, queue, overview.texture())
        });
        let Some(Some(drawn)) = drawn else {
            eprintln!("skipped: no DX12 adapter");
            return;
        };
        for y in 0..4 {
            for x in 0..4 {
                let expected = if x < 2 && y < 2 { 200 } else { 10 };
                assert_eq!(
                    at(&drawn, 4, [x, y]),
                    [expected, expected, expected, 255],
                    "overview pixel {x},{y}"
                );
            }
        }
    }

    #[test]
    fn a_part_shown_smaller_lands_where_it_is_on_the_canvas() {
        // The right half of an 8x4 canvas, shown at half size.
        let whole = opaque([8, 4], |_, _| [10, 10, 10, 255]);
        let part = opaque([2, 2], |_, _| [200, 200, 200, 255]);
        let drawn = with_device(|device, queue| {
            let mut view = DocumentView::new(device, wgpu::TextureFormat::Rgba8Unorm, WORKSPACE);
            view.update(device, queue, &whole, None);
            let mut overview = Overview::new(device, [8, 4]);
            overview.draw(device, queue, &view, Part::whole([8, 4]), None);
            view.update(device, queue, &part, None);
            let right_half = Part {
                canvas: [8, 4],
                origin: [4, 0],
                shrink: 2,
            };
            overview.draw(device, queue, &view, right_half, None);
            read(device, queue, overview.texture())
        });
        let Some(Some(drawn)) = drawn else {
            eprintln!("skipped: no DX12 adapter");
            return;
        };
        for y in 0..4 {
            for x in 0..8 {
                let expected = if x >= 4 { 200 } else { 10 };
                assert_eq!(
                    at(&drawn, 8, [x, y]),
                    [expected, expected, expected, 255],
                    "overview pixel {x},{y}"
                );
            }
        }
    }

    #[test]
    fn a_resized_overview_is_drawn_whole_at_its_new_size() {
        let canvas = opaque([8, 4], |x, _| [if x < 4 { 0 } else { 240 }, 0, 0, 255]);
        let drawn = with_device(|device, queue| {
            let mut view = DocumentView::new(device, wgpu::TextureFormat::Rgba8Unorm, WORKSPACE);
            view.update(device, queue, &canvas, None);
            let mut overview = Overview::new(device, [8, 4]);
            overview.draw(device, queue, &view, Part::whole([8, 4]), None);
            overview.resize(device, [2, 1]);
            overview.draw(device, queue, &view, Part::whole([8, 4]), None);
            (overview.size(), read(device, queue, overview.texture()))
        });
        let Some((size, Some(drawn))) = drawn else {
            eprintln!("skipped: no DX12 adapter");
            return;
        };
        assert_eq!(size, [2, 1]);
        assert_eq!(drawn, [0, 0, 0, 255, 240, 0, 0, 255]);
    }
}

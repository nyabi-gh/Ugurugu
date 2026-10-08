// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! Marching ants around the selection, drawn on the GPU over the canvas: a
//! light line under a dark dashed one, as 2.2.13 draws them. The outline is
//! uploaded once per selection as one instance per straight run, in document
//! pixels; each frame only the placement, the dash phase and a pending
//! transform change, so a selection of many small pieces costs no more per
//! frame than a rectangle, and moving it re-uploads nothing.

use std::sync::Arc;

use ugu_core::ops::Affine;
use ugu_core::selection::Selection;

use crate::view::Placement;

const SHADER: &str = r"
struct View {
    // From document pixels to target pixels, by rows.
    x: vec4<f32>,
    y: vec4<f32>,
    surface: vec2<f32>,
    pixels_per_point: f32,
    // Points the dashes have moved.
    phase: f32,
    // Target pixels per document pixel along the outline.
    stretch: f32,
}

fn place(point: vec2<f32>) -> vec2<f32> {
    let p = vec3<f32>(point, 1.0);
    return vec2<f32>(dot(view.x.xyz, p), dot(view.y.xyz, p));
}

@group(0) @binding(0) var<uniform> view: View;

struct Varying {
    @builtin(position) position: vec4<f32>,
    // Target pixels from the line's middle, and points along its loop.
    @location(0) across: f32,
    @location(1) along: f32,
}

const LIGHT_HALF: f32 = 0.9;
const DARK_HALF: f32 = 0.5;
const DASH: f32 = 4.0;

@vertex
fn vertex(
    @builtin(vertex_index) index: u32,
    @location(0) ends: vec4<f32>,
    @location(1) start: f32,
) -> Varying {
    let a = place(ends.xy);
    let b = place(ends.zw);
    let span = distance(a, b);
    let direction = (b - a) / max(span, 1e-6);
    let normal = vec2<f32>(-direction.y, direction.x);
    // Half the light line and a pixel to fade over; the runs overlap by as
    // much at each corner, so corners are covered.
    let half = LIGHT_HALF * view.pixels_per_point + 1.0;
    let end = f32(index == 1u || index == 4u || index == 5u);
    let side = select(-1.0, 1.0, index == 2u || index == 3u || index == 5u);
    let run = mix(-half, span + half, end);
    let at = a + direction * run + normal * side * half;
    var out: Varying;
    out.position = vec4<f32>(at / view.surface * vec2<f32>(2.0, -2.0) + vec2<f32>(-1.0, 1.0), 0.0, 1.0);
    out.across = side * half;
    out.along = (start * view.stretch + run) / view.pixels_per_point;
    return out;
}

@fragment
fn fragment(in: Varying) -> @location(0) vec4<f32> {
    let apart = abs(in.across);
    let light = 0.92 * clamp(LIGHT_HALF * view.pixels_per_point + 0.5 - apart, 0.0, 1.0);
    let on = fract((in.along + view.phase) / (2.0 * DASH)) < 0.5;
    let dark = select(0.0, 0.96 * clamp(DARK_HALF * view.pixels_per_point + 0.5 - apart, 0.0, 1.0), on);
    let ink = 20.0 / 255.0;
    let alpha = dark + (1.0 - dark) * light;
    return vec4<f32>(vec3<f32>(dark * ink + (1.0 - dark) * light), alpha);
}
";

/// One straight run of the outline: its ends, then how far along its loop it
/// starts, in document pixels.
type Run = [f32; 5];

const RUN_BYTES: u64 = std::mem::size_of::<Run>() as u64;

/// The outline's straight runs, each loop's lengths counted from its first
/// corner.
fn runs(loops: &[Vec<[i32; 2]>]) -> Vec<Run> {
    let mut runs = Vec::with_capacity(loops.iter().map(Vec::len).sum());
    for corners in loops {
        let mut along = 0.0;
        for (index, &[x0, y0]) in corners.iter().enumerate() {
            let [x1, y1] = corners[(index + 1) % corners.len()];
            runs.push([x0 as f32, y0 as f32, x1 as f32, y1 as f32, along]);
            along += (x1 - x0).abs() as f32 + (y1 - y0).abs() as f32;
        }
    }
    runs
}

pub struct Ants {
    pipeline: wgpu::RenderPipeline,
    uniform: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    /// The selection outlined, its runs and how many there are.
    traced: Option<(Arc<Selection>, wgpu::Buffer, u32)>,
    /// Moves the outline, as a pending transform moves the selection.
    moved: Affine,
    uploaded: Option<[u32; 16]>,
}

impl Ants {
    /// `format` is the target's format.
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("marching ants"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("marching ants"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("marching ants"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("marching ants"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vertex"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: RUN_BYTES,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x4, 1 => Float32],
                })],
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
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("marching ants"),
            size: 64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("marching ants"),
            layout: &layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform.as_entire_binding(),
            }],
        });
        Self {
            pipeline,
            uniform,
            bind_group,
            traced: None,
            moved: Affine::IDENTITY,
            uploaded: None,
        }
    }

    /// Shows the outline of `selection` moved by `moved`, tracing and
    /// uploading it when it is not the one shown; `None` shows nothing.
    pub fn show(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        selection: Option<&Arc<Selection>>,
        moved: Affine,
    ) {
        self.moved = moved;
        let Some(selection) = selection else {
            self.traced = None;
            return;
        };
        if self
            .traced
            .as_ref()
            .is_some_and(|(traced, ..)| Arc::ptr_eq(traced, selection))
        {
            return;
        }
        let runs = runs(&selection.outline());
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("marching ants"),
            size: (runs.len() as u64 * RUN_BYTES).max(RUN_BYTES),
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bytes: Vec<u8> = runs
            .iter()
            .flatten()
            .flat_map(|value| value.to_ne_bytes())
            .collect();
        queue.write_buffer(&buffer, 0, &bytes);
        self.traced = Some((selection.clone(), buffer, runs.len() as u32));
    }

    /// Draws the outline shown into `area` (left, top, right, bottom target
    /// pixels) of a target of `target_size`, the document placed by
    /// `placement`, the dashes moved `phase` points along.
    #[expect(clippy::too_many_arguments, reason = "one draw's whole state")]
    pub fn draw(
        &mut self,
        queue: &wgpu::Queue,
        pass: &mut wgpu::RenderPass<'_>,
        area: [u32; 4],
        placement: Placement,
        pixels_per_point: f32,
        phase: f32,
        target_size: [u32; 2],
    ) {
        let Some((_, buffer, count)) = &self.traced else {
            return;
        };
        let right = area[2].min(target_size[0]);
        let bottom = area[3].min(target_size[1]);
        if right <= area[0] || bottom <= area[1] {
            return;
        }
        let [a, b, c, d, e, f] = self.moved.0;
        let (scale, [x, y]) = (f64::from(placement.scale), placement.offset.map(f64::from));
        // Lengths along the outline scale by the transform's mean stretch;
        // exact unless it stretches one way more than the other.
        let stretch = scale * (a * e - b * d).abs().sqrt();
        let values = [
            scale * a,
            scale * b,
            scale * c + x,
            0.0,
            scale * d,
            scale * e,
            scale * f + y,
            0.0,
            f64::from(target_size[0]),
            f64::from(target_size[1]),
            f64::from(pixels_per_point),
            f64::from(phase),
            stretch,
            0.0,
            0.0,
            0.0,
        ]
        .map(|value| value as f32);
        let bits = values.map(f32::to_bits);
        if self.uploaded != Some(bits) {
            let bytes: Vec<u8> = values
                .iter()
                .flat_map(|value| value.to_ne_bytes())
                .collect();
            queue.write_buffer(&self.uniform, 0, &bytes);
            self.uploaded = Some(bits);
        }
        pass.set_viewport(
            0.0,
            0.0,
            target_size[0] as f32,
            target_size[1] as f32,
            0.0,
            1.0,
        );
        pass.set_scissor_rect(area[0], area[1], right - area[0], bottom - area[1]);
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.set_vertex_buffer(0, buffer.slice(..));
        pass.draw(0..6, 0..*count);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ugu_core::selection::Shape;

    #[test]
    fn runs_go_round_each_loop_counting_length_from_its_first_corner() {
        let loops = vec![
            vec![[1, 2], [4, 2], [4, 4], [1, 4]],
            vec![[0, 0], [1, 0], [1, 1], [0, 1]],
        ];
        assert_eq!(
            runs(&loops),
            [
                [1.0, 2.0, 4.0, 2.0, 0.0],
                [4.0, 2.0, 4.0, 4.0, 3.0],
                [4.0, 4.0, 1.0, 4.0, 5.0],
                [1.0, 4.0, 1.0, 2.0, 8.0],
                [0.0, 0.0, 1.0, 0.0, 0.0],
                [1.0, 0.0, 1.0, 1.0, 1.0],
                [1.0, 1.0, 0.0, 1.0, 2.0],
                [0.0, 1.0, 0.0, 0.0, 3.0],
            ]
        );
    }

    #[test]
    fn the_ants_lie_on_the_selection_edge_and_move_with_the_phase() {
        let selection = Arc::new(
            Selection::of_shape(&Shape::Rectangle([2.0, 2.0], [8.0, 6.0]), [10, 8]).unwrap(),
        );
        let placement = Placement {
            offset: [3.5, 1.5],
            scale: 4.0,
        };
        let target = [48, 36];
        let shot = |phase: f32, moved: Affine| {
            crate::view::tests::render_with(target, |device, queue, format| {
                let mut ants = Ants::new(device, format);
                ants.show(device, queue, Some(&selection), moved);
                move |queue: &wgpu::Queue, pass: &mut wgpu::RenderPass<'_>| {
                    ants.draw(
                        queue,
                        pass,
                        [0, 0, target[0], target[1]],
                        placement,
                        1.0,
                        phase,
                        target,
                    );
                }
            })
        };
        let Some(first) = shot(0.0, Affine::IDENTITY) else {
            eprintln!("skipped: no DX12 adapter");
            return;
        };
        let at = |pixels: &[u8], x: u32, y: u32| {
            let index = ((y * target[0] + x) * 4) as usize;
            [pixels[index], pixels[index + 1], pixels[index + 2]]
        };
        // The top edge runs through the middle of screen row 9, x 11.5 to 35.5.
        let row: Vec<[u8; 3]> = (12..34).map(|x| at(&first, x, 9)).collect();
        assert!(
            row.iter().any(|&pixel| pixel[0] > 200),
            "light gaps: {row:?}"
        );
        assert!(
            row.iter().any(|&pixel| pixel[0] < 60 && pixel != [0; 3]),
            "dark dashes: {row:?}"
        );
        // Inside and outside, away from the edge, nothing is drawn.
        assert_eq!(at(&first, 23, 17), [0; 3]);
        assert_eq!(at(&first, 23, 3), [0; 3]);
        assert_eq!(at(&first, 44, 30), [0; 3]);
        let later = shot(4.0, Affine::IDENTITY).unwrap();
        let later_row: Vec<[u8; 3]> = (12..34).map(|x| at(&later, x, 9)).collect();
        assert_ne!(row, later_row);
        // A pending transform a document pixel down moves the edge 4 rows.
        let moved = shot(0.0, Affine::translation(0.0, 1.0)).unwrap();
        let moved_row: Vec<[u8; 3]> = (12..34).map(|x| at(&moved, x, 13)).collect();
        assert_eq!(row, moved_row);
        assert_eq!(at(&moved, 23, 9), [0; 3]);
    }
}

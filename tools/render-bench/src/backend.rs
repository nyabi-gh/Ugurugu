// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! The same operations on tiny-skia and Vello CPU. Images are premultiplied
//! RGBA8 in both.

use std::sync::Arc;

use crate::scene::{Blend, CLIP_BASE, CLIPPED, Draw, LAYER_BLENDS, Rgba, Segment, circle};

pub trait Backend {
    type Image;

    fn new_image(&self, size: u32) -> Self::Image;
    /// Clears `target` and draws `draws` into it.
    fn rasterize(&mut self, draws: &[Draw], target: &mut Self::Image);
    /// Composites `layers` over white, with `LAYER_BLENDS` and the clip group.
    fn composite(&mut self, layers: &mut [Self::Image], out: &mut Self::Image);
    /// Draws `source` through `affine` (a, b, c, d, e, f) with bilinear sampling.
    fn transform(&mut self, source: &mut Self::Image, affine: [f32; 6], out: &mut Self::Image);
    /// Draws one live stroke segment over `target`, touching only `dirty`.
    fn draw_segment(&mut self, draw: &Draw, dirty: [u32; 4], target: &mut Self::Image);
    fn bytes(&self, image: &Self::Image) -> Vec<u8>;
}

pub struct TinySkia {
    group: tiny_skia::Pixmap,
}

impl TinySkia {
    pub fn new(size: u32) -> Self {
        Self {
            group: tiny_skia::Pixmap::new(size, size).expect("valid size"),
        }
    }
}

fn skia_color([r, g, b, a]: Rgba) -> tiny_skia::Color {
    tiny_skia::Color::from_rgba8(r, g, b, a)
}

fn skia_path(segments: &[Segment]) -> Option<tiny_skia::Path> {
    let mut builder = tiny_skia::PathBuilder::new();
    for segment in segments {
        match *segment {
            Segment::Move([x, y]) => builder.move_to(x, y),
            Segment::Line([x, y]) => builder.line_to(x, y),
            Segment::Cubic([x1, y1], [x2, y2], [x, y]) => builder.cubic_to(x1, y1, x2, y2, x, y),
            Segment::Close => builder.close(),
        }
    }
    builder.finish()
}

fn skia_blend(blend: Blend) -> tiny_skia::BlendMode {
    match blend {
        Blend::Normal => tiny_skia::BlendMode::SourceOver,
        Blend::Multiply => tiny_skia::BlendMode::Multiply,
        Blend::Screen => tiny_skia::BlendMode::Screen,
        Blend::Overlay => tiny_skia::BlendMode::Overlay,
    }
}

fn skia_draw(target: &mut tiny_skia::Pixmap, draw: &Draw) {
    let identity = tiny_skia::Transform::identity();
    match draw {
        Draw::Fill { path, color } => {
            let Some(path) = skia_path(path) else { return };
            let mut paint = tiny_skia::Paint::default();
            paint.set_color(skia_color(*color));
            target.fill_path(&path, &paint, tiny_skia::FillRule::Winding, identity, None);
        }
        Draw::Stroke {
            points,
            width,
            color,
        } => {
            let mut builder = tiny_skia::PathBuilder::new();
            builder.move_to(points[0][0], points[0][1]);
            for point in &points[1..] {
                builder.line_to(point[0], point[1]);
            }
            let Some(path) = builder.finish() else { return };
            let mut paint = tiny_skia::Paint::default();
            paint.set_color(skia_color(*color));
            let stroke = tiny_skia::Stroke {
                width: *width,
                line_cap: tiny_skia::LineCap::Round,
                line_join: tiny_skia::LineJoin::Round,
                ..tiny_skia::Stroke::default()
            };
            target.stroke_path(&path, &paint, &stroke, identity, None);
        }
        Draw::Dab {
            center,
            radius,
            color,
        } => {
            let [r, g, b, _] = *color;
            let centre = tiny_skia::Point::from_xy(center[0], center[1]);
            let Some(shader) = tiny_skia::RadialGradient::new(
                centre,
                0.0,
                centre,
                *radius,
                vec![
                    tiny_skia::GradientStop::new(0.0, skia_color(*color)),
                    tiny_skia::GradientStop::new(0.5, skia_color(*color)),
                    tiny_skia::GradientStop::new(1.0, skia_color([r, g, b, 0])),
                ],
                tiny_skia::SpreadMode::Pad,
                identity,
            ) else {
                return;
            };
            let Some(path) = skia_path(&circle(*center, *radius)) else {
                return;
            };
            let paint = tiny_skia::Paint {
                shader,
                ..tiny_skia::Paint::default()
            };
            target.fill_path(&path, &paint, tiny_skia::FillRule::Winding, identity, None);
        }
    }
}

impl Backend for TinySkia {
    type Image = tiny_skia::Pixmap;

    fn new_image(&self, size: u32) -> Self::Image {
        tiny_skia::Pixmap::new(size, size).expect("valid size")
    }

    fn rasterize(&mut self, draws: &[Draw], target: &mut Self::Image) {
        target.fill(tiny_skia::Color::TRANSPARENT);
        for draw in draws {
            skia_draw(target, draw);
        }
    }

    fn composite(&mut self, layers: &mut [Self::Image], out: &mut Self::Image) {
        let identity = tiny_skia::Transform::identity();
        let paint = |blend: tiny_skia::BlendMode| tiny_skia::PixmapPaint {
            opacity: 1.0,
            blend_mode: blend,
            quality: tiny_skia::FilterQuality::Nearest,
        };
        out.fill(tiny_skia::Color::WHITE);
        for (layer, blend) in layers.iter().zip(LAYER_BLENDS).take(CLIP_BASE) {
            out.draw_pixmap(
                0,
                0,
                layer.as_ref(),
                &paint(skia_blend(blend)),
                identity,
                None,
            );
        }
        self.group
            .data_mut()
            .copy_from_slice(layers[CLIP_BASE].data());
        self.group.draw_pixmap(
            0,
            0,
            layers[CLIPPED].as_ref(),
            &paint(tiny_skia::BlendMode::SourceAtop),
            identity,
            None,
        );
        out.draw_pixmap(
            0,
            0,
            self.group.as_ref(),
            &paint(skia_blend(LAYER_BLENDS[CLIP_BASE])),
            identity,
            None,
        );
    }

    fn transform(&mut self, source: &mut Self::Image, affine: [f32; 6], out: &mut Self::Image) {
        let [a, b, c, d, e, f] = affine;
        out.fill(tiny_skia::Color::TRANSPARENT);
        out.draw_pixmap(
            0,
            0,
            source.as_ref(),
            &tiny_skia::PixmapPaint {
                quality: tiny_skia::FilterQuality::Bilinear,
                ..tiny_skia::PixmapPaint::default()
            },
            tiny_skia::Transform::from_row(a, b, c, d, e, f),
            None,
        );
    }

    fn draw_segment(&mut self, draw: &Draw, _dirty: [u32; 4], target: &mut Self::Image) {
        // tiny-skia already limits work to the path bounds.
        skia_draw(target, draw);
    }

    fn bytes(&self, image: &Self::Image) -> Vec<u8> {
        image.data().to_vec()
    }
}

pub struct Vello {
    context: vello_cpu::RenderContext,
    /// Sized to each live segment's dirty rectangle.
    live: vello_cpu::RenderContext,
    resources: vello_cpu::Resources,
    size: u16,
}

use vello_cpu::kurbo::{self, Affine, BezPath, Rect, Shape};
use vello_cpu::peniko::{BlendMode, Compose, ImageQuality, ImageSampler, Mix};

impl Vello {
    /// `threads` 0 renders on the calling thread only.
    pub fn new(size: u32, threads: u16, level: vello_cpu::Level) -> Self {
        let size = u16::try_from(size).expect("Vello CPU images are at most 65535 wide");
        let settings = vello_cpu::RenderSettings {
            num_threads: threads,
            level,
        };
        Self {
            context: vello_cpu::RenderContext::new_with(size, size, settings),
            live: vello_cpu::RenderContext::new_with(1, 1, settings),
            resources: vello_cpu::Resources::new(),
            size,
        }
    }

    fn render(&mut self, target: &mut Arc<vello_cpu::Pixmap>) {
        render(
            &mut self.context,
            &mut self.resources,
            target,
            vello_cpu::RasterizerSettings::default(),
        );
    }

    fn fill_image(&mut self, image: &Arc<vello_cpu::Pixmap>, quality: ImageQuality) {
        self.context.set_paint(vello_cpu::Image {
            image: vello_cpu::ImageSource::Pixmap(image.clone()),
            sampler: ImageSampler::default().with_quality(quality),
        });
        let (width, height) = (f64::from(image.width()), f64::from(image.height()));
        self.context.fill_rect(&Rect::new(0.0, 0.0, width, height));
    }
}

/// The drawing calls Vello CPU and Vello GPU share.
pub trait Painter {
    fn set_paint(&mut self, paint: vello_cpu::PaintType);
    fn set_stroke(&mut self, stroke: kurbo::Stroke);
    fn fill_path(&mut self, path: &BezPath);
    fn stroke_path(&mut self, path: &BezPath);
}

impl Painter for vello_cpu::RenderContext {
    fn set_paint(&mut self, paint: vello_cpu::PaintType) {
        vello_cpu::RenderContext::set_paint(self, paint);
    }
    fn set_stroke(&mut self, stroke: kurbo::Stroke) {
        vello_cpu::RenderContext::set_stroke(self, stroke);
    }
    fn fill_path(&mut self, path: &BezPath) {
        vello_cpu::RenderContext::fill_path(self, path);
    }
    fn stroke_path(&mut self, path: &BezPath) {
        vello_cpu::RenderContext::stroke_path(self, path);
    }
}

pub fn draw(context: &mut impl Painter, draw: &Draw) {
    match draw {
        Draw::Fill { path, color } => {
            context.set_paint(vello_color(*color).into());
            context.fill_path(&bez_path(path));
        }
        Draw::Stroke {
            points,
            width,
            color,
        } => {
            let mut path = BezPath::new();
            path.move_to(point(points[0]));
            for &next in &points[1..] {
                path.line_to(point(next));
            }
            context.set_paint(vello_color(*color).into());
            context.set_stroke(
                kurbo::Stroke::new(f64::from(*width))
                    .with_caps(kurbo::Cap::Round)
                    .with_join(kurbo::Join::Round),
            );
            context.stroke_path(&path);
        }
        Draw::Dab {
            center,
            radius,
            color,
        } => {
            let [r, g, b, _] = *color;
            let gradient = vello_cpu::peniko::Gradient::new_radial(point(*center), *radius)
                .with_stops([
                    (0.0, vello_color(*color)),
                    (0.5, vello_color(*color)),
                    (1.0, vello_color([r, g, b, 0])),
                ]);
            context.set_paint(gradient.into());
            context.fill_path(&kurbo::Circle::new(point(*center), f64::from(*radius)).to_path(0.1));
        }
    }
}

fn render(
    context: &mut vello_cpu::RenderContext,
    resources: &mut vello_cpu::Resources,
    target: &mut Arc<vello_cpu::Pixmap>,
    settings: vello_cpu::RasterizerSettings,
) {
    context.flush();
    let pixmap = Arc::get_mut(target).expect("no scene holds the target");
    context.render_with(pixmap, resources, settings);
    // Drops the scene's references to images, so they can be drawn into again.
    context.reset();
}

pub fn point([x, y]: [f32; 2]) -> kurbo::Point {
    kurbo::Point::new(f64::from(x), f64::from(y))
}

pub fn vello_color([r, g, b, a]: Rgba) -> vello_cpu::color::AlphaColor<vello_cpu::color::Srgb> {
    vello_cpu::color::AlphaColor::from_rgba8(r, g, b, a)
}

pub fn bez_path(segments: &[Segment]) -> BezPath {
    let mut path = BezPath::new();
    for segment in segments {
        match *segment {
            Segment::Move(to) => path.move_to(point(to)),
            Segment::Line(to) => path.line_to(point(to)),
            Segment::Cubic(c1, c2, to) => path.curve_to(point(c1), point(c2), point(to)),
            Segment::Close => path.close_path(),
        }
    }
    path
}

pub fn vello_blend(blend: Blend) -> BlendMode {
    let mix = match blend {
        Blend::Normal => Mix::Normal,
        Blend::Multiply => Mix::Multiply,
        Blend::Screen => Mix::Screen,
        Blend::Overlay => Mix::Overlay,
    };
    BlendMode::new(mix, Compose::SrcOver)
}

impl Backend for Vello {
    type Image = Arc<vello_cpu::Pixmap>;

    fn new_image(&self, size: u32) -> Self::Image {
        let size = u16::try_from(size).expect("Vello CPU images are at most 65535 wide");
        Arc::new(vello_cpu::Pixmap::new(size, size))
    }

    fn rasterize(&mut self, draws: &[Draw], target: &mut Self::Image) {
        for item in draws {
            draw(&mut self.context, item);
        }
        self.render(target);
    }

    fn composite(&mut self, layers: &mut [Self::Image], out: &mut Self::Image) {
        let size = f64::from(self.size);
        self.context.set_paint(vello_color([255, 255, 255, 255]));
        self.context.fill_rect(&Rect::new(0.0, 0.0, size, size));
        for (layer, blend) in layers.iter().zip(LAYER_BLENDS).take(CLIP_BASE) {
            if blend == Blend::Normal {
                self.fill_image(layer, ImageQuality::Low);
            } else {
                self.context.push_blend_layer(vello_blend(blend));
                self.fill_image(layer, ImageQuality::Low);
                self.context.pop_layer();
            }
        }
        self.context
            .push_blend_layer(vello_blend(LAYER_BLENDS[CLIP_BASE]));
        self.fill_image(&layers[CLIP_BASE], ImageQuality::Low);
        self.context
            .push_blend_layer(BlendMode::new(Mix::Normal, Compose::SrcAtop));
        self.fill_image(&layers[CLIPPED], ImageQuality::Low);
        self.context.pop_layer();
        self.context.pop_layer();
        self.render(out);
    }

    fn transform(&mut self, source: &mut Self::Image, affine: [f32; 6], out: &mut Self::Image) {
        self.context
            .set_transform(Affine::new(affine.map(f64::from)));
        self.fill_image(source, ImageQuality::Medium);
        self.context.reset_transform();
        self.render(out);
    }

    fn draw_segment(&mut self, segment: &Draw, dirty: [u32; 4], target: &mut Self::Image) {
        let [left, top, right, bottom] = dirty.map(|value| value as u16);
        self.live.reset_and_resize(right - left, bottom - top);
        self.live
            .set_transform(Affine::translate((-f64::from(left), -f64::from(top))));
        draw(&mut self.live, segment);
        render(
            &mut self.live,
            &mut self.resources,
            target,
            vello_cpu::RasterizerSettings {
                target_init: vello_cpu::TargetInit::SrcOver,
                offset: (left, top),
                ..vello_cpu::RasterizerSettings::default()
            },
        );
    }

    fn bytes(&self, image: &Self::Image) -> Vec<u8> {
        image.data_as_u8_slice().to_vec()
    }
}

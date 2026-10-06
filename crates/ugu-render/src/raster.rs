// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! Canvas pixels drawn on the CPU with Vello CPU.
//!
//! One input step touches a few pixels, so it is drawn on the calling thread
//! into its dirty rectangle only; handing that to worker threads costs more
//! than it saves. A full redraw uses the worker threads. Both produce the same
//! pixels, so what is shown while drawing matches a later redraw.

use vello_cpu::kurbo::{Affine, BezPath, Cap, Circle, Join, Point, Shape, Stroke};
use vello_cpu::{RasterizerSettings, RenderContext, RenderSettings, Resources, TargetInit};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StrokeStyle {
    /// Physical pixels.
    pub width: f32,
    /// Straight (not premultiplied) RGBA.
    pub color: [u8; 4],
}

/// Pixel rectangle: left, top, right, bottom (exclusive).
pub type PixelRect = [u32; 4];

pub struct CanvasRaster {
    /// Premultiplied RGBA8.
    pixmap: vello_cpu::Pixmap,
    live: RenderContext,
    full: RenderContext,
    resources: Resources,
    /// Pixels changed since the last `take_dirty`.
    dirty: Option<PixelRect>,
}

fn point([x, y]: [f32; 2]) -> Point {
    Point::new(f64::from(x), f64::from(y))
}

fn color([r, g, b, a]: [u8; 4]) -> vello_cpu::color::AlphaColor<vello_cpu::color::Srgb> {
    vello_cpu::color::AlphaColor::from_rgba8(r, g, b, a)
}

fn pen(style: StrokeStyle) -> Stroke {
    Stroke::new(f64::from(style.width))
        .with_caps(Cap::Round)
        .with_join(Join::Round)
}

fn union(a: Option<PixelRect>, b: PixelRect) -> PixelRect {
    match a {
        Some(a) => [
            a[0].min(b[0]),
            a[1].min(b[1]),
            a[2].max(b[2]),
            a[3].max(b[3]),
        ],
        None => b,
    }
}

/// The SIMD level every document is drawn with, so that all CPUs give the
/// same pixels. Vello's AVX2 path fuses multiply-adds (FMA), which rounds
/// gradient colours differently; SSE4.2, which Windows 11 requires, has no FMA
/// and draws the same bytes as plain SSE2. It costs no measured speed.
pub fn document_level() -> vello_cpu::Level {
    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    if let Some(sse) = vello_cpu::Level::new().as_sse4_2() {
        return vello_cpu::Level::Sse4_2(sse);
    }
    vello_cpu::Level::baseline()
}

impl CanvasRaster {
    pub fn new(size: [u32; 2]) -> Self {
        Self::with_level(size, document_level())
    }

    /// Uses the SIMD instructions of `level` instead of the best ones present.
    pub fn with_level(size: [u32; 2], level: vello_cpu::Level) -> Self {
        let [width, height] = Self::pixmap_size(size);
        let settings = RenderSettings {
            level,
            ..RenderSettings::default()
        };
        Self {
            pixmap: vello_cpu::Pixmap::new(width, height),
            live: RenderContext::new_with(
                1,
                1,
                RenderSettings {
                    num_threads: 0,
                    ..settings
                },
            ),
            full: RenderContext::new_with(width, height, settings),
            resources: Resources::new(),
            dirty: Some([0, 0, u32::from(width), u32::from(height)]),
        }
    }

    fn pixmap_size(size: [u32; 2]) -> [u16; 2] {
        size.map(|value| value.clamp(1, u32::from(u16::MAX)) as u16)
    }

    pub fn size(&self) -> [u32; 2] {
        [
            u32::from(self.pixmap.width()),
            u32::from(self.pixmap.height()),
        ]
    }

    /// Premultiplied RGBA8 rows of `size()[0]` pixels.
    pub fn pixels(&self) -> &[u8] {
        self.pixmap.data_as_u8_slice()
    }

    pub fn take_dirty(&mut self) -> Option<PixelRect> {
        self.dirty.take()
    }

    /// Resizes to a cleared canvas; the caller redraws what it holds.
    pub fn resize(&mut self, size: [u32; 2]) {
        if size == self.size() {
            return;
        }
        let level = self.full.render_settings().level;
        *self = Self::with_level(size, level);
    }

    pub fn clear(&mut self) {
        self.pixmap.data_as_u8_slice_mut().fill(0);
        let [width, height] = self.size();
        self.dirty = Some([0, 0, width, height]);
    }

    /// The pixels a shape within `bounds` (in pixels, unclamped) may touch.
    fn clamp(&self, [left, top, right, bottom]: [f32; 4]) -> Option<PixelRect> {
        let [width, height] = self.size();
        // One more pixel each way for antialiasing.
        let rect = [
            (left.floor() - 1.0).clamp(0.0, width as f32) as u32,
            (top.floor() - 1.0).clamp(0.0, height as f32) as u32,
            (right.ceil() + 1.0).clamp(0.0, width as f32) as u32,
            (bottom.ceil() + 1.0).clamp(0.0, height as f32) as u32,
        ];
        (rect[0] < rect[2] && rect[1] < rect[3]).then_some(rect)
    }

    /// Draws over the canvas within `rect` only, on this thread.
    fn draw_live(&mut self, rect: PixelRect, draw: impl FnOnce(&mut RenderContext)) {
        let [left, top, right, bottom] = rect.map(|value| value as u16);
        self.live.reset_and_resize(right - left, bottom - top);
        self.live
            .set_transform(Affine::translate((-f64::from(left), -f64::from(top))));
        draw(&mut self.live);
        self.live.flush();
        self.live.render_with(
            &mut self.pixmap,
            &mut self.resources,
            RasterizerSettings {
                target_init: TargetInit::SrcOver,
                offset: (left, top),
                ..RasterizerSettings::default()
            },
        );
        self.dirty = Some(union(self.dirty, rect));
    }

    /// The first sample of a stroke: a round dot.
    pub fn dot(&mut self, at: [f32; 2], style: StrokeStyle) {
        let radius = style.width * 0.5;
        let Some(rect) = self.clamp([
            at[0] - radius,
            at[1] - radius,
            at[0] + radius,
            at[1] + radius,
        ]) else {
            return;
        };
        self.draw_live(rect, |context| {
            context.set_paint(color(style.color));
            context.fill_path(&Circle::new(point(at), f64::from(radius)).to_path(0.1));
        });
    }

    /// One more segment of a stroke. Consecutive segments overlap at their
    /// round ends, so translucent colours would darken there.
    pub fn segment(&mut self, from: [f32; 2], to: [f32; 2], style: StrokeStyle) {
        let radius = style.width * 0.5;
        let Some(rect) = self.clamp([
            from[0].min(to[0]) - radius,
            from[1].min(to[1]) - radius,
            from[0].max(to[0]) + radius,
            from[1].max(to[1]) + radius,
        ]) else {
            return;
        };
        self.draw_live(rect, |context| {
            let mut path = BezPath::new();
            path.move_to(point(from));
            path.line_to(point(to));
            context.set_paint(color(style.color));
            context.set_stroke(pen(style));
            context.stroke_path(&path);
        });
    }

    /// Clears the canvas and draws `strokes` on the worker threads.
    pub fn redraw<'a>(&mut self, strokes: impl IntoIterator<Item = (&'a [[f32; 2]], StrokeStyle)>) {
        for (points, style) in strokes {
            let Some((&first, rest)) = points.split_first() else {
                continue;
            };
            self.full.set_paint(color(style.color));
            if rest.is_empty() {
                self.full.fill_path(
                    &Circle::new(point(first), f64::from(style.width * 0.5)).to_path(0.1),
                );
                continue;
            }
            let mut path = BezPath::new();
            path.move_to(point(first));
            for &next in rest {
                path.line_to(point(next));
            }
            self.full.set_stroke(pen(style));
            self.full.stroke_path(&path);
        }
        self.full.flush();
        self.full.render_with(
            &mut self.pixmap,
            &mut self.resources,
            RasterizerSettings::default(),
        );
        self.full.reset();
        let [width, height] = self.size();
        self.dirty = Some([0, 0, width, height]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const STYLE: StrokeStyle = StrokeStyle {
        width: 4.0,
        color: [20, 20, 20, 255],
    };

    fn alpha_at(raster: &CanvasRaster, [x, y]: [u32; 2]) -> u8 {
        raster.pixels()[((y * raster.size()[0] + x) * 4 + 3) as usize]
    }

    #[test]
    fn a_segment_marks_only_its_area_dirty() {
        let mut raster = CanvasRaster::new([64, 64]);
        raster.take_dirty();
        raster.segment([10.0, 10.0], [20.0, 12.0], STYLE);
        let [left, top, right, bottom] = raster.take_dirty().expect("drawn");
        assert!(left <= 8 && top <= 8 && right >= 22 && bottom >= 14);
        assert!(right <= 24 && bottom <= 16);
        assert_eq!(alpha_at(&raster, [15, 11]), 255);
        assert_eq!(alpha_at(&raster, [40, 40]), 0);
    }

    #[test]
    fn live_drawing_matches_a_redraw() {
        let points = [[5.0, 5.0], [30.0, 9.0], [41.0, 50.0]];
        let mut live = CanvasRaster::new([64, 64]);
        live.dot(points[0], STYLE);
        for pair in points.windows(2) {
            live.segment(pair[0], pair[1], STYLE);
        }
        let mut redrawn = CanvasRaster::new([64, 64]);
        redrawn.redraw([(&points[..], STYLE)]);
        // Opaque ink: overlapping round ends do not change the result,
        // except for antialiased edge pixels covered twice.
        let differing = live
            .pixels()
            .iter()
            .zip(redrawn.pixels())
            .filter(|(a, b)| a.abs_diff(**b) > 8)
            .count();
        assert!(differing <= 16, "{differing} channels differ");
    }

    /// Every SIMD level this CPU has, so that pixels can be compared across
    /// the levels other users' CPUs would pick.
    fn levels() -> Vec<(&'static str, vello_cpu::Level)> {
        let detected = vello_cpu::Level::new();
        let mut levels = vec![("baseline", vello_cpu::Level::baseline())];
        #[cfg(target_arch = "x86_64")]
        {
            if let Some(sse) = detected.as_sse4_2() {
                levels.push(("sse4.2", vello_cpu::Level::Sse4_2(sse)));
            }
            if let Some(avx) = detected.as_avx2() {
                levels.push(("avx2", vello_cpu::Level::Avx2(avx)));
            }
            if let Some(avx) = detected.as_avx512() {
                levels.push(("avx512", vello_cpu::Level::Avx512(avx)));
            }
        }
        levels
    }

    fn draw_sample(level: vello_cpu::Level) -> (Vec<u8>, Vec<u8>) {
        let thin = StrokeStyle {
            width: 1.3,
            color: [200, 40, 30, 255],
        };
        let wide = StrokeStyle {
            width: 17.5,
            color: [30, 90, 200, 150],
        };
        let points: Vec<[f32; 2]> = (0..60)
            .map(|index| {
                let angle = index as f32 * 0.37;
                [
                    64.0 + angle.cos() * (10.0 + index as f32),
                    64.0 + angle.sin() * 40.0,
                ]
            })
            .collect();
        let mut live = CanvasRaster::with_level([128, 128], level);
        live.dot(points[0], thin);
        for pair in points.windows(2) {
            live.segment(pair[0], pair[1], thin);
        }
        let mut redrawn = CanvasRaster::with_level([128, 128], level);
        redrawn.redraw([
            (&points[..], wide),
            (&points[5..9], thin),
            (&points[..1], wide),
        ]);
        (live.pixels().to_vec(), redrawn.pixels().to_vec())
    }

    #[test]
    fn every_simd_level_draws_the_same_pixels() {
        let levels = levels();
        let (reference_name, reference_level) = levels[0];
        let reference = draw_sample(reference_level);
        for &(name, level) in &levels[1..] {
            let pixels = draw_sample(level);
            assert!(
                pixels.0 == reference.0,
                "live drawing differs between {reference_name} and {name}"
            );
            assert!(
                pixels.1 == reference.1,
                "redraw differs between {reference_name} and {name}"
            );
        }
    }

    #[test]
    fn shapes_outside_the_canvas_are_skipped() {
        let mut raster = CanvasRaster::new([16, 16]);
        raster.take_dirty();
        raster.segment([-50.0, -50.0], [-40.0, -40.0], STYLE);
        assert_eq!(raster.take_dirty(), None);
    }
}

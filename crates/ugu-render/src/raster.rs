// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! What every CPU drawing shares: the SIMD level and pixel rectangles.

/// Pixel rectangle: left, top, right, bottom (exclusive).
pub type PixelRect = [u32; 4];

/// The SIMD level every document is drawn with, so that all CPUs give the
/// same pixels. Vello's AVX2 path fuses multiply-adds (FMA), which rounds
/// gradient colours differently; SSE4.2, which Windows 11 requires, has no FMA
/// and draws the same bytes as plain SSE2. It costs no measured speed. ARM's
/// one level is NEON, whose fused multiply-adds `third_party/fearless_simd`
/// takes out for the same reason.
pub fn document_level() -> vello_cpu::Level {
    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    if let Some(sse) = vello_cpu::Level::new().as_sse4_2() {
        return vello_cpu::Level::Sse4_2(sse);
    }
    vello_cpu::Level::baseline()
}

/// A Vello context drawing with `threads` workers. Vello gives one worker
/// a thread of its own and copies every path to it; drawing on the calling
/// thread takes as long and does not keep the copies (④ 192 → 122MiB).
pub fn context(level: vello_cpu::Level, threads: u16) -> vello_cpu::RenderContext {
    vello_cpu::RenderContext::new_with(
        1,
        1,
        vello_cpu::RenderSettings {
            level,
            num_threads: if threads == 1 { 0 } else { threads },
        },
    )
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use sha2::{Digest, Sha256};
    use ugu_core::document::{Document, LayerKind};
    use ugu_core::ops::{Op, Rgba8, StrokeId, Wobble};
    use ugu_core::store::{Brush, BrushEngine, Point, Stroke};
    use vello_cpu::color::AlphaColor;
    use vello_cpu::kurbo::BezPath;
    use vello_cpu::{Pixmap, RasterizerSettings, RenderContext, RenderSettings, Resources};

    use crate::document::{DocumentRenderer, Purpose};

    /// Every SIMD level this CPU has, so that pixels can be compared across
    /// the levels other users' CPUs would pick.
    fn levels() -> Vec<(&'static str, vello_cpu::Level)> {
        let levels = vec![("baseline", vello_cpu::Level::baseline())];
        #[cfg(target_arch = "x86_64")]
        let levels = {
            let detected = vello_cpu::Level::new();
            let mut levels = levels;
            if let Some(sse) = detected.as_sse4_2() {
                levels.push(("sse4.2", vello_cpu::Level::Sse4_2(sse)));
            }
            if let Some(avx) = detected.as_avx2() {
                levels.push(("avx2", vello_cpu::Level::Avx2(avx)));
            }
            if let Some(avx) = detected.as_avx512() {
                levels.push(("avx512", vello_cpu::Level::Avx512(avx)));
            }
            levels
        };
        levels
    }

    /// Translucent strokes of uneven pressure, an eraser and a translucent
    /// layer, so that coverage, compositing and erasing all take part.
    fn document() -> Document {
        let mut document = Document::new([128, 128]);
        document.wobble = Wobble::classic(2.5);
        let LayerKind::Paint(paint) = &mut document.layers[0].kind else {
            unreachable!()
        };
        paint.opacity = 0.7;
        for index in 0..4u32 {
            let points: Vec<Point> = (0..60)
                .map(|step| {
                    let angle = step as f32 * 0.37 + index as f32;
                    Point {
                        x: 64.0 + angle.cos() * (10.0 + step as f32),
                        y: 64.0 + angle.sin() * 40.0,
                        pressure: 0.2 + (step % 7) as f32 / 9.0,
                    }
                })
                .collect();
            document.store.strokes.insert(
                StrokeId(index),
                Stroke {
                    points: Arc::from(points),
                    color: Rgba8([200, 40 + index as u8 * 50, 30, 150]),
                    width: 1.3 + index as f32 * 5.0,
                    brush: Brush {
                        engine: BrushEngine::Line,
                        opacity: 0.9,
                        hardness: 1.0,
                        antialias: index != 1,
                        size_dynamics: 0.8,
                        wobble_scale: 1.0,
                        ..Brush::default()
                    },
                    seed: u64::from(index),
                },
            );
            let stroke = StrokeId(index);
            paint.ops.push(if index == 2 {
                Op::Erase { stroke, clip: None }
            } else {
                Op::Paint { stroke, clip: None }
            });
        }
        document
    }

    #[test]
    fn every_simd_level_draws_the_same_pixels() {
        let document = document();
        let render = |level| {
            let mut pixmap = Pixmap::new(128, 128);
            DocumentRenderer::with_level(0, level).render(
                &document,
                3,
                Purpose::Display,
                &mut pixmap,
            );
            pixmap.data_as_u8_slice().to_vec()
        };
        let levels = levels();
        let (reference_name, reference_level) = levels[0];
        let reference = render(reference_level);
        for &(name, level) in &levels[1..] {
            assert!(
                render(level) == reference,
                "{name} draws differently from {reference_name}"
            );
        }
    }

    /// One digest for every CPU, written on x86. So many edges at fractional
    /// positions because a fused multiply-add changes only a few coverage
    /// bytes; the regression fixtures and the scene above do not reach one.
    /// Only straight edges and plain arithmetic, so that no OS's libm takes
    /// part.
    #[test]
    fn antialiased_edges_come_out_the_same_on_every_cpu() {
        const X86: &str = "559d2212aeded4487ec733016f27bac0d1fa5f878a459fc4fb538279e50a9649";
        let mut context = RenderContext::new_with(
            256,
            256,
            RenderSettings {
                level: super::document_level(),
                num_threads: 0,
            },
        );
        for index in 0..1000u32 {
            let [x0, y0, x1, y1] = [97, 193, 151, 59]
                .map(|step| f64::from(index * step % 251) + f64::from(index) * 0.013 + 0.37);
            let mut edges = BezPath::new();
            edges.move_to((x0, y0));
            edges.line_to((x1, y1));
            edges.line_to((x1 + 1.7, y1 + 0.9));
            edges.line_to((x0 + 1.7, y0 + 0.9));
            edges.close_path();
            context.set_paint(AlphaColor::from_rgba8(
                (index * 6 % 256) as u8,
                (200 + index * 5 % 56) as u8,
                90,
                (110 + index * 3 % 140) as u8,
            ));
            context.fill_path(&edges);
        }
        context.flush();
        let mut pixmap = Pixmap::new(256, 256);
        context.render_with(
            &mut pixmap,
            &mut Resources::new(),
            RasterizerSettings::default(),
        );
        let digest = Sha256::digest(pixmap.data_as_u8_slice());
        let digest = digest
            .iter()
            .fold(String::new(), |out, byte| out + &format!("{byte:02x}"));
        assert_eq!(digest, X86);
    }
}

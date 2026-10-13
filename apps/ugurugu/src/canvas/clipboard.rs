// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! The canvas's side of copy and paste: what is copied, and showing what
//! is pasted or placed.

use std::sync::Arc;

use ugu_core::clip::Clip;
use ugu_core::document::{Document, LayerId};
use ugu_core::ops::AssetId;
use ugu_core::store::Asset;
use ugu_render::document::DocumentRenderer;
use ugu_render::plan::{Reference, RenderPlan};
use ugu_session::FillError;
use ugu_win::clipboard::Image;
use vello_cpu::Pixmap;

use super::Canvas;

/// What other apps get of a copy, drawn off the render thread: the copied
/// layer's own pixels at the frame shown, cut to the selection.
pub struct CopiedPixels {
    document: Arc<Document>,
    layer: LayerId,
    frame: i64,
    clip: Arc<Clip>,
}

impl CopiedPixels {
    /// Draws them, cut to the selection's bounds, as straight RGBA.
    pub fn image(&self) -> Image {
        let plan = RenderPlan::reference(&self.document, Reference::Layer(self.layer));
        let [canvas_width, canvas_height] = self.document.canvas.map(|edge| edge as u16);
        let mut pixels = Pixmap::new(canvas_width, canvas_height);
        DocumentRenderer::new(crate::cache::worker_threads()).render_plan(
            &self.document,
            &plan,
            self.frame,
            None,
            &mut pixels,
        );
        let mask = self.clip.selection.mask();
        let [left, top, width, height] = mask.bounds;
        let source = pixels.data_as_u8_slice().as_chunks::<4>().0;
        let mut straight = Vec::with_capacity(width as usize * height as usize * 4);
        for y in top..top + height {
            for x in left..left + width {
                let pixel = if mask.contains(x, y) {
                    source[y as usize * usize::from(canvas_width) + x as usize]
                } else {
                    [0; 4]
                };
                straight.extend(ugu_io::image::unpremultiply(pixel));
            }
        }
        Image {
            size: [width as u32, height as u32],
            straight,
        }
    }
}

impl Canvas {
    /// Copies the selected part of the current layer; returns the copy and
    /// what other apps get of it. `None` when there is nothing to copy.
    pub fn copy(&mut self) -> Option<(Arc<Clip>, CopiedPixels)> {
        let started = std::time::Instant::now();
        self.notice = None;
        let clip = match self.edit(|session| session.copy()) {
            Ok(clip) => clip,
            Err(FillError::NoSelection) => return None,
            Err(error) => {
                self.fill_notice(&error);
                return None;
            }
        };
        let pixels = CopiedPixels {
            document: self.snapshot().document,
            layer: self.tab.session.current_layer(),
            frame: self.tab.session.frame(),
            clip: clip.clone(),
        };
        tracing::debug!(
            ms = started.elapsed().as_secs_f64() * 1000.0,
            "selection copied"
        );
        Some((clip, pixels))
    }

    /// Pastes `clip` as a new layer, ready to move.
    pub fn paste(&mut self, clip: &Clip) {
        let started = std::time::Instant::now();
        self.notice = None;
        if let Err(error) = self.edit(|session| session.paste(clip)) {
            self.notice = Some(format!("{} {error}", crate::i18n::tr("paste-failed")));
        }
        tracing::debug!(ms = started.elapsed().as_secs_f64() * 1000.0, "clip pasted");
    }

    /// Places `asset` as a new layer, ready to move.
    pub fn place_image(&mut self, id: AssetId, asset: Asset) {
        self.notice = None;
        if let Err(error) = self.edit(|session| session.place_image(id, asset)) {
            self.notice = Some(format!(
                "{}: {error}",
                crate::i18n::tr("insert-image-failed")
            ));
        }
    }

    pub fn set_notice(&mut self, notice: String) {
        self.notice = Some(notice);
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use ugu_session::Session;

    use super::super::Canvas;

    /// How long copying everything holds the render thread, on the document
    /// at `UGU_COPY_FIXTURE` (`ugu-doc fixture 4`), without and with a split,
    /// and how long the clipboard thread then draws it: `cargo test
    /// --release -p ugurugu copy_cost -- --ignored --nocapture`.
    #[test]
    #[ignore = "measurement"]
    fn copy_cost_on_a_large_document() {
        let path = std::env::var("UGU_COPY_FIXTURE").expect("UGU_COPY_FIXTURE");
        let file = std::fs::File::open(path).unwrap();
        let (document, _) = ugu_io::read::read(std::io::BufReader::new(file)).unwrap();
        let (to_test, renders) = std::sync::mpsc::channel();
        let mut canvas = Canvas::new(document, move |rendered| {
            let _ = to_test.send(rendered);
        });
        canvas.edit(Session::select_all).unwrap();
        let times = |canvas: &mut Canvas| {
            let mut times: Vec<f64> = (0..10)
                .map(|_| {
                    let started = Instant::now();
                    assert!(canvas.copy().is_some());
                    started.elapsed().as_secs_f64() * 1000.0
                })
                .collect();
            times.sort_by(f64::total_cmp);
            format!("p50 {:.1}ms, max {:.1}ms", times[5], times[9])
        };
        let without = times(&mut canvas);
        canvas.sync();
        while canvas.tab.split.as_ref().map(|(key, _)| *key) != Some(canvas.key()) {
            let rendered = renders.recv_timeout(Duration::from_secs(60)).unwrap();
            canvas.adopt(rendered);
        }
        let with = times(&mut canvas);
        let (_, pixels) = canvas.copy().unwrap();
        let mut drawn: Vec<f64> = (0..10)
            .map(|_| {
                let started = Instant::now();
                pixels.image();
                started.elapsed().as_secs_f64() * 1000.0
            })
            .collect();
        drawn.sort_by(f64::total_cmp);
        println!(
            "copy without a split: {without}; with one: {with}; drawn off it: p50 {:.1}ms, max {:.1}ms",
            drawn[5], drawn[9]
        );
    }
}

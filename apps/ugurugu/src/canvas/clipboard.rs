// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! The canvas's side of copy and paste: what is copied, and showing what
//! is pasted or placed.

use std::sync::Arc;

use ugu_core::clip::Clip;
use ugu_core::ops::AssetId;
use ugu_core::store::Asset;
use ugu_render::plan::{Reference, RenderPlan};
use ugu_session::FillError;
use ugu_win::clipboard::Image;
use vello_cpu::Pixmap;

use super::Canvas;

impl Canvas {
    /// Copies the selected part of the current layer; returns the copy and
    /// its pixels at this frame, cut to the selection's bounds, for other
    /// apps. `None` when there is nothing to copy.
    pub fn copy(&mut self) -> Option<(Arc<Clip>, Image)> {
        self.notice = None;
        let clip = match self.edit(|session| session.copy()) {
            Ok(clip) => clip,
            Err(FillError::NoSelection) => return None,
            Err(error) => {
                self.fill_notice(&error);
                return None;
            }
        };
        let pixels = self.layer_pixels()?;
        let mask = clip.selection.mask();
        let [left, top, width, height] = mask.bounds;
        let canvas_width = usize::from(pixels.width());
        let source = pixels.data_as_u8_slice().as_chunks::<4>().0;
        let mut straight = Vec::with_capacity(width as usize * height as usize * 4);
        for y in top..top + height {
            for x in left..left + width {
                let pixel = if mask.contains(x, y) {
                    source[y as usize * canvas_width + x as usize]
                } else {
                    [0; 4]
                };
                straight.extend(ugu_io::image::unpremultiply(pixel));
            }
        }
        Some((
            clip,
            Image {
                size: [width as u32, height as u32],
                straight,
            },
        ))
    }

    /// The current layer's own pixels at this frame: put together from the
    /// split when it shows this state, else drawn by the worker.
    fn layer_pixels(&mut self) -> Option<Pixmap> {
        let document = self.session.document();
        let plan = RenderPlan::reference(document, Reference::Layer(self.session.current_layer()));
        let [width, height] = document.canvas.map(|edge| edge as u16);
        let mut pixels = Pixmap::new(width, height);
        let key = self.key();
        let threads = std::thread::available_parallelism().map_or(1, usize::from);
        let from_split = self.split.as_ref().is_some_and(|(shown, split)| {
            *shown == key && split.reference(&plan, &mut pixels, threads)
        });
        if from_split {
            return Some(pixels);
        }
        let document = self.snapshot().document;
        self.cache
            .renders()
            .render(document, plan, self.session.frame())
    }

    /// Pastes `clip` as a new layer, ready to move.
    pub fn paste(&mut self, clip: &Clip) {
        self.notice = None;
        if let Err(error) = self.edit(|session| session.paste(clip)) {
            self.notice = Some(format!("{} {error}", crate::i18n::tr("paste-failed")));
        }
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

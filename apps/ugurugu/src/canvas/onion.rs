// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! Onion skin: the frames around the one shown of the animation folder
//! being edited, drawn faintly over the canvas, those before in red and
//! those after in blue. It is only shown, never exported or read by tools.

use std::sync::Arc;

use ugu_core::document::LayerId;
use ugu_render::compose::src_over;
use ugu_render::raster::PixelRect;
use vello_cpu::Pixmap;

use super::{Canvas, Interaction};
use crate::cache::Version;

/// Frames before and after the one shown, and how strongly they show.
pub const BEFORE: u32 = 1;
pub const AFTER: u32 = 1;
pub const OPACITY: f32 = 0.3;
const RED: [u8; 3] = [230, 40, 40];
const BLUE: [u8; 3] = [40, 90, 235];

/// What an onion skin is drawn from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OnionKey {
    pub version: Version,
    pub folder: LayerId,
    /// The frame shown, within the cycle.
    pub frame: u32,
}

#[derive(Default)]
pub struct Onion {
    /// The frames tinted and put together over transparency, and what
    /// they show.
    pixels: Option<(OnionKey, Arc<Pixmap>)>,
    asked: Option<OnionKey>,
    /// What the display should show now; `None` for no onion skin.
    wanted: Option<OnionKey>,
}

impl Onion {
    /// Puts the onion skin over `rect` of `display`, when it is of the
    /// state shown.
    pub fn cover(&self, display: &mut Pixmap, rect: PixelRect) {
        let Some((key, pixels)) = &self.pixels else {
            return;
        };
        if self.wanted != Some(*key) {
            return;
        }
        let width = usize::from(display.width());
        let [left, top, right, bottom] = rect.map(|edge| edge as usize);
        let from = pixels.data_as_u8_slice().as_chunks::<4>().0;
        let to = display.data_as_u8_slice_mut().as_chunks_mut::<4>().0;
        for y in top..bottom {
            for x in left..right {
                let pixel = from[y * width + x];
                if pixel[3] != 0 {
                    src_over(&mut to[y * width + x], &pixel);
                }
            }
        }
    }
}

/// `frames`, premultiplied over transparency, tinted and put together:
/// each frame `before` it red, the rest blue, the nearest on top.
pub fn tint(frames: &[(Pixmap, bool)], size: [u32; 2]) -> Pixmap {
    let mut out = Pixmap::new(size[0] as u16, size[1] as u16);
    let target = out.data_as_u8_slice_mut().as_chunks_mut::<4>().0;
    for (frame, before) in frames {
        let [r, g, b] = if *before { RED } else { BLUE };
        let source = frame.data_as_u8_slice().as_chunks::<4>().0;
        for (to, from) in target.iter_mut().zip(source) {
            if from[3] == 0 {
                continue;
            }
            let alpha = (f32::from(from[3]) * OPACITY).round();
            let channel = |value: u8| (f32::from(value) * alpha / 255.0).round() as u8;
            let tinted = [channel(r), channel(g), channel(b), alpha as u8];
            src_over(to, &tinted);
        }
    }
    out
}

/// The frames an onion skin around `frame` of a loop of `frames` shows,
/// farthest first, with whether each is before it. A frame reached both
/// ways round a short loop shows once, as the nearer, and before when as
/// near.
pub fn around(frame: u32, frames: u32) -> Vec<(i64, bool)> {
    let frame = i64::from(frame);
    let frames = i64::from(frames);
    let mut seen = vec![frame.rem_euclid(frames)];
    let mut wanted = Vec::new();
    for step in 1..=i64::from(BEFORE.max(AFTER)) {
        for (at, before, within) in [
            (frame - step, true, step <= i64::from(BEFORE)),
            (frame + step, false, step <= i64::from(AFTER)),
        ] {
            if within && !seen.contains(&at.rem_euclid(frames)) {
                seen.push(at.rem_euclid(frames));
                wanted.push((at, before));
            }
        }
    }
    wanted.reverse();
    wanted
}

impl Canvas {
    pub fn onion_skin(&self) -> bool {
        self.onion_skin
    }

    pub fn set_onion_skin(&mut self, on: bool) {
        self.onion_skin = on;
        self.sync_onion();
    }

    /// The onion skin wanted now: on, not playing, and editing a folder or
    /// one of its cels.
    fn onion_wanted(&self) -> Option<OnionKey> {
        if !self.onion_skin || self.tab.playback.is_some() {
            return None;
        }
        let session = &self.tab.session;
        Some(OnionKey {
            version: self.version(),
            folder: session.folder()?,
            frame: session.cycle(),
        })
    }

    /// Asks for the onion skin wanted, and shows it or takes it away.
    pub(super) fn sync_onion(&mut self) {
        let wanted = self.onion_wanted();
        let before = self.tab.onion.wanted;
        self.tab.onion.wanted = wanted;
        if let Some(key) = wanted
            && self.tab.onion.pixels.as_ref().map(|(shown, _)| *shown) != Some(key)
            && self.tab.onion.asked != Some(key)
        {
            let snapshot = self.snapshot();
            let frames = around(key.frame, snapshot.document.frames);
            self.tab.cache.request_onion(key, frames, snapshot.document);
            self.tab.onion.asked = Some(key);
        }
        let shown = |wanted: Option<OnionKey>| {
            wanted.filter(|key| {
                self.tab
                    .onion
                    .pixels
                    .as_ref()
                    .is_some_and(|(shown, _)| shown == key)
            })
        };
        if shown(before) != shown(wanted) {
            self.redisplay();
        }
    }

    /// Takes an onion skin drawn for `key`.
    pub(super) fn adopt_onion(&mut self, key: OnionKey, pixels: Pixmap) {
        if self.tab.onion.asked == Some(key) {
            self.tab.onion.asked = None;
        }
        if self.onion_wanted() != Some(key) {
            return;
        }
        self.tab.onion.pixels = Some((key, Arc::new(pixels)));
        self.tab.onion.wanted = Some(key);
        self.redisplay();
    }

    /// Puts the whole display back together from the split, with the
    /// stroke being drawn and the onion skin.
    fn redisplay(&mut self) {
        let Some((_, split)) = &self.tab.split else {
            return;
        };
        let [width, height] = self.tab.session.document().canvas;
        if [
            u32::from(self.tab.display.width()),
            u32::from(self.tab.display.height()),
        ] != [width, height]
        {
            return;
        }
        let whole = [0, 0, width, height];
        let live = match &self.interaction {
            Interaction::Drawing { live, .. } => Some(live.as_ref()),
            _ => None,
        };
        ugu_render::compose::composite(split, live, whole, &mut self.tab.display);
        self.tab.onion.cover(&mut self.tab.display, whole);
        self.upload_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_frames_around_come_farthest_first_and_never_the_frame_itself() {
        assert_eq!(around(5, 30), [(6, false), (4, true)]);
        // In a loop of two, the other frame is before and after: once.
        assert_eq!(around(0, 2), [(-1, true)]);
    }

    #[test]
    fn frames_before_are_red_and_after_blue_at_the_onion_opacity() {
        let mut opaque = Pixmap::new(1, 1);
        opaque.data_as_u8_slice_mut().copy_from_slice(&[255; 4]);
        let red = tint(&[(opaque.clone(), true)], [1, 1]);
        let pixel = red.data_as_u8_slice();
        assert_eq!(pixel[3], (255.0 * OPACITY).round() as u8);
        assert!(pixel[0] > pixel[2]);
        let blue = tint(&[(opaque, false)], [1, 1]);
        assert!(blue.data_as_u8_slice()[2] > blue.data_as_u8_slice()[0]);
    }
}

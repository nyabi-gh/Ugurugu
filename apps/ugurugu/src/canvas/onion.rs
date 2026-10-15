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

const RED: [u8; 3] = [230, 40, 40];
const BLUE: [u8; 3] = [40, 90, 235];

/// How many frames before and after the one shown, and how strongly they
/// show.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OnionSettings {
    pub before: u32,
    pub after: u32,
    /// In percent.
    pub opacity: u32,
}

impl OnionSettings {
    pub const DEFAULT: Self = Self {
        before: 1,
        after: 1,
        opacity: 30,
    };
    pub const FRAMES: std::ops::RangeInclusive<u32> = 0..=4;
    pub const OPACITY: std::ops::RangeInclusive<u32> = 10..=80;

    /// Brought into the ranges above.
    pub fn clamped(self) -> Self {
        let frames = |count: u32| count.clamp(*Self::FRAMES.start(), *Self::FRAMES.end());
        Self {
            before: frames(self.before),
            after: frames(self.after),
            opacity: self
                .opacity
                .clamp(*Self::OPACITY.start(), *Self::OPACITY.end()),
        }
    }
}

impl Default for OnionSettings {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// What an onion skin is drawn from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OnionKey {
    pub version: Version,
    pub folder: LayerId,
    /// The frame shown, within the cycle.
    pub frame: u32,
    pub settings: OnionSettings,
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

/// `frames`, premultiplied over transparency, tinted and put together at
/// `opacity` percent: each frame `before` it red, the rest blue, the
/// nearest on top.
pub fn tint(frames: &[(&Pixmap, bool)], size: [u32; 2], opacity: u32) -> Pixmap {
    let opacity = opacity as f32 / 100.0;
    // A tinted pixel depends on its alpha alone.
    let table = |[r, g, b]: [u8; 3]| -> [[u8; 4]; 256] {
        std::array::from_fn(|from| {
            let alpha = (from as f32 * opacity).round();
            let channel = |value: u8| (f32::from(value) * alpha / 255.0).round() as u8;
            [channel(r), channel(g), channel(b), alpha as u8]
        })
    };
    let (red, blue) = (table(RED), table(BLUE));
    let mut out = Pixmap::new(size[0] as u16, size[1] as u16);
    let width = size[0] as usize;
    let target = out.data_as_u8_slice_mut().as_chunks_mut::<4>().0;
    let threads = std::thread::available_parallelism().map_or(1, |count| count.get().min(8));
    let rows = (size[1] as usize).div_ceil(threads).max(1);
    std::thread::scope(|scope| {
        for (index, part) in target.chunks_mut(rows * width).enumerate() {
            let (red, blue) = (&red, &blue);
            scope.spawn(move || {
                let start = index * rows * width;
                for (frame, before) in frames {
                    let table = if *before { red } else { blue };
                    let source = &frame.data_as_u8_slice().as_chunks::<4>().0[start..];
                    for (to, from) in part.iter_mut().zip(source) {
                        if from[3] != 0 {
                            src_over(to, &table[usize::from(from[3])]);
                        }
                    }
                }
            });
        }
    });
    out
}

/// The frames an onion skin around `frame` of a loop of `frames` shows,
/// farthest first, with whether each is before it. A frame reached both
/// ways round a short loop shows once, as the nearer, and before when as
/// near.
pub fn around(frame: u32, frames: u32, settings: OnionSettings) -> Vec<(i64, bool)> {
    let frame = i64::from(frame);
    let frames = i64::from(frames);
    let (before, after) = (i64::from(settings.before), i64::from(settings.after));
    let mut seen = vec![frame.rem_euclid(frames)];
    let mut wanted = Vec::new();
    for step in 1..=before.max(after) {
        for (at, before, within) in [
            (frame - step, true, step <= before),
            (frame + step, false, step <= after),
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

    #[cfg(test)]
    pub fn onion_settings(&self) -> OnionSettings {
        self.onion_settings
    }

    pub fn set_onion_settings(&mut self, settings: OnionSettings) {
        let settings = settings.clamped();
        if settings != self.onion_settings {
            self.onion_settings = settings;
            self.sync_onion();
        }
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
            settings: self.onion_settings,
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
            let frames = around(key.frame, snapshot.document.frames, key.settings);
            self.tab.cache.request_onion(key, frames, snapshot);
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
        let one = OnionSettings::DEFAULT;
        assert_eq!(around(5, 30, one), [(6, false), (4, true)]);
        // In a loop of two, the other frame is before and after: once.
        assert_eq!(around(0, 2, one), [(-1, true)]);
        let more = OnionSettings {
            before: 2,
            after: 0,
            ..one
        };
        assert_eq!(around(5, 30, more), [(3, true), (4, true)]);
    }

    #[test]
    fn frames_before_are_red_and_after_blue_at_the_onion_opacity() {
        let mut opaque = Pixmap::new(1, 1);
        opaque.data_as_u8_slice_mut().copy_from_slice(&[255; 4]);
        let red = tint(&[(&opaque, true)], [1, 1], 30);
        let pixel = red.data_as_u8_slice();
        assert_eq!(pixel[3], (255.0 * 0.3_f32).round() as u8);
        assert!(pixel[0] > pixel[2]);
        let blue = tint(&[(&opaque, false)], [1, 1], 30);
        assert!(blue.data_as_u8_slice()[2] > blue.data_as_u8_slice()[0]);
    }
}

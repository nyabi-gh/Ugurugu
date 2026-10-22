// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! Placed text shown on the edited layer before it is in the document: its
//! fill and strokes are stamped as committing them would stamp them, over
//! the layer as it was when the text was placed, and stamped again where
//! they were when the text moves or changes.

use std::sync::Arc;

use ugu_core::ops::Wobble;
use ugu_core::store::{Mask, Stroke};

use crate::compose::{Split, Stamp};
use crate::raster::PixelRect;
use crate::stream::Held;
use crate::tile::TiledSurface;

pub struct Placing {
    original: Arc<TiledSurface>,
    /// The split's `reached` and `redraw` with `original`.
    reached: (Option<[u32; 4]>, bool),
    /// And with the text shown now, which they become once it is applied.
    with_text: (Option<[u32; 4]>, bool),
    /// What differs from `original` now.
    shown: Option<PixelRect>,
}

/// A fill: its coverage, whether its edge is soft, and its straight colour.
pub type Fill<'a> = (&'a Mask, bool, [u8; 4]);

fn union(a: Option<PixelRect>, b: Option<PixelRect>) -> Option<PixelRect> {
    match (a, b) {
        (Some(a), Some(b)) => Some([
            a[0].min(b[0]),
            a[1].min(b[1]),
            a[2].max(b[2]),
            a[3].max(b[3]),
        ]),
        (a, None) => a,
        (None, b) => b,
    }
}

impl Split {
    /// Starts showing placed text; `None` when the edited layer is not
    /// shown.
    pub fn begin_place(&self) -> Option<Placing> {
        let Held::Tiles(surface) = &self.sources[self.edited?] else {
            unreachable!("a paint layer's pixels are tiles");
        };
        Some(Placing {
            original: surface.clone(),
            reached: (self.reached, self.redraw),
            with_text: (self.reached, self.redraw),
            shown: None,
        })
    }

    /// Shows the edited layer with `fill` and then `strokes` added, cut to
    /// `clip`. Returns the pixels it changed.
    #[allow(clippy::too_many_arguments)]
    pub fn show_place(
        &mut self,
        placing: &mut Placing,
        stamp: &mut Stamp,
        fill: Option<Fill<'_>>,
        strokes: &[Stroke],
        wobble: Wobble,
        frames: u32,
        clip: Option<&Mask>,
    ) -> Option<PixelRect> {
        let before = placing.shown.take();
        if let Some(rect) = before {
            self.restore(&placing.original, rect);
        }
        (self.reached, self.redraw) = placing.reached;
        let mut now = None;
        if let Some((coverage, antialias, color)) = fill {
            now = union(
                now,
                self.stamp_fill(coverage, clip, antialias, color, false),
            );
        }
        for stroke in strokes {
            now = union(now, self.stamp(stamp, stroke, false, wobble, frames, clip));
        }
        // Placed text is not in the document, so a split drawn again would
        // come back without it and be marked again: it widens what the
        // layer reaches only once applied.
        placing.with_text = (self.reached, self.redraw);
        (self.reached, self.redraw) = placing.reached;
        placing.shown = now;
        union(before, now)
    }

    /// Ends showing placed text: when it is applied the shown pixels stay,
    /// otherwise the layer as it was comes back. Returns the pixels changed.
    pub fn end_place(&mut self, placing: Placing, applied: bool) -> Option<PixelRect> {
        if applied {
            (self.reached, self.redraw) = placing.with_text;
            return None;
        }
        let edited = self.edited?;
        self.sources[edited] = Held::Tiles(placing.original);
        (self.reached, self.redraw) = placing.reached;
        placing.shown
    }

    /// Puts `rect` of the edited layer back as it is in `original`.
    fn restore(&mut self, original: &TiledSurface, rect: PixelRect) {
        let Some(edited) = self.edited else {
            return;
        };
        let Held::Tiles(surface) = &mut self.sources[edited] else {
            unreachable!("a paint layer's pixels are tiles");
        };
        let surface = Arc::make_mut(surface);
        surface.ensure(rect);
        let [left, _, right, _] = rect;
        for (y, line) in surface.rows_mut(rect) {
            line.fill([0; 4]);
            for (x, part) in original.row(y, left, right) {
                let from = (x - left) as usize;
                line[from..from + part.len()].copy_from_slice(part);
            }
        }
    }
}

// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! The built-in brushes and erasers, with 2.2.13's values
//! (`BrushPresetCatalog`, `EraserPresetCatalog`), and 3.0's pixel pencil and
//! eraser.

use crate::store::{Brush, BrushEngine, TipShape};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Category {
    Pen,
    Marker,
    Airbrush,
    Spray,
    Pixel,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Preset {
    /// Stable, for settings and translations.
    pub id: &'static str,
    pub category: Category,
    pub brush: Brush,
    pub size: f32,
}

const fn line(opacity: f32, size_dynamics: f32, tip: TipShape) -> Brush {
    Brush {
        engine: BrushEngine::Line,
        tip,
        opacity,
        size_dynamics,
        ..Brush::DEFAULT
    }
}

const fn airbrush(
    opacity: f32,
    flow: f32,
    hardness: f32,
    spacing: f32,
    size_dynamics: f32,
    opacity_dynamics: f32,
) -> Brush {
    Brush {
        engine: BrushEngine::Airbrush,
        opacity,
        flow,
        hardness,
        spacing,
        size_dynamics,
        opacity_dynamics,
        ..Brush::DEFAULT
    }
}

#[allow(clippy::too_many_arguments)]
const fn spray(
    tip: TipShape,
    opacity: f32,
    flow: f32,
    spacing: f32,
    scatter: f32,
    particle_size: f32,
    density: f32,
    size_dynamics: f32,
    opacity_dynamics: f32,
    size_jitter: f32,
    animated_jitter: bool,
) -> Brush {
    Brush {
        engine: BrushEngine::Spray,
        tip,
        opacity,
        flow,
        spacing,
        scatter,
        particle_size,
        density,
        size_dynamics,
        opacity_dynamics,
        size_jitter,
        animated_jitter,
        ..Brush::DEFAULT
    }
}

/// A pixel pencil: full opacity and the same width at any pressure.
const PIXEL: Brush = Brush {
    engine: BrushEngine::Pixel,
    tip: TipShape::Square,
    size_dynamics: 0.0,
    opacity_dynamics: 0.0,
    antialias: false,
    ..Brush::DEFAULT
};

const fn preset(id: &'static str, category: Category, brush: Brush, size: f32) -> Preset {
    Preset {
        id,
        category,
        brush,
        size,
    }
}

use Category::{Airbrush, Marker, Pen, Pixel, Spray};
use TipShape::{Round, Square};

pub const BRUSHES: [Preset; 18] = [
    preset("ink-pen", Pen, line(1.0, 0.8, Round), 6.0),
    preset("g-pen", Pen, line(1.0, 0.95, Round), 7.0),
    preset("round-pen", Pen, line(1.0, 0.6, Round), 8.0),
    preset("monoline", Pen, line(1.0, 0.0, Round), 6.0),
    preset("bold-ink", Pen, line(1.0, 0.35, Round), 16.0),
    preset("opaque-marker", Marker, line(0.92, 0.12, Square), 20.0),
    preset("transparent-marker", Marker, line(0.38, 0.05, Square), 28.0),
    preset("highlighter", Marker, line(0.22, 0.0, Square), 36.0),
    preset(
        "soft-airbrush",
        Airbrush,
        airbrush(0.9, 0.11, 0.0, 0.09, 0.15, 0.75),
        64.0,
    ),
    preset(
        "hard-airbrush",
        Airbrush,
        airbrush(0.95, 0.18, 0.72, 0.12, 0.25, 0.55),
        48.0,
    ),
    preset(
        "dense-airbrush",
        Airbrush,
        airbrush(1.0, 0.28, 0.35, 0.08, 0.2, 0.45),
        44.0,
    ),
    preset(
        "fine-mist",
        Airbrush,
        airbrush(0.75, 0.07, 0.12, 0.07, 0.05, 0.85),
        34.0,
    ),
    preset(
        "pixel-spray",
        Spray,
        spray(
            Square, 1.0, 0.55, 0.13, 0.9, 0.06, 1.4, 0.15, 0.65, 0.4, false,
        ),
        44.0,
    ),
    preset(
        "rough-spray",
        Spray,
        spray(
            Round, 0.9, 0.38, 0.14, 1.1, 0.11, 1.0, 0.2, 0.55, 0.85, false,
        ),
        58.0,
    ),
    preset(
        "dust-spray",
        Spray,
        spray(
            Square, 0.8, 0.28, 0.1, 1.25, 0.035, 2.4, 0.05, 0.8, 0.6, false,
        ),
        52.0,
    ),
    preset(
        "droplet-spray",
        Spray,
        spray(
            Round, 1.0, 0.68, 0.2, 1.2, 0.22, 0.35, 0.3, 0.35, 0.75, false,
        ),
        72.0,
    ),
    preset(
        "wobble-spray",
        Spray,
        spray(
            Square, 0.95, 0.48, 0.13, 1.0, 0.075, 1.2, 0.15, 0.65, 0.55, true,
        ),
        48.0,
    ),
    preset("pixel-pencil", Pixel, PIXEL, 1.0),
];

/// Hard, soft, kneaded and pixel; their category is that of their engine.
pub const ERASERS: [Preset; 4] = [
    preset(
        "hard-eraser",
        Pen,
        Brush {
            antialias: true,
            ..line(1.0, 0.8, Round)
        },
        6.0,
    ),
    preset(
        "soft-eraser",
        Airbrush,
        Brush {
            antialias: true,
            ..airbrush(0.9, 0.16, 0.0, 0.08, 0.25, 0.65)
        },
        64.0,
    ),
    preset(
        "kneaded-eraser",
        Spray,
        spray(
            Round, 0.72, 0.42, 0.1, 0.55, 0.13, 1.2, 0.3, 0.55, 0.65, false,
        ),
        48.0,
    ),
    preset("pixel-eraser", Pixel, PIXEL, 1.0),
];

/// The brush or eraser with `id`.
pub fn find(id: &str) -> Option<&'static Preset> {
    BRUSHES
        .iter()
        .chain(&ERASERS)
        .find(|preset| preset.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::brush_in_range;

    #[test]
    fn every_preset_is_in_range_and_named_once() {
        let mut ids: Vec<_> = BRUSHES
            .iter()
            .chain(&ERASERS)
            .map(|preset| preset.id)
            .collect();
        assert!(
            BRUSHES
                .iter()
                .chain(&ERASERS)
                .all(|preset| brush_in_range(&preset.brush))
        );
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), BRUSHES.len() + ERASERS.len());
        assert_eq!(find("wobble-spray").map(|preset| preset.size), Some(48.0));
    }
}

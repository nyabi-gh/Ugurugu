// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! What the window keeps for every document it shows and between runs: the
//! tools' settings and the colours used. A session is handed them when an
//! edit starts.

use std::collections::HashMap;

use ugu_core::brush::{self, Preset};
use ugu_core::ops::{Rgba8, Sampling};
use ugu_core::store::BrushEngine;

use crate::{FillSettings, ShapeKind, TextSettings, Tool, ToolSettings};

/// The colours strokes, fills and text were made with, newest first and each
/// once, as 2.2.13's colour history.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ColorHistory(Vec<Rgba8>);

impl ColorHistory {
    /// 2.2.13's.
    pub const CAPACITY: usize = 256;

    /// The first `CAPACITY` distinct colours of `colors`, in order.
    pub fn from_colors(colors: impl IntoIterator<Item = Rgba8>) -> Self {
        let mut kept: Vec<Rgba8> = Vec::new();
        for color in colors {
            if kept.len() == Self::CAPACITY {
                break;
            }
            if !kept.contains(&color) {
                kept.push(color);
            }
        }
        Self(kept)
    }

    pub fn colors(&self) -> &[Rgba8] {
        &self.0
    }

    /// Puts `color` first, moving it there if it was further down.
    pub fn record(&mut self, color: Rgba8) {
        self.0.retain(|each| *each != color);
        self.0.insert(0, color);
        self.0.truncate(Self::CAPACITY);
    }

    pub fn clear(&mut self) {
        self.0.clear();
    }
}

pub fn is_pixel(preset: &Preset) -> bool {
    preset.brush.engine == BrushEngine::Pixel
}

/// A brush and an eraser, and the width and stabilizer each preset not in
/// use had when another was chosen.
#[derive(Clone, Debug, PartialEq)]
pub struct Brushes {
    pub pen: ToolSettings,
    pub eraser: ToolSettings,
    /// By preset id.
    pub remembered: HashMap<&'static str, (f32, f32)>,
}

impl Brushes {
    /// 2.2.13's on a fresh install.
    pub fn plain() -> Self {
        Self {
            pen: ToolSettings::PEN,
            eraser: ToolSettings::ERASER,
            remembered: HashMap::new(),
        }
    }

    /// The first pixel brush and eraser at their own sizes.
    pub fn pixel() -> Self {
        let first = |presets: &'static [Preset]| {
            presets
                .iter()
                .find(|preset| is_pixel(preset))
                .expect("a pixel preset of each")
        };
        let at_size = |preset: &'static Preset| ToolSettings {
            preset,
            width: preset.size,
            stabilizer: 0.0,
            antialias: preset.brush.antialias,
        };
        Self {
            pen: at_size(first(&brush::BRUSHES)),
            eraser: at_size(first(&brush::ERASERS)),
            remembered: HashMap::new(),
        }
    }

    /// The eraser's settings with `Tool::Eraser`, else the brush's.
    pub fn get(&self, tool: Tool) -> &ToolSettings {
        match tool {
            Tool::Eraser => &self.eraser,
            Tool::Pen | Tool::Select | Tool::Wand | Tool::Fill | Tool::Text | Tool::Eyedropper => {
                &self.pen
            }
        }
    }

    pub fn get_mut(&mut self, tool: Tool) -> &mut ToolSettings {
        match tool {
            Tool::Eraser => &mut self.eraser,
            Tool::Pen | Tool::Select | Tool::Wand | Tool::Fill | Tool::Text | Tool::Eyedropper => {
                &mut self.pen
            }
        }
    }

    /// Makes `preset` the brush, or with `Tool::Eraser` the eraser. Each
    /// preset keeps its own width and stabilizer, starting from its size and
    /// none, as in 2.2.13.
    pub fn choose(&mut self, tool: Tool, preset: &'static Preset) {
        let settings = match tool {
            Tool::Eraser => &mut self.eraser,
            Tool::Pen | Tool::Select | Tool::Wand | Tool::Fill | Tool::Text | Tool::Eyedropper => {
                &mut self.pen
            }
        };
        if settings.preset.id == preset.id {
            return;
        }
        self.remembered
            .insert(settings.preset.id, (settings.width, settings.stabilizer));
        let (width, stabilizer) = self
            .remembered
            .remove(preset.id)
            .unwrap_or((preset.size, 0.0));
        settings.preset = preset;
        settings.width = width;
        settings.stabilizer = stabilizer;
        if tool == Tool::Eraser {
            settings.antialias = preset.brush.antialias;
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Tools {
    pub tool: Tool,
    /// What strokes, fills and text are made with.
    pub color: Rgba8,
    /// The brush and eraser out of pixel mode.
    pub plain: Brushes,
    /// The brush and eraser in pixel mode, pixel presets only, so moving
    /// between documents in and out of the mode changes neither.
    pub pixel: Brushes,
    pub selection_shape: ShapeKind,
    pub lasso_paints: bool,
    pub uses_pressure: bool,
    pub fill: FillSettings,
    pub transform_sampling: Sampling,
    pub text: TextSettings,
    pub colors: ColorHistory,
}

impl Default for Tools {
    /// 2.2.13's on a fresh install.
    fn default() -> Self {
        Self {
            tool: Tool::Pen,
            color: Rgba8([0, 0, 0, 255]),
            plain: Brushes::plain(),
            pixel: Brushes::pixel(),
            selection_shape: ShapeKind::default(),
            lasso_paints: false,
            uses_pressure: true,
            fill: FillSettings::DEFAULT,
            transform_sampling: Sampling::Smooth,
            text: TextSettings::DEFAULT,
            colors: ColorHistory::default(),
        }
    }
}

impl Tools {
    /// The brushes a document in pixel mode or out of it draws with.
    pub fn brushes(&self, pixel_mode: bool) -> &Brushes {
        if pixel_mode { &self.pixel } else { &self.plain }
    }

    pub fn brushes_mut(&mut self, pixel_mode: bool) -> &mut Brushes {
        if pixel_mode {
            &mut self.pixel
        } else {
            &mut self.plain
        }
    }

    /// What the current tool strokes with in such a document.
    pub fn stroke(&self, pixel_mode: bool) -> &ToolSettings {
        self.brushes(pixel_mode).get(self.tool)
    }

    /// Makes `preset` the brush, or with `Tool::Eraser` the eraser, of the
    /// brushes for `pixel_mode`; pixel mode takes pixel presets alone.
    pub fn choose_preset(&mut self, pixel_mode: bool, tool: Tool, preset: &'static Preset) {
        if pixel_mode && !is_pixel(preset) {
            return;
        }
        self.brushes_mut(pixel_mode).choose(tool, preset);
        // Pixel art fills without blending edges too; it can be turned on
        // again. Pixel mode never blends them.
        if !pixel_mode && is_pixel(preset) && tool != Tool::Eraser {
            self.fill.antialias = false;
        }
    }

    /// Makes the colour that of `pixel`, a premultiplied pixel of the frame
    /// shown; a clear pixel leaves it, as in 2.2.13. Returns whether the
    /// colour changed.
    pub fn pick_color(&mut self, pixel: [u8; 4]) -> bool {
        if pixel[3] == 0 {
            return false;
        }
        let color = Rgba8::from_premultiplied(pixel);
        let changed = self.color != color;
        self.color = color;
        changed
    }
}

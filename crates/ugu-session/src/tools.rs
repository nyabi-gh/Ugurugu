// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! What stays from one document to the next and between runs: the tools'
//! settings and the colours used.

use std::collections::HashMap;

use ugu_core::ops::{Rgba8, Sampling};

use crate::{FillSettings, Session, ShapeKind, TextSettings, Tool, ToolSettings};

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

#[derive(Clone, Debug, PartialEq)]
pub struct Tools {
    pub tool: Tool,
    pub pen: ToolSettings,
    pub eraser: ToolSettings,
    /// Width and stabilizer of the presets not in use, by preset id.
    pub remembered: HashMap<&'static str, (f32, f32)>,
    pub selection_shape: ShapeKind,
    pub lasso_paints: bool,
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
            pen: ToolSettings::PEN,
            eraser: ToolSettings::ERASER,
            remembered: HashMap::new(),
            selection_shape: ShapeKind::default(),
            lasso_paints: false,
            fill: FillSettings::DEFAULT,
            transform_sampling: Sampling::Smooth,
            text: TextSettings::DEFAULT,
            colors: ColorHistory::default(),
        }
    }
}

impl Session {
    /// The tools as they are now.
    pub fn tools(&self) -> Tools {
        Tools {
            tool: self.tool,
            pen: self.pen,
            eraser: self.eraser,
            remembered: self.remembered.clone(),
            selection_shape: self.selection_shape,
            lasso_paints: self.lasso_paints,
            fill: self.fill,
            transform_sampling: self.transform_sampling,
            text: self.text.clone(),
            colors: self.colors.clone(),
        }
    }

    /// Takes `tools` over, as from the document before or the settings. A
    /// pending transform or placed text is applied first, as a tool change.
    pub fn set_tools(&mut self, tools: Tools) {
        self.settle();
        let Tools {
            tool,
            pen,
            eraser,
            remembered,
            selection_shape,
            lasso_paints,
            fill,
            transform_sampling,
            text,
            colors,
        } = tools;
        self.tool = tool;
        self.pen = pen;
        self.eraser = eraser;
        self.remembered = remembered;
        self.selection_shape = selection_shape;
        self.lasso_paints = lasso_paints;
        self.fill = fill;
        self.transform_sampling = transform_sampling;
        self.text = text;
        self.colors = colors;
    }
}

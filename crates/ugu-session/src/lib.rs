// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! One open document and what the user is doing with it: the tools, the
//! stroke being drawn, the current layer and frame, and the history that
//! every change goes through.
//!
//! Input reaches the session in document pixels. A stroke is held here until
//! the pen lifts and is then committed as one edit; a cancelled stroke leaves
//! nothing behind.

pub mod stabilizer;

use std::hash::{BuildHasher, RandomState};
use std::sync::Arc;

use ugu_core::command;
use ugu_core::document::{Document, Layer, LayerId, LayerKind, limits};
use ugu_core::edit::{EditError, Outcome};
use ugu_core::history::{History, LayerRevisions, StateId};
use ugu_core::ops::{Rgba8, Wobble};
use ugu_core::store::{self, Brush, BrushEngine, Point, Stroke};

use crate::stabilizer::Stabilizer;

/// Points closer than this to the last kept one are dropped, as in 2.2.13.
const MIN_POINT_DISTANCE: f64 = 0.75;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    Pen,
    Eraser,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ToolSettings {
    /// Ignored by the eraser.
    pub color: Rgba8,
    pub width: f32,
    pub opacity: f32,
    pub size_dynamics: f32,
    pub antialias: bool,
    /// 0 (off) to 1.
    pub stabilizer: f32,
}

impl ToolSettings {
    /// 2.2.13's ink pen: width 6, black, aliased.
    pub const PEN: Self = Self {
        color: Rgba8([0, 0, 0, 255]),
        width: 6.0,
        opacity: 1.0,
        size_dynamics: 0.8,
        antialias: false,
        stabilizer: 0.0,
    };
    /// 2.2.13's hard eraser.
    pub const ERASER: Self = Self {
        color: Rgba8([0, 0, 0, 255]),
        width: 6.0,
        opacity: 1.0,
        size_dynamics: 0.8,
        antialias: true,
        stabilizer: 0.0,
    };
}

/// One pointer sample in document pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct InputPoint {
    pub position: [f64; 2],
    /// `None` for devices without pressure, such as a mouse.
    pub pressure: Option<f32>,
    /// Milliseconds on any steady clock.
    pub time: f64,
}

/// Why a stroke cannot start.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StrokeRefused {
    NoLayer,
    HiddenLayer,
}

/// The stroke being drawn.
#[derive(Clone, Debug)]
pub struct LiveStroke {
    pub layer: LayerId,
    pub erase: bool,
    /// Everything of the stroke but its points.
    pub template: Stroke,
    pub points: Vec<Point>,
    /// Set when the last point was replaced rather than added, so a display
    /// that drew it must draw from the point before.
    pub replaced_last: bool,
    stabilizer: Stabilizer,
}

impl LiveStroke {
    fn last_pressure(&self) -> f32 {
        self.points
            .last()
            .expect("a stroke starts with a point")
            .pressure
    }

    /// Puts the end on the raw lift position rather than the smoothed one:
    /// replaces the last point when it is close or the stroke is full,
    /// otherwise adds one.
    pub fn finish(&mut self, position: [f64; 2], pressure: f32) {
        let end = point(position, pressure);
        let last = *self.points.last().expect("a stroke starts with a point");
        if self.points.len() >= store::limits::POINTS_PER_STROKE
            || distance(position, &last) < MIN_POINT_DISTANCE
        {
            let last = self.points.last_mut().expect("not empty");
            self.replaced_last = *last != end;
            *last = end;
        } else {
            self.replaced_last = false;
            self.points.push(end);
        }
    }
}

pub struct Session {
    history: History,
    layer: LayerId,
    frame: i64,
    pub tool: Tool,
    pub pen: ToolSettings,
    pub eraser: ToolSettings,
    live: Option<LiveStroke>,
    seeds: RandomState,
    strokes_started: u64,
}

impl Session {
    /// Opens `document` with its top layer current; `saved` is whether it is
    /// already on disk as it is.
    pub fn new(document: Document, saved: bool) -> Self {
        let layer = top_paint_layer(&document.layers).unwrap_or(LayerId(0));
        Self {
            history: History::new(document, saved),
            layer,
            frame: 0,
            tool: Tool::Pen,
            pen: ToolSettings::PEN,
            eraser: ToolSettings::ERASER,
            live: None,
            seeds: RandomState::new(),
            strokes_started: 0,
        }
    }

    pub fn document(&self) -> &Document {
        self.history.document()
    }

    /// Goes up on every change, undo and redo included.
    pub fn revision(&self) -> u64 {
        self.history.revision()
    }

    /// What each layer's own pixels are made from.
    pub fn layer_revisions(&self) -> &LayerRevisions {
        self.history.layer_revisions()
    }

    pub fn state(&self) -> StateId {
        self.history.state()
    }

    pub fn is_dirty(&self) -> bool {
        self.history.is_dirty()
    }

    pub fn mark_saved(&mut self, state: StateId) {
        self.history.mark_saved(state);
    }

    pub fn undo_label(&self) -> Option<&str> {
        self.history.undo_label()
    }

    pub fn redo_label(&self) -> Option<&str> {
        self.history.redo_label()
    }

    pub fn current_layer(&self) -> LayerId {
        self.layer
    }

    pub fn select_layer(&mut self, id: LayerId) {
        if self.document().layer(id).is_some() {
            self.layer = id;
        }
    }

    pub fn frame(&self) -> i64 {
        self.frame
    }

    /// Any frame number; the document's frames repeat.
    pub fn set_frame(&mut self, frame: i64) {
        self.frame = frame;
    }

    pub fn live(&self) -> Option<&LiveStroke> {
        self.live.as_ref()
    }

    fn settings(&self) -> &ToolSettings {
        match self.tool {
            Tool::Pen => &self.pen,
            Tool::Eraser => &self.eraser,
        }
    }

    pub fn begin_stroke(&mut self, input: InputPoint) -> Result<(), StrokeRefused> {
        self.live = None;
        if !matches!(
            self.document().layer(self.layer).map(|layer| &layer.kind),
            Some(LayerKind::Paint(_))
        ) {
            return Err(StrokeRefused::NoLayer);
        }
        if !shown(&self.document().layers, self.layer) {
            return Err(StrokeRefused::HiddenLayer);
        }
        let settings = *self.settings();
        let erase = self.tool == Tool::Eraser;
        self.strokes_started += 1;
        let template = Stroke {
            points: Arc::from([]),
            color: if erase {
                Rgba8([0, 0, 0, 255])
            } else {
                settings.color
            },
            width: settings.width.clamp(
                *store::limits::STROKE_WIDTH.start(),
                *store::limits::STROKE_WIDTH.end(),
            ),
            brush: Brush {
                engine: BrushEngine::Line,
                opacity: settings.opacity,
                hardness: 1.0,
                antialias: settings.antialias,
                size_dynamics: settings.size_dynamics,
                wobble_scale: 1.0,
            },
            seed: self.seeds.hash_one(self.strokes_started),
        };
        let position = clamp_position(input.position);
        self.live = Some(LiveStroke {
            layer: self.layer,
            erase,
            template,
            points: vec![point(position, pressure(input.pressure))],
            replaced_last: false,
            stabilizer: Stabilizer::new(settings.stabilizer, position, input.time),
        });
        Ok(())
    }

    /// Returns whether the point was kept.
    pub fn extend_stroke(&mut self, input: InputPoint) -> bool {
        let Some(live) = self.live.as_mut() else {
            return false;
        };
        live.replaced_last = false;
        if live.points.len() >= store::limits::POINTS_PER_STROKE {
            return false;
        }
        let position = live
            .stabilizer
            .update(clamp_position(input.position), input.time);
        let last = live.points.last().expect("a stroke starts with a point");
        if distance(position, last) < MIN_POINT_DISTANCE {
            return false;
        }
        live.points.push(point(position, pressure(input.pressure)));
        true
    }

    /// Ends the stroke where the pen lifted and commits it. The lift reports
    /// no useful pressure, so the last sampled one carries on.
    pub fn end_stroke(&mut self, input: InputPoint) -> Result<Outcome, EditError> {
        let Some(carried) = self.live.as_ref().map(LiveStroke::last_pressure) else {
            return Ok(Outcome::NoChange);
        };
        self.extend_stroke(InputPoint {
            pressure: Some(carried),
            ..input
        });
        let mut live = self.live.take().expect("still drawing");
        live.finish(clamp_position(input.position), carried);
        let stroke = Stroke {
            points: Arc::from(live.points),
            ..live.template
        };
        let label = if live.erase { "Erase" } else { "Draw" };
        self.history.edit(label, |document| {
            command::draw(document, live.layer, stroke, live.erase, None)
        })
    }

    pub fn cancel_stroke(&mut self) {
        self.live = None;
    }

    pub fn undo(&mut self) -> Result<bool, EditError> {
        self.live = None;
        let undone = self.history.undo()?;
        self.keep_layer_valid();
        Ok(undone)
    }

    pub fn redo(&mut self) -> Result<bool, EditError> {
        self.live = None;
        let redone = self.history.redo()?;
        self.keep_layer_valid();
        Ok(redone)
    }

    /// Adds an empty layer above the current one and makes it current.
    pub fn add_layer(&mut self) -> Result<Outcome, EditError> {
        let (parent, index) = self
            .document()
            .position(self.layer)
            .map_or((None, self.document().layers.len()), |(parent, index)| {
                (parent, index + 1)
            });
        let name = format!("Layer {}", command::next_layer_id(self.document()).0);
        let mut added = None;
        let outcome = self.history.edit("Add layer", |document| {
            let (id, changes) = command::add_paint_layer(document, parent, index, name);
            added = Some(id);
            changes
        })?;
        if let (Outcome::Committed(_), Some(id)) = (&outcome, added) {
            self.layer = id;
        }
        Ok(outcome)
    }

    pub fn can_remove_layer(&self) -> bool {
        paint_outside(&self.document().layers, self.layer)
    }

    /// Removes the current layer, with what it holds, unless no paint layer
    /// would be left.
    pub fn remove_layer(&mut self) -> Result<Outcome, EditError> {
        let id = self.layer;
        if !self.can_remove_layer() {
            return Ok(Outcome::NoChange);
        }
        let outcome = self
            .history
            .edit("Delete layer", |_| command::remove_layer(id))?;
        self.keep_layer_valid();
        Ok(outcome)
    }

    /// Moves the current layer up (`1`) or down (`-1`) among its siblings.
    pub fn move_layer(&mut self, by: isize) -> Result<Outcome, EditError> {
        let id = self.layer;
        let Some((parent, index)) = self.document().position(id) else {
            return Ok(Outcome::NoChange);
        };
        let Some(to) = index.checked_add_signed(by) else {
            return Ok(Outcome::NoChange);
        };
        let count = command::siblings(self.document(), parent).map_or(0, <[Layer]>::len);
        if to >= count {
            return Ok(Outcome::NoChange);
        }
        let changes = command::move_layer(self.document(), id, parent, to)?;
        self.history.edit("Move layer", |_| changes)
    }

    /// Puts the current layer in a new group in its place.
    pub fn add_group(&mut self) -> Result<Outcome, EditError> {
        let name = (1..)
            .map(|number| format!("Group {number}"))
            .find(|name| !named(&self.document().layers, name))
            .expect("fewer layers than numbers");
        let (_, changes) = command::wrap_in_group(self.document(), self.layer, name)?;
        self.history.edit("Add layer group", |_| changes)
    }

    /// Puts the children of the current group in its place; the top one
    /// becomes current.
    pub fn ungroup(&mut self) -> Result<Outcome, EditError> {
        let group = self.layer;
        let Some(LayerKind::Group(content)) = self.document().layer(group).map(|layer| &layer.kind)
        else {
            return Ok(Outcome::NoChange);
        };
        let top = content.children.last().map(|child| child.id);
        let changes = command::ungroup(self.document(), group)?;
        let outcome = self.history.edit("Ungroup", |_| changes)?;
        if let (Outcome::Committed(_), Some(top)) = (&outcome, top) {
            self.layer = top;
        }
        self.keep_layer_valid();
        Ok(outcome)
    }

    /// Moves the current layer to the top of `group`, or out to the top level
    /// with `None`.
    pub fn move_to_group(&mut self, group: Option<LayerId>) -> Result<Outcome, EditError> {
        let changes = command::move_to_group(self.document(), self.layer, group)?;
        let label = match group {
            Some(_) => "Move layer into group",
            None => "Move layer out of groups",
        };
        self.history.edit(label, |_| changes)
    }

    /// Changes one of a layer's own properties.
    pub fn update_layer(
        &mut self,
        id: LayerId,
        label: &str,
        update: impl FnOnce(&mut Layer),
    ) -> Result<Outcome, EditError> {
        let changes = command::update_layer(self.document(), id, update)?;
        let unchanged = changes.iter().all(|change| match change {
            ugu_core::edit::Change::ReplaceLayer(layer) => {
                self.document().layer(layer.id) == Some(layer)
            }
            _ => false,
        });
        if unchanged {
            return Ok(Outcome::NoChange);
        }
        self.history.edit(label, |_| changes)
    }

    /// Merges the current layer into the one below, which becomes current.
    pub fn merge_down(&mut self) -> Result<Outcome, EditError> {
        let above = self.layer;
        let below = self.document().position(above).and_then(|(parent, index)| {
            let siblings = command::siblings(self.document(), parent)?;
            Some(siblings.get(index.checked_sub(1)?)?.id)
        });
        let changes = command::merge_down(self.document(), above)?;
        let outcome = self.history.edit("Merge down", |_| changes)?;
        if let Some(below) = below {
            self.layer = below;
        }
        Ok(outcome)
    }

    /// Changes the frame count, playback speed and wobble together.
    pub fn set_animation(
        &mut self,
        frames: u32,
        frames_per_second: f32,
        wobble: Wobble,
    ) -> Result<Outcome, EditError> {
        let mut settings = self.document().settings();
        settings.frames = frames.clamp(*limits::FRAMES.start(), *limits::FRAMES.end());
        settings.frames_per_second = frames_per_second;
        settings.wobble = wobble;
        if settings == self.document().settings() {
            return Ok(Outcome::NoChange);
        }
        self.history.edit("Animation settings", |_| {
            vec![ugu_core::edit::Change::SetSettings(settings)]
        })
    }

    /// After undo, redo or removal the current layer may be gone.
    fn keep_layer_valid(&mut self) {
        if self.document().layer(self.layer).is_none() {
            self.layer = top_paint_layer(&self.document().layers).unwrap_or(LayerId(0));
        }
    }
}

/// Whether `id` and every group holding it are visible.
fn shown(layers: &[Layer], id: LayerId) -> bool {
    layers.iter().any(|layer| {
        layer.visible
            && (layer.id == id
                || matches!(&layer.kind, LayerKind::Group(group) if shown(&group.children, id)))
    })
}

/// Whether a paint layer is left when `id` and what it holds are taken out.
fn paint_outside(layers: &[Layer], id: LayerId) -> bool {
    layers.iter().any(|layer| {
        layer.id != id
            && match &layer.kind {
                LayerKind::Paint(_) => true,
                LayerKind::Group(group) => paint_outside(&group.children, id),
            }
    })
}

/// Whether a layer is called `name`, inside groups too.
fn named(layers: &[Layer], name: &str) -> bool {
    layers.iter().any(|layer| {
        layer.name == name
            || matches!(&layer.kind, LayerKind::Group(group) if named(&group.children, name))
    })
}

/// The paint layer drawn last, inside groups too.
fn top_paint_layer(layers: &[Layer]) -> Option<LayerId> {
    layers.iter().rev().find_map(|layer| match &layer.kind {
        LayerKind::Paint(_) => Some(layer.id),
        LayerKind::Group(group) => top_paint_layer(&group.children),
    })
}

fn clamp_position(position: [f64; 2]) -> [f64; 2] {
    let limit = f64::from(store::limits::COORDINATE);
    position.map(|value| value.clamp(-limit, limit))
}

/// Devices without pressure draw at full pressure; a pen's light touch never
/// reaches zero width.
fn pressure(pressure: Option<f32>) -> f32 {
    pressure.map_or(1.0, |pressure| pressure.clamp(0.05, 1.0))
}

fn point(position: [f64; 2], pressure: f32) -> Point {
    Point {
        x: position[0] as f32,
        y: position[1] as f32,
        pressure,
    }
}

fn distance(position: [f64; 2], point: &Point) -> f64 {
    (position[0] - f64::from(point.x)).hypot(position[1] - f64::from(point.y))
}

#[cfg(test)]
mod tests;

// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! One open document and what the user is doing with it: the tools, the
//! stroke being drawn, the current layer and frame, and the history that
//! every change goes through.
//!
//! Input reaches the session in document pixels. A stroke is held here until
//! the pen lifts and is then committed as one edit; a cancelled stroke leaves
//! nothing behind.

mod clipboard;
mod filling;
mod placing;
mod restyling;
mod selecting;
pub mod stabilizer;
mod tools;
mod transforming;

use std::collections::HashMap;
use std::hash::{BuildHasher, RandomState};
use std::sync::Arc;

use ugu_core::brush::{self, Preset};
use ugu_core::command;
use ugu_core::document::{Document, Layer, LayerId, LayerKind, limits};
use ugu_core::edit::{EditError, Outcome};
use ugu_core::history::{History, LayerRevisions, StateId};
use ugu_core::ops::{Rgba8, Sampling, Wobble};
use ugu_core::store::{self, Brush, BrushEngine, Point, Stroke};

pub use crate::filling::{FillError, FillSettings, Reads};
pub use crate::placing::{Placed, TextDrawing, TextSettings};
pub use crate::selecting::{Lasso, ShapeKind};
use crate::stabilizer::Stabilizer;
pub use crate::tools::{ColorHistory, Tools};
pub use crate::transforming::{Ended, Pending};
use ugu_core::selection::Selection;

/// Points closer than this to the last kept one are dropped, as in 2.2.13.
const MIN_POINT_DISTANCE: f64 = 0.75;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    Pen,
    Eraser,
    Select,
    Wand,
    Fill,
    Text,
    Eyedropper,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ToolSettings {
    /// Ignored by the eraser.
    pub color: Rgba8,
    pub width: f32,
    /// The brush tool's own setting, which its presets follow; an eraser
    /// takes its preset's.
    pub antialias: bool,
    /// 0 (off) to 1.
    pub stabilizer: f32,
    /// What the stroke is drawn with.
    pub preset: &'static Preset,
}

impl ToolSettings {
    /// 2.2.13's ink pen: width 6, black, aliased.
    pub const PEN: Self = Self {
        color: Rgba8([0, 0, 0, 255]),
        width: 6.0,
        antialias: false,
        stabilizer: 0.0,
        preset: &brush::BRUSHES[0],
    };
    /// 2.2.13's hard eraser.
    pub const ERASER: Self = Self {
        color: Rgba8([0, 0, 0, 255]),
        width: 6.0,
        antialias: true,
        stabilizer: 0.0,
        preset: &brush::ERASERS[0],
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
    /// The selection the stroke is cut to.
    pub clip: Option<Arc<Selection>>,
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
    tool: Tool,
    pub pen: ToolSettings,
    pub eraser: ToolSettings,
    /// The width and stabilizer each preset not in use had when another was
    /// chosen.
    remembered: HashMap<&'static str, (f32, f32)>,
    live: Option<LiveStroke>,
    /// The selection tool's shape.
    pub selection_shape: ShapeKind,
    /// The selection tool fills its shape instead of selecting.
    pub lasso_paints: bool,
    /// How the wand and the bucket find an area, and how fills look.
    pub fill: FillSettings,
    /// How a transformed selection is resampled.
    pub transform_sampling: Sampling,
    pending: Option<Pending>,
    /// The text tool's settings and the text it places.
    pub text: TextSettings,
    pub text_content: String,
    /// The colours drawn, filled and written with.
    pub colors: ColorHistory,
    placed: Option<Placed>,
    ended: Option<Ended>,
    lasso: Option<Lasso>,
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
            remembered: HashMap::new(),
            live: None,
            selection_shape: ShapeKind::default(),
            lasso_paints: false,
            fill: FillSettings::DEFAULT,
            transform_sampling: Sampling::Smooth,
            pending: None,
            text: TextSettings::DEFAULT,
            text_content: String::new(),
            colors: ColorHistory::default(),
            placed: None,
            ended: None,
            lasso: None,
            seeds: RandomState::new(),
            strokes_started: 0,
        }
    }

    pub fn document(&self) -> &Document {
        self.history.document()
    }

    pub fn tool(&self) -> Tool {
        self.tool
    }

    /// Changes the tool; a pending transform or placed text is applied
    /// first, as changing tools ends it.
    pub fn set_tool(&mut self, tool: Tool) {
        if tool != self.tool {
            self.settle();
            self.tool = tool;
        }
    }

    /// The history, with a pending transform applied first.
    fn history_mut(&mut self) -> &mut History {
        self.settle();
        &mut self.history
    }

    /// Makes `preset` the brush, or with `Tool::Eraser` the eraser. Each
    /// preset keeps its own width and stabilizer, starting from its size and
    /// none, as in 2.2.13.
    pub fn choose_preset(&mut self, tool: Tool, preset: &'static Preset) {
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
        // Pixel art fills without blending edges too; it can be turned on
        // again.
        if preset.brush.engine == BrushEngine::Pixel && tool != Tool::Eraser {
            self.fill.antialias = false;
        }
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

    /// A pending transform or placed text counts as an unsaved change.
    pub fn is_dirty(&self) -> bool {
        self.history.is_dirty() || self.pending.is_some() || self.placed.is_some()
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
        if self.document().layer(id).is_some() && id != self.layer {
            self.settle();
            self.layer = id;
        }
    }

    pub fn frame(&self) -> i64 {
        self.frame
    }

    /// Any frame number; the document's frames repeat.
    pub fn set_frame(&mut self, frame: i64) {
        if frame != self.frame {
            self.settle();
        }
        self.frame = frame;
    }

    pub fn live(&self) -> Option<&LiveStroke> {
        self.live.as_ref()
    }

    /// The settings strokes are drawn with: the eraser's, else the pen's.
    fn settings(&self) -> &ToolSettings {
        match self.tool {
            Tool::Eraser => &self.eraser,
            Tool::Pen | Tool::Select | Tool::Wand | Tool::Fill | Tool::Text | Tool::Eyedropper => {
                &self.pen
            }
        }
    }

    /// Whether the current layer can be drawn on.
    fn can_paint(&self) -> Result<(), StrokeRefused> {
        if !matches!(
            self.document().layer(self.layer).map(|layer| &layer.kind),
            Some(LayerKind::Paint(_))
        ) {
            return Err(StrokeRefused::NoLayer);
        }
        if !shown(&self.document().layers, self.layer) {
            return Err(StrokeRefused::HiddenLayer);
        }
        Ok(())
    }

    /// Applies a pending transform or placed text first, so the stroke is
    /// cut to the selection as they leave it.
    pub fn begin_stroke(&mut self, input: InputPoint) -> Result<(), StrokeRefused> {
        self.live = None;
        self.can_paint()?;
        self.settle();
        let settings = *self.settings();
        let erase = self.tool == Tool::Eraser;
        self.strokes_started += 1;
        // A pixel brush draws whole pixels, a whole number of them wide.
        let pixel = settings.preset.brush.engine == BrushEngine::Pixel;
        let width = if pixel {
            settings.width.round()
        } else {
            settings.width
        };
        let template = Stroke {
            points: Arc::from([]),
            color: if erase {
                Rgba8([0, 0, 0, 255])
            } else {
                settings.color
            },
            width: width.clamp(
                *store::limits::STROKE_WIDTH.start(),
                *store::limits::STROKE_WIDTH.end(),
            ),
            brush: Brush {
                antialias: settings.antialias && !pixel,
                ..settings.preset.brush
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
            clip: self.clip(),
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
        let color = stroke.color;
        let outcome = self.history_mut().group(label, |group| {
            let clip = match &live.clip {
                Some(selection) => Some(selecting::stored_mask(group, selection)?),
                None => None,
            };
            group.apply(|document| command::draw(document, live.layer, stroke, live.erase, clip))
        })?;
        if !live.erase && matches!(outcome, Outcome::Committed(_)) {
            self.colors.record(color);
        }
        Ok(outcome)
    }

    pub fn cancel_stroke(&mut self) {
        self.live = None;
    }

    /// Cancels a pending transform or placed text, else undoes the last
    /// change.
    pub fn undo(&mut self) -> Result<bool, EditError> {
        self.live = None;
        self.lasso = None;
        if self.cancel_transform() || self.cancel_text() {
            return Ok(true);
        }
        let undone = self.history.undo()?;
        self.keep_layer_valid();
        Ok(undone)
    }

    /// Cancels a pending transform or placed text, else redoes the last
    /// change undone.
    pub fn redo(&mut self) -> Result<bool, EditError> {
        self.live = None;
        self.lasso = None;
        if self.cancel_transform() || self.cancel_text() {
            return Ok(true);
        }
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
        let outcome = self.history_mut().edit("Add layer", |document| {
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
        self.history_mut().edit("Move layer", |_| changes)
    }

    /// Moves `id` to `index` in `parent` (`None` for the top level),
    /// counted after it is taken out.
    pub fn move_layer_to(
        &mut self,
        id: LayerId,
        parent: Option<LayerId>,
        index: usize,
    ) -> Result<Outcome, EditError> {
        let changes = command::move_layer(self.document(), id, parent, index)?;
        self.history_mut().edit("Move layer", |_| changes)
    }

    /// Puts the current layer in a new group in its place.
    pub fn add_group(&mut self) -> Result<Outcome, EditError> {
        let name = (1..)
            .map(|number| format!("Group {number}"))
            .find(|name| !named(&self.document().layers, name))
            .expect("fewer layers than numbers");
        let (_, changes) = command::wrap_in_group(self.document(), self.layer, name)?;
        self.history_mut().edit("Add layer group", |_| changes)
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
        let outcome = self.history_mut().edit("Ungroup", |_| changes)?;
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
        self.history_mut().edit(label, |_| changes)
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
        self.history_mut().edit(label, |_| changes)
    }

    /// Merges the current layer into the one below, which becomes current.
    pub fn merge_down(&mut self) -> Result<Outcome, EditError> {
        let above = self.layer;
        let below = self.document().position(above).and_then(|(parent, index)| {
            let siblings = command::siblings(self.document(), parent)?;
            Some(siblings.get(index.checked_sub(1)?)?.id)
        });
        let changes = command::merge_down(self.document(), above)?;
        let outcome = self.history_mut().edit("Merge down", |_| changes)?;
        if let Some(below) = below {
            self.layer = below;
        }
        Ok(outcome)
    }

    /// Changes the canvas size; what is drawn keeps its pixels and moves by
    /// `offset`.
    pub fn crop_canvas(&mut self, offset: [i32; 2], size: [u32; 2]) -> Result<Outcome, EditError> {
        if offset == [0, 0] && size == self.document().canvas {
            return Ok(Outcome::NoChange);
        }
        self.lasso = None;
        self.history_mut().group("Canvas size", |group| {
            group.apply(|document| command::crop_canvas(document, offset, size))?;
            let moved = group
                .selection()
                .and_then(|selection| selection.cropped(offset, size));
            group.select(moved);
            Ok(())
        })
    }

    /// Resizes the image: everything drawn so far is resampled to `size`.
    pub fn resample_image(
        &mut self,
        size: [u32; 2],
        sampling: Sampling,
    ) -> Result<Outcome, EditError> {
        if size == self.document().canvas {
            return Ok(Outcome::NoChange);
        }
        self.lasso = None;
        self.history_mut().group("Image size", |group| {
            group.apply(|document| command::resample_image(document, size, sampling))?;
            let scaled = group
                .selection()
                .and_then(|selection| selection.resampled(size));
            group.select(scaled);
            Ok(())
        })
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
        self.history_mut().edit("Animation settings", |_| {
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

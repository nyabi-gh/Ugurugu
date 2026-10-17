// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! One open document and what the user is doing with it: the stroke being
//! drawn, the current layer and frame, and the history that every change
//! goes through. The tools are the window's, handed in when an edit starts.
//!
//! Input reaches the session in document pixels; what is drawn goes into the
//! current layer's own pixels, taken back through its move on the frame
//! shown. A stroke is held here until the pen lifts and is then committed as
//! one edit; a cancelled stroke leaves nothing behind.

mod animating;
mod clipboard;
mod filling;
mod placing;
mod restyling;
mod selecting;
mod shaping;
pub mod stabilizer;
mod symmetry;
mod tools;
mod transforming;

use std::hash::{BuildHasher, RandomState};
use std::sync::Arc;

use ugu_core::brush::{self, Preset};
use ugu_core::command;
use ugu_core::document::{Document, Layer, LayerId, LayerKind, limits};
use ugu_core::edit::{EditError, Outcome};
use ugu_core::history::{History, LayerRevisions, StateId};
use ugu_core::ops::{Affine, Rgba8, Sampling, Wobble};
use ugu_core::store::{self, Brush, BrushEngine, Point, Stroke};

pub use crate::animating::Posing;
pub use crate::filling::{FillError, FillSettings, Reads};
pub use crate::placing::{Placed, TextDrawing, TextSettings, TextStyle};
pub use crate::selecting::{Lasso, ShapeKind};
pub use crate::shaping::{Drag, Shape, ShapeFill, ShapePaint, ShapeSettings, outline};
use crate::stabilizer::Stabilizer;
pub use crate::symmetry::Symmetry;
pub use crate::tools::{Brushes, ColorHistory, Kept, Tools, is_pixel};
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
    Shape,
    /// Moves, turns and scales the current layer on the frame shown.
    Move,
    /// Places the symmetry ruler on the canvas.
    Symmetry,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ToolSettings {
    pub width: f32,
    /// The brush tool's own setting, which its presets follow; an eraser
    /// takes its preset's.
    pub antialias: bool,
    /// 0 (off) to 1.
    pub stabilizer: f32,
    /// How opaque the whole stroke is, in place of its preset's; a pixel
    /// brush draws opaque whatever it is.
    pub opacity: f32,
    /// What the stroke is drawn with.
    pub preset: &'static Preset,
}

impl ToolSettings {
    /// 2.2.13's ink pen: width 6, aliased.
    pub const PEN: Self = Self {
        width: 6.0,
        antialias: false,
        stabilizer: 0.0,
        opacity: brush::BRUSHES[0].brush.opacity,
        preset: &brush::BRUSHES[0],
    };
    /// 2.2.13's hard eraser.
    pub const ERASER: Self = Self {
        width: 6.0,
        antialias: true,
        stabilizer: 0.0,
        opacity: brush::ERASERS[0].brush.opacity,
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
    /// The pending transform or placed text cannot be applied first.
    Pending,
    /// The selection covers none of the current layer's own pixels on this
    /// frame.
    OutsideSelection,
}

/// The stroke being drawn.
#[derive(Clone, Debug)]
pub struct LiveStroke {
    pub layer: LayerId,
    pub erase: bool,
    /// Everything of the stroke but its points.
    pub template: Stroke,
    /// In the layer's own pixels.
    pub points: Vec<Point>,
    /// Set when the last point was replaced rather than added, so a display
    /// that drew it must draw from the point before.
    pub replaced_last: bool,
    /// The selection the stroke is cut to, in the layer's own pixels.
    pub clip: Option<Arc<Selection>>,
    /// From document pixels to the layer's, as when the stroke began.
    to_layer: Option<Affine>,
    stabilizer: Stabilizer,
    /// The pressure setting when the stroke began, which it keeps.
    uses_pressure: bool,
    /// The shape dragged out, for a stroke the shape tool draws.
    shaping: Option<shaping::Shaping>,
    /// Whether the stroke itself is drawn; a shape that is only filled
    /// draws none.
    pub outlined: bool,
    /// The area a shape fills, in the layer's own pixels.
    pub fill: Option<Vec<[f64; 2]>>,
    /// The strokes the symmetry ruler draws with this one, drawn after it.
    pub copies: Vec<CopiedStroke>,
}

/// A stroke the symmetry ruler repeats as the one drawn by hand goes.
#[derive(Clone, Debug)]
pub struct CopiedStroke {
    /// Its own seed, so that it wobbles as a stroke of its own.
    pub template: Stroke,
    /// The hand-drawn stroke's points moved, in the layer's own pixels.
    pub points: Vec<Point>,
    /// From the hand-drawn stroke's layer pixels to this one's.
    transform: Affine,
}

impl LiveStroke {
    /// Brings the copies' points up to the stroke's own after its last
    /// point was added or replaced.
    fn copy_last(&mut self) {
        let last = *self.points.last().expect("a stroke starts with a point");
        let kept = self.points.len() - 1;
        for copy in &mut self.copies {
            copy.points.truncate(kept);
            let moved = copy.transform.apply([f64::from(last.x), f64::from(last.y)]);
            copy.points
                .push(point(clamp_position(moved), last.pressure));
        }
    }

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
        self.copy_last();
    }
}

pub struct Session {
    history: History,
    layer: LayerId,
    frame: i64,
    live: Option<LiveStroke>,
    pending: Option<Pending>,
    /// The text the text tool places.
    pub text_content: String,
    placed: Option<Placed>,
    /// How the pending transform and placed text are applied.
    style: Style,
    /// The colours drawn, filled and written with since last taken.
    used: Vec<Rgba8>,
    ended: Option<Ended>,
    lasso: Option<Lasso>,
    seeds: RandomState,
    strokes_started: u64,
    posing: Option<Posing>,
    /// The cel added for the stroke or fill under way, its folder and the
    /// state the addition left.
    made_cel: Option<(LayerId, StateId)>,
    /// The symmetry ruler, set for a canvas of `symmetry_canvas`.
    symmetry: Symmetry,
    symmetry_canvas: [u32; 2],
}

/// The window's tools as last handed in with `Session::use_tools`, for the
/// work that waits: a pending transform and placed text.
#[derive(Clone, Debug, PartialEq)]
struct Style {
    sampling: Sampling,
    text: TextStyle,
}

impl Session {
    /// Opens `document` with its top layer current; `saved` is whether it is
    /// already on disk as it is.
    pub fn new(document: Document, saved: bool) -> Self {
        let layer = top_paint_layer(&document.layers).unwrap_or(LayerId(0));
        let canvas = document.canvas;
        let mut session = Self {
            history: History::new(document, saved),
            layer,
            frame: 0,
            live: None,
            pending: None,
            text_content: String::new(),
            placed: None,
            style: Style {
                sampling: Sampling::Smooth,
                text: TextStyle::of(&Tools::default(), false),
            },
            used: Vec::new(),
            ended: None,
            lasso: None,
            seeds: RandomState::new(),
            strokes_started: 0,
            posing: None,
            made_cel: None,
            symmetry: Symmetry::new(canvas),
            symmetry_canvas: canvas,
        };
        session.use_tools(&Tools::default());
        session
    }

    /// The symmetry ruler. Its centre keeps its place on the canvas, as a
    /// share of each side, when the canvas changes size, undone or not.
    pub fn symmetry(&self) -> Symmetry {
        let canvas = self.document().canvas;
        if canvas == self.symmetry_canvas {
            return self.symmetry;
        }
        let mut symmetry = self.symmetry;
        for ((centre, now), then) in symmetry
            .centre
            .iter_mut()
            .zip(canvas)
            .zip(self.symmetry_canvas)
        {
            *centre *= f64::from(now) / f64::from(then);
        }
        symmetry
    }

    /// Sets the symmetry ruler, which applies from the next stroke. It is
    /// the window's and no edit of the document.
    pub fn set_symmetry(&mut self, symmetry: Symmetry) {
        self.symmetry = symmetry;
        self.symmetry_canvas = self.document().canvas;
    }

    pub fn document(&self) -> &Document {
        self.history.document()
    }

    /// Takes the window's `tools` as what a pending transform and placed
    /// text are applied with. Called whenever they may have changed.
    pub fn use_tools(&mut self, tools: &Tools) {
        self.style = Style {
            sampling: tools.transform_sampling,
            text: TextStyle::of(tools, self.pixel_mode()),
        };
    }

    /// The colours drawn, filled and written with since last asked, oldest
    /// first, for the colour history.
    pub fn take_used_colors(&mut self) -> Vec<Rgba8> {
        std::mem::take(&mut self.used)
    }

    /// The history, with a pending transform applied first.
    fn history_mut(&mut self) -> Result<&mut History, EditError> {
        self.settle()?;
        Ok(&mut self.history)
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

    pub fn select_layer(&mut self, id: LayerId) -> Result<(), EditError> {
        if self.document().layer(id).is_some() && id != self.layer {
            self.settle()?;
            self.layer = id;
        }
        Ok(())
    }

    pub fn frame(&self) -> i64 {
        self.frame
    }

    /// Any frame number; the document's frames repeat. A cel being edited
    /// gives way to the one its folder shows on the new frame.
    pub fn set_frame(&mut self, frame: i64) -> Result<(), EditError> {
        if frame != self.frame {
            self.settle()?;
            self.posing = None;
        }
        self.frame = frame;
        self.follow_cel();
        Ok(())
    }

    pub fn live(&self) -> Option<&LiveStroke> {
        self.live.as_ref()
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

    /// Starts a stroke with the current tool of `tools`: the eraser, else
    /// the brush, of the brushes for the document's mode. Applies a pending
    /// transform or placed text first, so the stroke is cut to the selection
    /// as they leave it.
    pub fn begin_stroke(&mut self, tools: &Tools, input: InputPoint) -> Result<(), StrokeRefused> {
        self.start_stroke(tools, input, true)
    }

    /// `begin_stroke`, with the symmetry ruler's copies when `repeated`.
    pub(crate) fn start_stroke(
        &mut self,
        tools: &Tools,
        input: InputPoint,
        repeated: bool,
    ) -> Result<(), StrokeRefused> {
        self.drop_live();
        // A folder draws on its cel, which may not be there yet.
        if self.folder() != Some(self.layer) {
            self.can_paint()?;
        }
        self.settle().map_err(|_| StrokeRefused::Pending)?;
        self.draw_on_cel().map_err(|_| StrokeRefused::NoLayer)?;
        let clip = self.stroke_clip();
        if clip.is_err() {
            self.drop_cel();
        }
        let clip = clip?;
        let to_layer = self
            .layer_transform()
            .and_then(|transform| transform.inverse());
        let settings = *tools.stroke(self.pixel_mode());
        let erase = tools.tool == Tool::Eraser;
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
                tools.color
            },
            width: width.clamp(
                *store::limits::STROKE_WIDTH.start(),
                *store::limits::STROKE_WIDTH.end(),
            ),
            brush: Brush {
                antialias: settings.antialias && !pixel,
                opacity: if pixel {
                    settings.preset.brush.opacity
                } else {
                    settings.opacity.clamp(0.01, 1.0)
                },
                ..settings.preset.brush
            },
            seed: self.seeds.hash_one(self.strokes_started),
        };
        let position = clamp_position(into(to_layer, input.position));
        // The ruler is on the canvas as shown; the copies are found in the
        // layer's own pixels by way of it.
        let from_layer = self.layer_transform().filter(|_| to_layer.is_some());
        let ruler = if repeated {
            self.symmetry().copies()
        } else {
            Vec::new()
        };
        let copies = ruler
            .into_iter()
            .map(|copy| {
                let transform = from_layer
                    .unwrap_or(Affine::IDENTITY)
                    .then(copy)
                    .then(to_layer.unwrap_or(Affine::IDENTITY));
                self.strokes_started += 1;
                CopiedStroke {
                    template: Stroke {
                        seed: self.seeds.hash_one(self.strokes_started),
                        ..template.clone()
                    },
                    points: Vec::new(),
                    transform,
                }
            })
            .collect();
        let mut live = LiveStroke {
            layer: self.layer,
            erase,
            template,
            points: vec![point(
                position,
                pressure(input.pressure, tools.uses_pressure),
            )],
            replaced_last: false,
            clip,
            to_layer,
            stabilizer: Stabilizer::new(settings.stabilizer, position, input.time),
            uses_pressure: tools.uses_pressure,
            shaping: None,
            outlined: true,
            fill: None,
            copies,
        };
        live.copy_last();
        self.live = Some(live);
        Ok(())
    }

    /// The selection a stroke on the current layer is cut to, in its own
    /// pixels.
    fn stroke_clip(&self) -> Result<Option<Arc<Selection>>, StrokeRefused> {
        self.can_paint()?;
        match self.clip() {
            Some(selection) => self
                .in_layer(&selection)
                .map(Some)
                .ok_or(StrokeRefused::OutsideSelection),
            None => Ok(None),
        }
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
        let position = live.stabilizer.update(
            clamp_position(into(live.to_layer, input.position)),
            input.time,
        );
        let last = live.points.last().expect("a stroke starts with a point");
        if distance(position, last) < MIN_POINT_DISTANCE {
            return false;
        }
        live.points.push(point(
            position,
            pressure(input.pressure, live.uses_pressure),
        ));
        live.copy_last();
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
        let end = into(live.to_layer, input.position);
        live.finish(clamp_position(end), carried);
        self.commit(live)
    }

    /// Commits `live`, and the symmetry ruler's copies after it, as one
    /// edit.
    fn commit(&mut self, live: LiveStroke) -> Result<Outcome, EditError> {
        let color = live.template.color;
        let strokes: Vec<Stroke> = std::iter::once(Stroke {
            points: Arc::from(live.points),
            ..live.template
        })
        .chain(live.copies.into_iter().map(|copy| Stroke {
            points: Arc::from(copy.points),
            ..copy.template
        }))
        .collect();
        let label = if live.erase { "Erase" } else { "Draw" };
        let outcome = self.with_cel(|session| {
            session.history_mut()?.group(label, |group| {
                let clip = match &live.clip {
                    Some(selection) => Some(selecting::stored_mask(group, selection)?),
                    None => None,
                };
                for stroke in strokes {
                    group.apply(|document| {
                        command::draw(document, live.layer, stroke, live.erase, clip)
                    })?;
                }
                Ok(())
            })
        })?;
        if !live.erase && matches!(outcome, Outcome::Committed(_)) {
            self.used.push(color);
        }
        Ok(outcome)
    }

    pub fn cancel_stroke(&mut self) {
        self.drop_live();
    }

    /// Ends the stroke under way without it, and the cel added for it.
    pub(crate) fn drop_live(&mut self) {
        self.live = None;
        self.drop_cel();
    }

    /// Cancels a pending transform or placed text, else undoes the last
    /// change.
    pub fn undo(&mut self) -> Result<bool, EditError> {
        self.live = None;
        self.made_cel = None;
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
        self.drop_live();
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
        let outcome = self.history_mut()?.edit("Add layer", |document| {
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
        let outcome = self.history_mut()?.edit("Delete layer", |document| {
            command::remove_layer(document, id)
        })?;
        self.keep_layer_valid();
        Ok(outcome)
    }

    pub fn can_duplicate_layer(&self) -> bool {
        matches!(
            self.document().layer(self.layer).map(|layer| &layer.kind),
            Some(LayerKind::Paint(_))
        )
    }

    /// Copies the current paint layer, pending edits applied, right above
    /// it and makes the copy current. `name` makes the copy's name from the
    /// original's.
    pub fn duplicate_layer(
        &mut self,
        name: impl FnOnce(&str) -> String,
    ) -> Result<Outcome, EditError> {
        if !self.can_duplicate_layer() {
            return Ok(Outcome::NoChange);
        }
        self.settle()?;
        let source = self.layer;
        let original = self
            .document()
            .layer(source)
            .map_or("", |layer| &layer.name);
        let name: String = name(original)
            .chars()
            .take(limits::LAYER_NAME_CHARS)
            .collect();
        let (copy, changes) = command::duplicate_layer(self.document(), source, name)?;
        let outcome = self.history_mut()?.edit("Duplicate layer", |_| changes)?;
        if let Outcome::Committed(_) = outcome {
            self.layer = copy;
        }
        Ok(outcome)
    }

    /// Moves the current layer up (`1`) or down (`-1`) among its siblings.
    pub fn move_layer(&mut self, by: isize) -> Result<Outcome, EditError> {
        self.settle()?;
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
        self.history_mut()?.edit("Move layer", |_| changes)
    }

    /// Moves `id` to `index` in `parent` (`None` for the top level),
    /// counted after it is taken out.
    pub fn move_layer_to(
        &mut self,
        id: LayerId,
        parent: Option<LayerId>,
        index: usize,
    ) -> Result<Outcome, EditError> {
        self.settle()?;
        let changes = command::move_layer(self.document(), id, parent, index)?;
        self.history_mut()?.edit("Move layer", |_| changes)
    }

    /// Puts the current layer in a new group in its place.
    pub fn add_group(&mut self) -> Result<Outcome, EditError> {
        self.settle()?;
        let name = (1..)
            .map(|number| format!("Group {number}"))
            .find(|name| !named(&self.document().layers, name))
            .expect("fewer layers than numbers");
        let (_, changes) = command::wrap_in_group(self.document(), self.layer, name)?;
        self.history_mut()?.edit("Add layer group", |_| changes)
    }

    /// Puts the children of the current group in its place; the top one
    /// becomes current.
    pub fn ungroup(&mut self) -> Result<Outcome, EditError> {
        self.settle()?;
        let group = self.layer;
        let Some(LayerKind::Group(content)) = self.document().layer(group).map(|layer| &layer.kind)
        else {
            return Ok(Outcome::NoChange);
        };
        let top = content.children.last().map(|child| child.id);
        let changes = command::ungroup(self.document(), group)?;
        let outcome = self.history_mut()?.edit("Ungroup", |_| changes)?;
        if let (Outcome::Committed(_), Some(top)) = (&outcome, top) {
            self.layer = top;
        }
        self.keep_layer_valid();
        Ok(outcome)
    }

    /// Moves the current layer to the top of `group`, or out to the top level
    /// with `None`.
    pub fn move_to_group(&mut self, group: Option<LayerId>) -> Result<Outcome, EditError> {
        self.settle()?;
        let changes = command::move_to_group(self.document(), self.layer, group)?;
        let label = match group {
            Some(_) => "Move layer into group",
            None => "Move layer out of groups",
        };
        self.history_mut()?.edit(label, |_| changes)
    }

    /// Changes one of a layer's own properties.
    pub fn update_layer(
        &mut self,
        id: LayerId,
        label: &str,
        update: impl FnOnce(&mut Layer),
    ) -> Result<Outcome, EditError> {
        self.settle()?;
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
        self.history_mut()?.edit(label, |_| changes)
    }

    /// Merges the current layer into the one below, which becomes current.
    pub fn merge_down(&mut self) -> Result<Outcome, EditError> {
        self.settle()?;
        let above = self.layer;
        let below = self.document().position(above).and_then(|(parent, index)| {
            let siblings = command::siblings(self.document(), parent)?;
            Some(siblings.get(index.checked_sub(1)?)?.id)
        });
        let changes = command::merge_down(self.document(), above)?;
        let outcome = self.history_mut()?.edit("Merge down", |_| changes)?;
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
        self.history_mut()?.group("Canvas size", |group| {
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
        let sampling = if self.pixel_mode() {
            Sampling::Nearest
        } else {
            sampling
        };
        self.history_mut()?.group("Image size", |group| {
            group.apply(|document| command::resample_image(document, size, sampling))?;
            let scaled = group
                .selection()
                .and_then(|selection| selection.resampled(size));
            group.select(scaled);
            Ok(())
        })
    }

    /// Whether the document is in pixel mode: strokes take the window's
    /// pixel brushes, fills and text have hard edges and transforms move
    /// whole pixels.
    pub fn pixel_mode(&self) -> bool {
        self.document().pixel
    }

    /// Turns pixel mode on or off as one step.
    pub fn set_pixel_mode(&mut self, on: bool) -> Result<Outcome, EditError> {
        let mut settings = self.document().settings();
        if settings.pixel == on {
            return Ok(Outcome::NoChange);
        }
        settings.pixel = on;
        self.history_mut()?.edit("Pixel mode", |_| {
            vec![ugu_core::edit::Change::SetSettings(settings)]
        })
    }

    /// How a transform resamples: whole pixels in pixel mode, otherwise as
    /// `chosen`.
    pub fn sampling(&self, chosen: Sampling) -> Sampling {
        if self.pixel_mode() {
            Sampling::Nearest
        } else {
            chosen
        }
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
        let outcome = self.history_mut()?.edit("Animation settings", |document| {
            command::set_settings(document, settings)
        })?;
        self.follow_cel();
        Ok(outcome)
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

/// `position` through `transform`, if there is one.
fn into(transform: Option<Affine>, position: [f64; 2]) -> [f64; 2] {
    transform.map_or(position, |transform| transform.apply(position))
}

fn clamp_position(position: [f64; 2]) -> [f64; 2] {
    let limit = f64::from(store::limits::COORDINATE);
    position.map(|value| value.clamp(-limit, limit))
}

/// Devices without pressure, and any with pressure not `used`, draw at full
/// pressure; a pen's light touch never reaches zero width.
fn pressure(pressure: Option<f32>, used: bool) -> f32 {
    match pressure {
        Some(pressure) if used => pressure.clamp(0.05, 1.0),
        _ => 1.0,
    }
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

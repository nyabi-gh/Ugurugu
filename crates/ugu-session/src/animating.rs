// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! Keys and animation folders as the user edits them: the layer space that
//! drawing happens in, poses keyed on the frame shown, and cels that follow
//! the frame.
//!
//! What is drawn on a moved layer goes into the layer's own pixels, so it
//! moves with the layer on other frames. Input arrives in document pixels
//! and is taken back through the frame's move of the current layer.

use std::sync::Arc;

use ugu_core::command;
use ugu_core::document::{Document, LayerId, LayerKind};
use ugu_core::edit::{EditError, Outcome};
use ugu_core::keys::{self, Ease, Key, Pose};
use ugu_core::motion::frame_in_cycle;
use ugu_core::ops::Affine;
use ugu_core::selection::Selection;

use crate::Session;

/// A pose being dragged out, shown but not yet in the document.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Posing {
    pub layer: LayerId,
    pub pose: Pose,
}

impl Session {
    /// The frame shown, within the document's loop.
    pub fn cycle(&self) -> u32 {
        frame_in_cycle(self.frame, self.document().frames)
    }

    /// Where the current layer's pixels are shown on this frame; `None`
    /// where they are drawn.
    pub fn layer_transform(&self) -> Option<Affine> {
        self.document().transform(self.layer, self.cycle())
    }

    /// `position` in document pixels taken into the current layer's own
    /// pixels on this frame.
    pub fn to_layer(&self, position: [f64; 2]) -> [f64; 2] {
        match self
            .layer_transform()
            .and_then(|transform| transform.inverse())
        {
            Some(inverse) => inverse.apply(position),
            None => position,
        }
    }

    /// `selection`, in document pixels, as the current layer's own pixels
    /// it covers on this frame; `None` when it covers none of them.
    pub(crate) fn in_layer(&self, selection: &Arc<Selection>) -> Option<Arc<Selection>> {
        match self
            .layer_transform()
            .and_then(|transform| transform.inverse())
        {
            Some(inverse) => selection.transformed(inverse).map(Arc::new),
            None => Some(selection.clone()),
        }
    }

    /// `selection` in the current layer's own pixels as it is shown on this
    /// frame.
    pub(crate) fn shown_from_layer(&self, selection: Selection) -> Option<Selection> {
        match self.layer_transform() {
            Some(transform) => selection.transformed(transform),
            None => Some(selection),
        }
    }

    /// The pose of `id` on this frame: its keys', or the rest pose without
    /// any. A pose being dragged out counts.
    pub fn pose(&self, id: LayerId) -> Pose {
        if let Some(posing) = self.posing.filter(|posing| posing.layer == id) {
            return posing.pose;
        }
        self.document()
            .layer(id)
            .and_then(|layer| layer.keys().pose(self.cycle(), self.document().frames))
            .unwrap_or(Pose::REST)
    }

    /// The key of `id` on this frame, if it has one.
    pub fn key_here(&self, id: LayerId) -> Option<Key> {
        let cycle = self.cycle();
        self.document()
            .layer(id)
            .and_then(|layer| layer.keys().key(cycle).copied())
    }

    /// Gives `id` `pose` on this frame as one step: the key there changes,
    /// or one is added there. The first key also puts the pivot at
    /// `pivot`, in document pixels.
    pub fn set_pose(
        &mut self,
        id: LayerId,
        pose: Pose,
        pivot: [f64; 2],
    ) -> Result<Outcome, EditError> {
        self.posing = None;
        let cycle = self.cycle();
        self.update_layer(id, "Move layer on this frame", |layer| {
            let keys = layer.keys_mut();
            if keys.is_empty() {
                keys.pivot = pivot;
            }
            // A new key eases on as the key before it does.
            let ease = keys
                .key(cycle)
                .or_else(|| keys.keys.iter().rev().find(|key| key.frame < cycle))
                .map_or(Ease::Linear, |key| key.ease);
            keys.set(Key {
                frame: cycle,
                pose,
                ease,
            });
        })
    }

    /// Adds a key on this frame holding the pose `id` has now, or takes
    /// away the one there.
    pub fn toggle_key(&mut self, id: LayerId, pivot: [f64; 2]) -> Result<Outcome, EditError> {
        let cycle = self.cycle();
        if self.key_here(id).is_some() {
            return self.update_layer(id, "Remove key", |layer| {
                layer.keys_mut().remove(cycle);
            });
        }
        let pose = self.pose(id);
        self.set_pose(id, pose, pivot)
    }

    /// Takes away the key of `id` on `frame`.
    pub fn remove_key(&mut self, id: LayerId, frame: u32) -> Result<Outcome, EditError> {
        self.update_layer(id, "Remove key", |layer| {
            layer.keys_mut().remove(frame);
        })
    }

    /// Moves the key of `id` on `from` to `to`, in place of one there.
    pub fn move_key(&mut self, id: LayerId, from: u32, to: u32) -> Result<Outcome, EditError> {
        if from == to {
            return Ok(Outcome::NoChange);
        }
        self.update_layer(id, "Move key", |layer| {
            let keys = layer.keys_mut();
            if let Some(mut key) = keys.key(from).copied() {
                keys.remove(from);
                key.frame = to;
                keys.set(key);
            }
        })
    }

    /// How the key of `id` on `frame` eases on to the next.
    pub fn set_ease(&mut self, id: LayerId, frame: u32, ease: Ease) -> Result<Outcome, EditError> {
        self.update_layer(id, "Key easing", |layer| {
            let keys = layer.keys_mut();
            if let Some(mut key) = keys.key(frame).copied() {
                key.ease = ease;
                keys.set(key);
            }
        })
    }

    /// Puts what `id` turns, scales and flips about at `pivot`, in document
    /// pixels, keeping where it is shown on this frame.
    pub fn set_pivot(&mut self, id: LayerId, pivot: [f64; 2]) -> Result<Outcome, EditError> {
        self.update_layer(id, "Move pivot", |layer| {
            let keys = layer.keys_mut();
            let old = keys.pivot;
            keys.pivot = pivot;
            // A move about another pivot moves by as much more as the
            // pivots' images differ; each key keeps where it puts the layer.
            for key in &mut keys.keys {
                let shown = |centre: [f64; 2]| {
                    key.pose
                        .transform(centre)
                        .map_or([0.0, 0.0], |transform| transform.apply([0.0, 0.0]))
                };
                let (before, after) = (shown(old), shown(pivot));
                key.pose.offset[0] += before[0] - after[0];
                key.pose.offset[1] += before[1] - after[1];
            }
        })
    }

    /// Shows `pose` for `id` until `end_pose` keys it or `cancel_pose`
    /// drops it.
    pub fn drag_pose(&mut self, id: LayerId, pose: Pose) {
        self.posing = Some(Posing { layer: id, pose });
    }

    pub fn posing(&self) -> Option<Posing> {
        self.posing
    }

    /// Keys the pose being dragged out as one step.
    pub fn end_pose(&mut self, pivot: [f64; 2]) -> Result<Outcome, EditError> {
        let Some(posing) = self.posing.take() else {
            return Ok(Outcome::NoChange);
        };
        self.set_pose(posing.layer, posing.pose, pivot)
    }

    pub fn cancel_pose(&mut self) -> bool {
        self.posing.take().is_some()
    }

    /// The document as it is shown: with the pose being dragged out.
    pub fn shown_document(&self) -> std::borrow::Cow<'_, Document> {
        let Some(posing) = self.posing else {
            return std::borrow::Cow::Borrowed(self.document());
        };
        let mut document = self.document().clone();
        let cycle = self.cycle();
        if let Some(layer) = find_mut(&mut document.layers, posing.layer) {
            layer.keys_mut().set(Key {
                frame: cycle,
                pose: posing.pose,
                ease: Ease::Linear,
            });
        }
        std::borrow::Cow::Owned(document)
    }

    /// The frame of the key before (`-1`) or after (`1`) this one on the
    /// current layer or the groups holding it.
    pub fn neighbour_key(&self, direction: i32) -> Option<u32> {
        let cycle = self.cycle();
        let document = self.document();
        let mut path = document.ancestors(self.layer).unwrap_or_default();
        path.extend(document.layer(self.layer));
        let frames: Vec<u32> = path
            .iter()
            .flat_map(|layer| layer.keys().keys.iter().map(|key| key.frame))
            .collect();
        let forward = |frame: u32| (frame + document.frames - cycle) % document.frames;
        let pick = if direction < 0 {
            frames
                .iter()
                .filter(|frame| **frame != cycle)
                .max_by_key(|frame| forward(**frame))
        } else {
            frames
                .iter()
                .filter(|frame| **frame != cycle)
                .min_by_key(|frame| forward(**frame))
        };
        pick.copied()
    }

    /// The animation folder holding the current layer as a cel, or the
    /// current layer when it is a folder itself.
    pub fn folder(&self) -> Option<LayerId> {
        let document = self.document();
        if document.layer(self.layer)?.cels().is_some() {
            return Some(self.layer);
        }
        let (parent, _) = document.position(self.layer)?;
        let parent = parent?;
        document.layer(parent)?.cels().is_some().then_some(parent)
    }

    /// The cel `folder` shows on this frame.
    pub fn exposed(&self, folder: LayerId) -> Option<LayerId> {
        let cels = self.document().layer(folder)?.cels()?;
        keys::exposed(cels, self.cycle())
    }

    /// After the frame changed: a cel being edited gives way to the one its
    /// folder shows now, or to the folder on an empty frame.
    pub(crate) fn follow_cel(&mut self) {
        let Some(folder) = self.folder().filter(|folder| *folder != self.layer) else {
            return;
        };
        self.layer = self.exposed(folder).unwrap_or(folder);
    }

    /// Puts the current layer in a new animation folder that shows it
    /// from the first frame.
    pub fn make_animation_folder(&mut self) -> Result<Outcome, EditError> {
        self.settle()?;
        let name = (1..)
            .map(|number| format!("Animation {number}"))
            .find(|name| !crate::named(&self.document().layers, name))
            .expect("fewer layers than numbers");
        let (_, changes) = command::make_animation_folder(self.document(), self.layer, name)?;
        self.history_mut()?
            .edit("Make animation folder", |_| changes)
    }

    /// Adds an empty cel to the folder of the current layer, shown from
    /// this frame, and makes it current.
    pub fn new_cel(&mut self) -> Result<Outcome, EditError> {
        let Some(folder) = self.folder() else {
            return Ok(Outcome::NoChange);
        };
        self.settle()?;
        let name = format!("Cel {}", command::next_layer_id(self.document()).0);
        let (id, changes) = command::new_cel(self.document(), folder, self.cycle(), name)?;
        let outcome = self.history_mut()?.edit("New cel", |_| changes)?;
        if let Outcome::Committed(_) = outcome {
            self.layer = id;
        }
        Ok(outcome)
    }

    /// Shows `cel` from this frame in the current layer's folder, or
    /// nothing with `None`; the shown cel becomes current.
    pub fn expose(&mut self, cel: Option<LayerId>) -> Result<Outcome, EditError> {
        let Some(folder) = self.folder() else {
            return Ok(Outcome::NoChange);
        };
        let cycle = self.cycle();
        let outcome = self.update_layer(folder, "Show cel", |layer| {
            if let LayerKind::Group(group) = &mut layer.kind
                && let Some(cels) = group.cels.as_mut()
            {
                keys::expose(cels, cycle, cel);
            }
        })?;
        self.layer = cel.unwrap_or(folder);
        Ok(outcome)
    }

    /// Before drawing with a folder current: draws on the cel it shows, or
    /// adds one when it shows none.
    pub(crate) fn draw_on_cel(&mut self) -> Result<(), EditError> {
        let Some(folder) = self.folder().filter(|folder| *folder == self.layer) else {
            return Ok(());
        };
        match self.exposed(folder) {
            Some(cel) => self.layer = cel,
            None => {
                self.new_cel()?;
            }
        }
        Ok(())
    }
}

fn find_mut(
    layers: &mut [ugu_core::document::Layer],
    id: LayerId,
) -> Option<&mut ugu_core::document::Layer> {
    layers.iter_mut().find_map(|layer| {
        if layer.id == id {
            return Some(layer);
        }
        match &mut layer.kind {
            LayerKind::Group(group) => find_mut(&mut group.children, id),
            LayerKind::Paint(_) => None,
        }
    })
}

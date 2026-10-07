// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! Invertible changes and atomic commits.
//!
//! Every edit is a list of `Change`s. Applying a change returns the change
//! that undoes it. A commit applies the list in order and, if any change or
//! the final structure check fails, undoes what it applied, so a document
//! never keeps half an edit.

use crate::document::{Document, DocumentError, Layer, LayerId, LayerKind};
use crate::ops::{AssetId, MaskId, Op, Rgba8, StrokeId, Wobble};
use crate::store::{self, Asset, Mask, StoreError, Stroke};

/// Document-wide settings, changed together.
#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub canvas: [u32; 2],
    pub background: Rgba8,
    pub frames: u32,
    pub frames_per_second: f32,
    pub wobble: Wobble,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Change {
    InsertStroke(StrokeId, Stroke),
    RemoveStroke(StrokeId),
    InsertMask(MaskId, Mask),
    RemoveMask(MaskId),
    InsertAsset(AssetId, Asset),
    RemoveAsset(AssetId),
    /// Into a paint layer's top-level operations.
    InsertOp {
        layer: LayerId,
        index: usize,
        op: Op,
    },
    RemoveOp {
        layer: LayerId,
        index: usize,
    },
    /// `parent` is a group, or `None` for the top level; `index` counts from
    /// the bottom.
    InsertLayer {
        parent: Option<LayerId>,
        index: usize,
        layer: Layer,
    },
    RemoveLayer(LayerId),
    /// Replaces the layer with the same id, children included.
    ReplaceLayer(Layer),
    SetSettings(Settings),
}

#[derive(Clone, Debug, PartialEq)]
pub enum EditError {
    NoSuchLayer(LayerId),
    NotPaintLayer(LayerId),
    NotGroup(LayerId),
    Index,
    IdInUse,
    NoSuchData,
    /// The layers to merge differ in visibility or reference use, or there is
    /// no paint layer below.
    CannotMerge,
    Merge(crate::ops::MergeRefusal),
    Store(StoreError),
    Document(DocumentError),
}

impl std::fmt::Display for EditError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoSuchLayer(id) => write!(f, "there is no layer {}", id.0),
            Self::NotPaintLayer(id) => write!(f, "layer {} is not a paint layer", id.0),
            Self::NotGroup(id) => write!(f, "layer {} is not a group", id.0),
            Self::Index => write!(f, "the position is out of range"),
            Self::IdInUse => write!(f, "the id is already in use"),
            Self::NoSuchData => write!(f, "the stroke, mask or image is not stored"),
            Self::CannotMerge => write!(f, "the layer cannot be merged down"),
            Self::Merge(refusal) => write!(f, "the layer cannot be merged down: {refusal:?}"),
            Self::Store(error) => error.fmt(f),
            Self::Document(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for EditError {}

#[derive(Debug, PartialEq)]
pub enum Outcome {
    /// The changes that undo the commit, in the order to apply them.
    Committed(Vec<Change>),
    /// Nothing changed: not an error and nothing to undo.
    NoChange,
}

/// Applies `changes` in order, or none of them.
pub fn commit(document: &mut Document, changes: Vec<Change>) -> Result<Outcome, EditError> {
    let mut undo = Vec::with_capacity(changes.len());
    for change in changes {
        match change.apply(document) {
            Ok(Some(inverse)) => undo.push(inverse),
            Ok(None) => {}
            Err(error) => {
                roll_back(document, undo);
                return Err(error);
            }
        }
    }
    if undo.is_empty() {
        return Ok(Outcome::NoChange);
    }
    if let Err(error) = document.check_structure() {
        roll_back(document, undo);
        return Err(EditError::Document(error));
    }
    undo.reverse();
    Ok(Outcome::Committed(undo))
}

fn roll_back(document: &mut Document, mut undo: Vec<Change>) {
    while let Some(inverse) = undo.pop() {
        // An inverse puts back exactly what its change took or removes what
        // it added, so it cannot fail.
        if let Err(error) = inverse.apply(document) {
            unreachable!("undoing an applied change failed: {error}");
        }
    }
}

impl Document {
    pub fn settings(&self) -> Settings {
        Settings {
            canvas: self.canvas,
            background: self.background,
            frames: self.frames,
            frames_per_second: self.frames_per_second,
            wobble: self.wobble,
        }
    }

    pub fn layer(&self, id: LayerId) -> Option<&Layer> {
        find(&self.layers, id)
    }

    /// The container holding `id` (`None` for the top level) and its index.
    pub fn position(&self, id: LayerId) -> Option<(Option<LayerId>, usize)> {
        position(&self.layers, None, id)
    }

    fn container_mut(&mut self, parent: Option<LayerId>) -> Result<&mut Vec<Layer>, EditError> {
        let Some(parent) = parent else {
            return Ok(&mut self.layers);
        };
        match find_mut(&mut self.layers, parent).map(|layer| &mut layer.kind) {
            Some(LayerKind::Group(group)) => Ok(&mut group.children),
            Some(LayerKind::Paint(_)) => Err(EditError::NotGroup(parent)),
            None => Err(EditError::NoSuchLayer(parent)),
        }
    }

    fn paint_ops_mut(&mut self, id: LayerId) -> Result<&mut Vec<Op>, EditError> {
        match find_mut(&mut self.layers, id).map(|layer| &mut layer.kind) {
            Some(LayerKind::Paint(paint)) => Ok(&mut paint.ops),
            Some(LayerKind::Group(_)) => Err(EditError::NotPaintLayer(id)),
            None => Err(EditError::NoSuchLayer(id)),
        }
    }
}

fn find(layers: &[Layer], id: LayerId) -> Option<&Layer> {
    layers.iter().find_map(|layer| {
        if layer.id == id {
            return Some(layer);
        }
        match &layer.kind {
            LayerKind::Group(group) => find(&group.children, id),
            LayerKind::Paint(_) => None,
        }
    })
}

fn find_mut(layers: &mut [Layer], id: LayerId) -> Option<&mut Layer> {
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

fn position(
    layers: &[Layer],
    parent: Option<LayerId>,
    id: LayerId,
) -> Option<(Option<LayerId>, usize)> {
    layers.iter().enumerate().find_map(|(index, layer)| {
        if layer.id == id {
            return Some((parent, index));
        }
        match &layer.kind {
            LayerKind::Group(group) => position(&group.children, Some(layer.id), id),
            LayerKind::Paint(_) => None,
        }
    })
}

impl Change {
    /// Applies the change and returns its inverse, or `None` when it changed
    /// nothing.
    fn apply(self, document: &mut Document) -> Result<Option<Change>, EditError> {
        let store = &mut document.store;
        Ok(Some(match self {
            Self::InsertStroke(id, stroke) => {
                if store.strokes.contains_key(&id) {
                    return Err(EditError::IdInUse);
                }
                store::check_stroke(id, &stroke).map_err(EditError::Store)?;
                store.strokes.insert(id, stroke);
                Self::RemoveStroke(id)
            }
            Self::RemoveStroke(id) => {
                let stroke = store.strokes.remove(&id).ok_or(EditError::NoSuchData)?;
                Self::InsertStroke(id, stroke)
            }
            Self::InsertMask(id, mask) => {
                if store.masks.contains_key(&id) {
                    return Err(EditError::IdInUse);
                }
                store::check_mask(id, &mask).map_err(EditError::Store)?;
                store.masks.insert(id, mask);
                Self::RemoveMask(id)
            }
            Self::RemoveMask(id) => {
                let mask = store.masks.remove(&id).ok_or(EditError::NoSuchData)?;
                Self::InsertMask(id, mask)
            }
            Self::InsertAsset(id, asset) => {
                if store.assets.contains_key(&id) {
                    return Err(EditError::IdInUse);
                }
                store::check_asset(id, &asset).map_err(EditError::Store)?;
                store.assets.insert(id, asset);
                Self::RemoveAsset(id)
            }
            Self::RemoveAsset(id) => {
                let asset = store.assets.remove(&id).ok_or(EditError::NoSuchData)?;
                Self::InsertAsset(id, asset)
            }
            Self::InsertOp { layer, index, op } => {
                let ops = document.paint_ops_mut(layer)?;
                if index > ops.len() {
                    return Err(EditError::Index);
                }
                ops.insert(index, op);
                Self::RemoveOp { layer, index }
            }
            Self::RemoveOp { layer, index } => {
                let ops = document.paint_ops_mut(layer)?;
                if index >= ops.len() {
                    return Err(EditError::Index);
                }
                let op = ops.remove(index);
                Self::InsertOp { layer, index, op }
            }
            Self::InsertLayer {
                parent,
                index,
                layer,
            } => {
                let container = document.container_mut(parent)?;
                if index > container.len() {
                    return Err(EditError::Index);
                }
                let id = layer.id;
                container.insert(index, layer);
                Self::RemoveLayer(id)
            }
            Self::RemoveLayer(id) => {
                let (parent, index) = document.position(id).ok_or(EditError::NoSuchLayer(id))?;
                let layer = document.container_mut(parent)?.remove(index);
                Self::InsertLayer {
                    parent,
                    index,
                    layer,
                }
            }
            Self::ReplaceLayer(layer) => {
                let slot = find_mut(&mut document.layers, layer.id)
                    .ok_or(EditError::NoSuchLayer(layer.id))?;
                if *slot == layer {
                    return Ok(None);
                }
                Self::ReplaceLayer(std::mem::replace(slot, layer))
            }
            Self::SetSettings(settings) => {
                let old = document.settings();
                if old == settings {
                    return Ok(None);
                }
                document.canvas = settings.canvas;
                document.background = settings.background;
                document.frames = settings.frames;
                document.frames_per_second = settings.frames_per_second;
                document.wobble = settings.wobble;
                Self::SetSettings(old)
            }
        }))
    }
}

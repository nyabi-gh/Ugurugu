// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#pragma once

#include "document/Document.hpp"

namespace ugurugu
{
namespace DocumentOperations
{

QSize initialCanvasSize(
    const QVector<Stroke> &operations, const QSize &fallback);

// Normalizes missing layer epochs and validates the ordered framebuffer
// operations without mutating the caller on failure.
bool normalizeAndValidate(Document &document);

// True when the layer reaches the frame: it is visible, not fully
// transparent, and no group holding it is hidden either. A layer's own flags
// do not answer this once it sits inside a group, and the drawing tools need
// the answer to refuse work the artist could not see.
bool isLayerRenderable(const Document &document, const Layer &layer);

// The document that renders one layer alone over transparency, detached from
// its group, clipping and blend so nothing else has to exist for it to
// composite. Flood-fill reference images read it.
Document isolatedLayerDocument(const Document &document, const Layer &layer);

}

}

// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#pragma once

#include "document/Document.hpp"

#include <QBrush>
#include <QImage>
#include <QPainterPath>
#include <QPointF>
#include <QTransform>

class QPainter;

namespace ugurugu
{
namespace canvas_detail
{

// Viewport and selection-overlay geometry shared by the CanvasWidget
// translation units. Everything here is free of widget state so it can be
// reasoned about — and changed — without touching interaction code.

constexpr int checkerSize = 12;
constexpr qreal canvasMargin = 32.0;
constexpr qreal minimumZoom = 0.01;
constexpr qreal maximumZoom = 16.0;
constexpr qreal keyboardZoomStep = 1.25;
constexpr qreal dragZoomDoublingDistance = 120.0;
constexpr qreal canvasRotationStep = 5.0;
constexpr qreal dragRotationDegreesPerPixel = 0.5;
constexpr int zoomRenderDelayMilliseconds = 80;

const QBrush &checkerBrush();

// True when the transform differs from the identity by less than what a
// device pixel can show, so callers can skip a no-op resample.
bool fuzzyIdentity(const QTransform &transform);

qreal pointDistance(const QPointF &a, const QPointF &b);

qreal normalizedRotation(qreal degrees);

qreal uniformScale(const QTransform &transform);

bool documentHasStrokes(const Document &document);

// Traces the boundary of the set pixels in an 8-bit mask. The path is in
// document coordinates and may contain several disjoint subpaths.
QPainterPath outlinePath(const QImage &mask);

// Marching-ants stroke: two offset dashed passes so the outline stays visible
// over both light and dark content.
void drawSelectionPath(
    QPainter &painter, const QPainterPath &path, qreal dashOffset);

}

}

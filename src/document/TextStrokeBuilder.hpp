// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyattic)

#pragma once

#include "document/Document.hpp"

#include <QFont>
#include <QPainterPath>
#include <QVector>

namespace ugurugu::TextStrokeBuilder
{

struct Options
{
    QString text;
    QFont font;
    // Document position of the laid-out block's top-left corner.
    QPointF anchor;
    QColor color = Qt::black;
    qreal outlineWidth = 6.0;
    BrushSettings brush;
    bool filled = false;
    QSize canvasSize;
    // Per-stroke seeds are derived deterministically from this value, so the
    // same options always produce byte-identical strokes.
    quint64 baseSeed = 0;
};

// Glyph outlines laid out with the block's top-left corner at the origin.
// Newlines start further lines one font line spacing apart.
QPainterPath layoutPath(const QString &text, const QFont &font);

// Converts the text into ordinary document strokes: one Paint stroke per
// glyph contour, preceded by one Fill stroke covering the glyph interiors
// when filled is set. Outline points are clamped into the canvas because
// DocumentController::addStroke rejects strokes with any point outside it.
QVector<Stroke> build(const Options &options);

// Font-independent entry point for headless clients which supply glyph paths.
// The path uses the same top-left-relative coordinates as layoutPath().
QVector<Stroke> buildFromPath(QPainterPath path, const Options &options);

}

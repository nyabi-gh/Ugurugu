// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#pragma once

#include "document/Document.hpp"

#include <QHash>
#include <QImage>
#include <QSize>
#include <QTransform>

#include <optional>

namespace ugurugu
{

QImage transformedMask(
    const QImage &source, const QSize &targetSize, const QTransform &transform);

bool transformMask(QImage &mask,
    const QSize &targetSize,
    const QTransform &transform,
    QHash<qint64, QImage> &cache);

QImage maskedPart(
    const QImage &source, const QImage &selection, bool insideSelection);

bool maskHasContent(const QImage &mask);

bool maskHasContent(
    const QImage &mask, const std::optional<QRect> &visibilityClip);

bool masksIntersect(const QImage &first,
    const QImage &second,
    const std::optional<QRect> &visibilityClip = std::nullopt);

// Returns a bitmap representation of clipMask intersected with
// visibilityClip. A present null QImage means unrestricted/full visibility;
// std::nullopt means allocation or validation failed.
std::optional<QImage> materializedVisibilityMask(
    const Stroke &stroke, const QSize &canvasSize);

// Removes redundant full-canvas restrictions and reports whether the stroke
// can affect at least one canvas pixel.
bool canonicalizeStrokeVisibility(Stroke &stroke, const QSize &canvasSize);

}

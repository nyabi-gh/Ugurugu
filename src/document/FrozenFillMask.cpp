// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "document/FrozenFillMask.hpp"

#include "document/DocumentLimits.hpp"
#include "document/SelectionOperation.hpp"

#include <QPainter>
#include <QPainterPath>

#include <algorithm>
#include <cmath>
#include <iterator>

namespace ugurugu::FrozenFillMask
{

namespace
{

bool validCanvasSize(const QSize &size)
{
    return size.width() >= DocumentLimits::minimumCanvasEdge
           && size.height() >= DocumentLimits::minimumCanvasEdge
           && size.width() <= DocumentLimits::maximumCanvasEdge
           && size.height() <= DocumentLimits::maximumCanvasEdge;
}

bool validPoint(const QPointF &point)
{
    return std::isfinite(point.x()) && std::isfinite(point.y())
           && std::abs(point.x())
                  <= DocumentLimits::maximumStoredCoordinateMagnitude
           && std::abs(point.y())
                  <= DocumentLimits::maximumStoredCoordinateMagnitude;
}

}

std::optional<QImage> fromPolygon(
    const QSize &canvasSize, const QVector<QPointF> &polygon)
{
    if (!validCanvasSize(canvasSize) || polygon.size() < 3
        || polygon.size() > DocumentLimits::maximumPointsPerStroke
        || !std::all_of(polygon.cbegin(), polygon.cend(), validPoint))
    {
        return std::nullopt;
    }

    QPainterPath path;
    path.setFillRule(Qt::OddEvenFill);
    path.moveTo(polygon.first());
    for (auto point = std::next(polygon.cbegin()); point != polygon.cend();
        ++point)
    {
        path.lineTo(*point);
    }
    path.closeSubpath();

    QImage mask(canvasSize, QImage::Format_Grayscale8);
    if (mask.isNull())
    {
        return std::nullopt;
    }
    mask.fill(0);
    QPainter painter(&mask);
    if (!painter.isActive())
    {
        return std::nullopt;
    }
    painter.setRenderHint(QPainter::Antialiasing, false);
    painter.setCompositionMode(QPainter::CompositionMode_Source);
    painter.fillPath(path, Qt::white);
    painter.end();
    return mask;
}

std::optional<PackedMaskRegion> packedFromPolygon(
    const QSize &canvasSize, const QVector<QPointF> &polygon)
{
    const std::optional<QImage> mask = fromPolygon(canvasSize, polygon);
    return mask ? packBinaryMask(*mask) : std::nullopt;
}

}

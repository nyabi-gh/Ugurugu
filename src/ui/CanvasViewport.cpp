// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "ui/CanvasViewport.hpp"

#include "document/SelectionOutline.hpp"
#include "ui/Theme.hpp"

#include <QList>
#include <QPainter>
#include <QPainterPath>
#include <QPolygonF>

#include <cmath>

namespace ugurugu
{
namespace canvas_detail
{

const QBrush &checkerBrush()
{
    static const QBrush brush = []()
    {
        QImage tile(checkerSize * 2, checkerSize * 2, QImage::Format_RGB32);
        tile.fill(QColor(238, 238, 238));
        QPainter painter(&tile);
        painter.fillRect(
            checkerSize, 0, checkerSize, checkerSize, QColor(210, 210, 210));
        painter.fillRect(
            0, checkerSize, checkerSize, checkerSize, QColor(210, 210, 210));
        return QBrush(tile);
    }();
    return brush;
}

bool fuzzyIdentity(const QTransform &transform)
{
    return qFuzzyCompare(transform.m11(), 1.0) && qFuzzyIsNull(transform.m12())
           && qFuzzyIsNull(transform.m13()) && qFuzzyIsNull(transform.m21())
           && qFuzzyCompare(transform.m22(), 1.0)
           && qFuzzyIsNull(transform.m23()) && qFuzzyIsNull(transform.m31())
           && qFuzzyIsNull(transform.m32())
           && qFuzzyCompare(transform.m33(), 1.0);
}

qreal pointDistance(const QPointF &a, const QPointF &b)
{
    return std::hypot(a.x() - b.x(), a.y() - b.y());
}

qreal normalizedRotation(qreal degrees)
{
    qreal normalized = std::fmod(degrees + 180.0, 360.0);
    if (normalized < 0.0)
    {
        normalized += 360.0;
    }
    normalized -= 180.0;
    return qFuzzyIsNull(normalized) ? 0.0 : normalized;
}

qreal uniformScale(const QTransform &transform)
{
    return std::hypot(transform.m11(), transform.m12());
}

bool documentHasStrokes(const Document &document)
{
    for (const Layer &layer : document.layers)
    {
        if (!layer.strokes.isEmpty())
        {
            return true;
        }
    }
    return false;
}

QPainterPath outlinePath(const QImage &mask)
{
    QPainterPath path;
    for (const QPolygonF &contour : selectionOutline(mask))
    {
        if (contour.isEmpty())
        {
            continue;
        }
        path.moveTo(contour.first());
        for (int index = 1; index < contour.size(); ++index)
        {
            path.lineTo(contour.at(index));
        }
        if (contour.first() == contour.last())
        {
            path.closeSubpath();
        }
    }
    return path;
}

void drawSelectionPath(
    QPainter &painter, const QPainterPath &path, qreal dashOffset)
{
    QPen lightPen(QColor(255, 255, 255, 235), 1.8);
    lightPen.setCosmetic(true);
    lightPen.setJoinStyle(Qt::MiterJoin);
    painter.setPen(lightPen);
    painter.setBrush(Qt::NoBrush);
    painter.drawPath(path);

    QPen darkPen(QColor(20, 20, 20, 245), 1.0);
    darkPen.setCosmetic(true);
    darkPen.setJoinStyle(Qt::MiterJoin);
    darkPen.setDashPattern({4.0, 4.0});
    darkPen.setDashOffset(dashOffset);
    painter.setPen(darkPen);
    painter.drawPath(path);
}

}

}

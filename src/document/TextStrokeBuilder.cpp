// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyattic)

#include "document/TextStrokeBuilder.hpp"

#include "document/DocumentLimits.hpp"
#include "document/SelectionOperation.hpp"

#include <QFontMetricsF>
#include <QPainter>

#include <algorithm>
#include <cmath>

namespace ugurugu::TextStrokeBuilder
{

namespace
{

quint64 derivedSeed(quint64 base, quint64 index)
{
    quint64 value = base + (index + 1ULL) * 0x9e3779b97f4a7c15ULL;
    value = (value ^ (value >> 30U)) * 0xbf58476d1ce4e5b9ULL;
    value = (value ^ (value >> 27U)) * 0x94d049bb133111ebULL;
    return value ^ (value >> 31U);
}

qreal pointDistance(const QPointF &first, const QPointF &second)
{
    return std::hypot(second.x() - first.x(), second.y() - first.y());
}

QPointF clampedPoint(const QPointF &point, const QSize &canvasSize)
{
    return QPointF(
        std::clamp(point.x(), 0.0, static_cast<qreal>(canvasSize.width())),
        std::clamp(point.y(), 0.0, static_cast<qreal>(canvasSize.height())));
}

std::optional<Stroke> fillStroke(
    const QPainterPath &path, const Options &options, qreal strokeWidth)
{
    QImage coverage(options.canvasSize, QImage::Format_Grayscale8);
    if (coverage.isNull())
    {
        return std::nullopt;
    }
    coverage.fill(0);
    QPainter painter(&coverage);
    painter.setRenderHint(QPainter::Antialiasing, false);
    painter.fillPath(path, Qt::white);
    painter.end();
    const std::optional<PackedMaskRegion> packed = packBinaryMask(coverage);
    if (!packed)
    {
        return std::nullopt;
    }
    Stroke fill;
    fill.seed = derivedSeed(options.baseSeed, 0);
    fill.mode = StrokeMode::Fill;
    fill.color = options.color;
    fill.width = strokeWidth;
    fill.brush = options.brush;
    fill.fillCoverage = *packed;
    fill.points.append(
        {clampedPoint(QPointF(packed->bounds.center()), options.canvasSize),
            1.0});
    return fill;
}

QVector<StrokePoint> contourPoints(
    const QPolygonF &polygon, const QSize &canvasSize, qreal overlapLength)
{
    QVector<StrokePoint> points;
    points.reserve(polygon.size() + 8);
    for (const QPointF &vertex : polygon)
    {
        if (!std::isfinite(vertex.x()) || !std::isfinite(vertex.y()))
        {
            continue;
        }
        const QPointF clamped = clampedPoint(vertex, canvasSize);
        if (!points.isEmpty()
            && pointDistance(clamped, points.constLast().position) < 0.01)
        {
            continue;
        }
        points.append({clamped, 1.0});
    }
    if (points.size() < 3)
    {
        return {};
    }
    if (pointDistance(points.constFirst().position, points.constLast().position)
        > 0.01)
    {
        points.append(points.constFirst());
    }
    // Overshooting the ring start hides the seam: the renderer's endpoint
    // tangents are one-sided and its midpoint smoothing treats the junction
    // as a corner, so a contour that merely closes shows a notch there.
    const qsizetype ringSize = points.size();
    qreal walked = 0.0;
    for (qsizetype index = 1;
        index < ringSize && walked < overlapLength
        && points.size() < DocumentLimits::maximumPointsPerStroke;
        ++index)
    {
        walked +=
            pointDistance(points[index].position, points[index - 1].position);
        points.append(points[index]);
    }
    return points;
}

}

QPainterPath layoutPath(const QString &text, const QFont &font)
{
    QPainterPath path;
    // Winding fill keeps overlapping contours inside one glyph solid; the
    // odd-even default would punch holes wherever they cross.
    path.setFillRule(Qt::WindingFill);
    const QFontMetricsF metrics(font);
    qreal baseline = metrics.ascent();
    const QStringList lines = text.split(QLatin1Char('\n'));
    for (const QString &line : lines)
    {
        if (!line.isEmpty())
        {
            path.addText(QPointF(0.0, baseline), font, line);
        }
        baseline += metrics.lineSpacing();
    }
    return path;
}

QVector<Stroke> build(const Options &options)
{
    QVector<Stroke> strokes;
    if (options.canvasSize.isEmpty() || options.text.trimmed().isEmpty())
    {
        return strokes;
    }
    return buildFromPath(layoutPath(options.text, options.font), options);
}

QVector<Stroke> buildFromPath(QPainterPath path, const Options &options)
{
    QVector<Stroke> strokes;
    if (options.canvasSize.isEmpty() || path.isEmpty())
    {
        return strokes;
    }
    path.translate(options.anchor);

    const qreal strokeWidth = std::clamp(options.outlineWidth,
        DocumentLimits::minimumStrokeWidth,
        DocumentLimits::maximumStrokeWidth);
    if (options.filled)
    {
        if (std::optional<Stroke> fill = fillStroke(path, options, strokeWidth))
        {
            strokes.append(std::move(*fill));
        }
    }

    const qreal overlapLength = std::clamp(strokeWidth * 0.55, 2.0, 5.0);
    const QList<QPolygonF> polygons = path.toSubpathPolygons();
    quint64 seedIndex = 1;
    for (const QPolygonF &polygon : polygons)
    {
        const QVector<StrokePoint> points =
            contourPoints(polygon, options.canvasSize, overlapLength);
        if (points.isEmpty())
        {
            continue;
        }
        Stroke outline;
        outline.seed = derivedSeed(options.baseSeed, seedIndex++);
        outline.mode = StrokeMode::Paint;
        outline.color = options.color;
        outline.width = strokeWidth;
        outline.brush = options.brush;
        outline.points = points;
        strokes.append(std::move(outline));
    }
    return strokes;
}

}

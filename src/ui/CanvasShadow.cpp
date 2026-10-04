// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "ui/CanvasShadow.hpp"

#include <QGradient>
#include <QPainter>
#include <QPainterPath>
#include <QtMath>

#include <algorithm>
#include <array>
#include <cmath>
#include <limits>
#include <vector>

namespace ugurugu::CanvasShadow
{

namespace
{

constexpr int passCount = 14;
constexpr QPointF shift(0.0, 2.0);

// Pass `step` is a stroke `step` px either side of the outline whose
// antialiased edge ramps over a pixel centred there, so the stacked alpha is
// piecewise linear with knots half a pixel past each pass edge.
constexpr qreal reach = passCount - 0.5;

// The canvas edge sits at most the shift inside the shifted outline, and the
// clip that cuts the canvas out is a pixel-aligned region.
constexpr qreal insideDepth = 3.0;

qreal stackedAlpha(int firstStep)
{
    qreal transparency = 1.0;
    for (int step = firstStep; step <= passCount; ++step)
    {
        transparency *= 1.0 - 0.020 * (1.0 - qreal(step) / passCount);
    }
    return 1.0 - transparency;
}

const QGradientStops &profile()
{
    static const QGradientStops stops = []()
    {
        const auto shade = [](qreal alpha)
        {
            QColor color(Qt::black);
            color.setAlphaF(static_cast<float>(alpha));
            return color;
        };
        QGradientStops built;
        built.append({0.0, shade(stackedAlpha(1))});
        built.append({0.5 / reach, shade(stackedAlpha(1))});
        for (int step = 1; step < passCount; ++step)
        {
            built.append({(step + 0.5) / reach, shade(stackedAlpha(step + 1))});
        }
        return built;
    }();
    return stops;
}

QPointF unitNormal(const QPointF &from, const QPointF &to, qreal orientation)
{
    const QPointF direction = to - from;
    const qreal length = std::hypot(direction.x(), direction.y());
    return QPointF(direction.y(), -direction.x()) * (orientation / length);
}

qreal screenAngle(const QPointF &direction)
{
    return qRadiansToDegrees(std::atan2(-direction.y(), direction.x()));
}

void fillBand(QPainter &painter,
    const std::array<QPointF, 4> &corners,
    const QPointF &start,
    const QPointF &end,
    const QRectF &visibleArea)
{
    const QPolygonF band(corners.begin(), corners.end());
    if (!band.boundingRect().intersects(visibleArea))
    {
        return;
    }
    QLinearGradient gradient(start, end);
    gradient.setStops(profile());
    painter.setBrush(gradient);
    painter.drawPolygon(band);
}

}

void paint(
    QPainter &painter, const QPolygonF &outline, const QRectF &visibleArea)
{
    std::vector<QPointF> vertices;
    for (const QPointF &point : outline)
    {
        const QPointF shifted = point + shift;
        if (vertices.empty() || vertices.back() != shifted)
        {
            vertices.push_back(shifted);
        }
    }
    if (vertices.size() > 1 && vertices.front() == vertices.back())
    {
        vertices.pop_back();
    }
    const std::size_t count = vertices.size();
    if (count < 3)
    {
        return;
    }

    qreal doubledArea = 0.0;
    qreal shortestEdge = std::numeric_limits<qreal>::max();
    for (std::size_t index = 0; index < count; ++index)
    {
        const QPointF &from = vertices[index];
        const QPointF &to = vertices[(index + 1) % count];
        doubledArea += from.x() * to.y() - to.x() * from.y();
        shortestEdge = std::min(
            shortestEdge, std::hypot(to.x() - from.x(), to.y() - from.y()));
    }
    if (std::abs(doubledArea) < 1e-6)
    {
        return;
    }
    const qreal orientation = doubledArea > 0.0 ? 1.0 : -1.0;
    const qreal depth = std::min(insideDepth, shortestEdge * 0.5);

    std::vector<QPointF> normals(count);
    for (std::size_t index = 0; index < count; ++index)
    {
        normals[index] = unitNormal(
            vertices[index], vertices[(index + 1) % count], orientation);
    }
    // Inside the outline the nearest edge changes along each corner's
    // bisector, so the inner bands are cut there and do not overlap.
    std::vector<QPointF> miters(count);
    for (std::size_t index = 0; index < count; ++index)
    {
        const QPointF &before = normals[(index + count - 1) % count];
        const QPointF &after = normals[index];
        const qreal cosine = QPointF::dotProduct(before, after);
        miters[index] = (before + after) / (1.0 + cosine);
    }

    painter.save();
    // Bands meet on shared edges; antialiasing would blend both into a seam.
    painter.setRenderHint(QPainter::Antialiasing, false);
    painter.setPen(Qt::NoPen);
    for (std::size_t index = 0; index < count; ++index)
    {
        const std::size_t next = (index + 1) % count;
        const QPointF &from = vertices[index];
        const QPointF &to = vertices[next];
        const QPointF outward = normals[index] * reach;
        fillBand(painter,
            {from, to, to + outward, from + outward},
            from,
            from + outward,
            visibleArea);
        fillBand(painter,
            {from, to, to - miters[next] * depth, from - miters[index] * depth},
            from,
            from - outward,
            visibleArea);

        const QRectF cornerBounds(
            to.x() - reach, to.y() - reach, reach * 2.0, reach * 2.0);
        if (!cornerBounds.intersects(visibleArea))
        {
            continue;
        }
        const qreal startAngle = screenAngle(normals[index]);
        qreal sweep = screenAngle(normals[next]) - startAngle;
        if (sweep > 180.0)
        {
            sweep -= 360.0;
        }
        else if (sweep <= -180.0)
        {
            sweep += 360.0;
        }
        QPainterPath corner(to);
        corner.arcTo(cornerBounds, startAngle, sweep);
        corner.closeSubpath();
        QRadialGradient gradient(to, reach);
        gradient.setStops(profile());
        painter.setBrush(gradient);
        painter.drawPath(corner);
    }
    painter.restore();
}

}

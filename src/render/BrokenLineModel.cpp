// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "render/BrokenLineModel.hpp"

#include "render/DeterministicNoise.hpp"

#include <algorithm>
#include <cmath>
#include <limits>

namespace ugurugu::BrokenLineModel
{

namespace
{

constexpr quint64 visibilityChannel = 0xd2b74407ULL;

}

QVector<quint8> visibleSegments(const QVector<StrokePoint> &points,
    quint64 seed,
    int pose,
    qreal breakAmount,
    qreal breakRange)
{
    if (points.size() < 2 || !std::isfinite(breakAmount)
        || !std::isfinite(breakRange) || breakAmount < 0.0 || breakAmount > 1.0
        || breakRange <= 0.0)
    {
        return {};
    }

    QVector<quint8> result(points.size() - 1, 1);
    if (breakAmount <= 0.0)
    {
        return result;
    }
    if (breakAmount >= 1.0)
    {
        std::fill(result.begin(), result.end(), 0);
        return result;
    }

    qreal arcLength = 0.0;
    for (qsizetype index = 0; index < result.size(); ++index)
    {
        const QPointF delta =
            points[index + 1].position - points[index].position;
        const qreal segmentLength = std::hypot(delta.x(), delta.y());
        const qreal midpoint = arcLength + segmentLength * 0.5;
        const qreal cellCoordinate = std::floor(midpoint / breakRange);
        const long double extendedCell =
            static_cast<long double>(cellCoordinate);
        if (!std::isfinite(segmentLength) || !std::isfinite(midpoint)
            || !std::isfinite(cellCoordinate)
            || extendedCell < static_cast<long double>(
                   std::numeric_limits<qint64>::min())
            || extendedCell >= static_cast<long double>(
                   std::numeric_limits<qint64>::max()))
        {
            return {};
        }
        const qint64 cell = static_cast<qint64>(cellCoordinate);
        result[index] = static_cast<quint8>(
            DeterministicNoise::unitValue(seed, pose, cell, visibilityChannel)
            >= breakAmount);
        arcLength += segmentLength;
        if (!std::isfinite(arcLength))
        {
            return {};
        }
    }
    return result;
}

QVector<VisibleRun> visibleRuns(const QVector<quint8> &segments)
{
    QVector<VisibleRun> result;
    qsizetype index = 0;
    while (index < segments.size())
    {
        while (index < segments.size() && segments[index] == 0)
        {
            ++index;
        }
        const qsizetype first = index;
        while (index < segments.size() && segments[index] != 0)
        {
            ++index;
        }
        if (first < index)
        {
            result.append({first, index});
        }
    }
    return result;
}

}

// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "document/DocumentBudget.hpp"

#include "document/DocumentLimits.hpp"

#include <QSet>

namespace ugurugu
{
namespace DocumentBudget
{

qsizetype totalPointCount(const Document &document)
{
    qsizetype count = 0;
    for (const Layer &layer : document.layers)
    {
        for (const Stroke &stroke : layer.strokes)
        {
            if (stroke.points.size()
                > DocumentLimits::maximumTotalPoints - count)
            {
                return DocumentLimits::maximumTotalPoints + 1;
            }
            count += stroke.points.size();
        }
    }
    return count;
}

qsizetype totalStrokeCount(const Document &document)
{
    qsizetype count = 0;
    for (const Layer &layer : document.layers)
    {
        if (layer.strokes.size() > DocumentLimits::maximumTotalStrokes - count)
        {
            return DocumentLimits::maximumTotalStrokes + 1;
        }
        count += layer.strokes.size();
    }
    return count;
}

quint64 distinctClipMaskBytes(const Document &document)
{
    QSet<qint64> seen;
    QSet<quintptr> seenPackedBackings;
    quint64 bytes = 0;
    for (const Layer &layer : document.layers)
    {
        for (const Stroke &stroke : layer.strokes)
        {
            for (const QImage *mask : {&stroke.clipMask, &stroke.fillMask})
            {
                if (mask->isNull() || seen.contains(mask->cacheKey()))
                {
                    continue;
                }
                seen.insert(mask->cacheKey());
                const quint64 maskBytes = mask->sizeInBytes();
                if (maskBytes
                    > DocumentLimits::maximumDistinctClipMaskBytes - bytes)
                {
                    return DocumentLimits::maximumDistinctClipMaskBytes + 1;
                }
                bytes += maskBytes;
            }
            const auto registerPacked = [&bytes, &seenPackedBackings](
                                            const QByteArray &packed)
            {
                const quintptr backing =
                    reinterpret_cast<quintptr>(packed.constData());
                if (seenPackedBackings.contains(backing))
                {
                    return true;
                }
                const quint64 packedBytes = static_cast<quint64>(packed.size());
                if (packedBytes
                    > DocumentLimits::maximumDistinctClipMaskBytes - bytes)
                {
                    return false;
                }
                seenPackedBackings.insert(backing);
                bytes += packedBytes;
                return true;
            };
            const std::optional<PixelSelectionOp> &operation =
                stroke.pixelSelectionOp;
            if (operation.has_value()
                && !registerPacked(operation.value().packedMask))
            {
                return DocumentLimits::maximumDistinctClipMaskBytes + 1;
            }
            const std::optional<PackedMaskRegion> &fillCoverage =
                stroke.fillCoverage;
            if (fillCoverage.has_value()
                && !registerPacked(fillCoverage.value().packedMask))
            {
                return DocumentLimits::maximumDistinctClipMaskBytes + 1;
            }
        }
    }
    return bytes;
}

}

}

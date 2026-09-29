// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "io/AnimationExportPolicy.hpp"

#include "app/MemoryBudget.hpp"
#include "document/DocumentLimits.hpp"
#include "io/RenderExportPolicy.hpp"

#include <algorithm>

namespace ugurugu
{

long double AnimationExportPolicy::estimatedWorkingBytes(
    const QSize &frameSize, qsizetype frameCount)
{
    if (!frameSize.isValid() || frameCount <= 0)
    {
        return 0.0L;
    }
    return static_cast<long double>(frameSize.width())
           * static_cast<long double>(frameSize.height())
           * static_cast<long double>(frameCount) * 12.0L;
}

long double AnimationExportPolicy::estimatedWorkingBytes(
    const Document &document)
{
    return RenderExportPolicy::animatedGif(document).workingBytes;
}

bool AnimationExportPolicy::fitsMemoryBudget(
    const QSize &frameSize, qsizetype frameCount)
{
    const long double bytes = estimatedWorkingBytes(frameSize, frameCount);
    return bytes > 0.0L
           && bytes <= static_cast<long double>(
                  MemoryBudget::animationExportWorkingBytes);
}

long double AnimationExportPolicy::estimatedWorkingBytes(
    const Document &document, const QSize &frameSize)
{
    return RenderExportPolicy::animatedGifAtSize(document, frameSize)
        .workingBytes;
}

bool AnimationExportPolicy::fitsMemoryBudget(const Document &document)
{
    return RenderExportPolicy::animatedGifFitsMemoryBudget(document);
}

bool AnimationExportPolicy::fitsMemoryBudget(
    const Document &document, const QSize &frameSize)
{
    return RenderExportPolicy::animatedGifFitsMemoryBudget(document, frameSize);
}

QVector<int> AnimationExportPolicy::frameDurations(
    int frameCount, qreal framesPerSecond, int unitsPerSecond)
{
    const qreal fps = std::clamp(framesPerSecond,
        DocumentLimits::minimumFramesPerSecond,
        DocumentLimits::maximumFramesPerSecond);
    QVector<int> delays;
    delays.reserve(frameCount);
    qint64 emittedUnits = 0;
    for (int frame = 1; frame <= frameCount; ++frame)
    {
        const qint64 targetUnits =
            qRound64(static_cast<qreal>(frame) * unitsPerSecond / fps);
        const int delay =
            static_cast<int>(std::max<qint64>(1, targetUnits - emittedUnits));
        delays.append(delay);
        emittedUnits += delay;
    }
    return delays;
}

}

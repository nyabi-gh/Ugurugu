// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#pragma once

#include "document/Document.hpp"

#include <QSize>
#include <QVector>

namespace ugurugu
{

class AnimationExportPolicy final
{
public:
    static long double estimatedWorkingBytes(
        const QSize &frameSize, qsizetype frameCount);
    static long double estimatedWorkingBytes(const Document &document);
    static long double estimatedWorkingBytes(
        const Document &document, const QSize &frameSize);
    static bool fitsMemoryBudget(const QSize &frameSize, qsizetype frameCount);
    static bool fitsMemoryBudget(const Document &document);
    static bool fitsMemoryBudget(
        const Document &document, const QSize &frameSize);
    // Per-frame delays in encoder units (100/s for GIF, 1000/s for WebP),
    // drift-corrected so the emitted total tracks frame×unit/fps exactly.
    static QVector<int> frameDurations(
        int frameCount, qreal framesPerSecond, int unitsPerSecond);
};

}

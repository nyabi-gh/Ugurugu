// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#pragma once

#include <QtGlobal>

namespace ugurugu
{

struct DocumentLimits final
{
    static constexpr int minimumCanvasEdge = 1;
    static constexpr int maximumCanvasEdge = 4096;
    static constexpr qreal maximumStoredCoordinateMagnitude = 32767.0;
    static constexpr int minimumAnimationFrames = 2;
    static constexpr int maximumAnimationFrames = 60;
    static constexpr qreal minimumFramesPerSecond = 1.0;
    static constexpr qreal maximumFramesPerSecond = 50.0;
    static constexpr qreal minimumWobbleAmount = 0.0;
    static constexpr qreal maximumWobbleAmount = 12.0;
    static constexpr int minimumMotionPoseCount = 1;
    static constexpr int maximumMotionPoseCount = maximumAnimationFrames;
    static constexpr int minimumMotionDetail = 1;
    static constexpr int maximumMotionDetail = 24;
    static constexpr qreal minimumBreakRange = 2.0;
    static constexpr qreal maximumBreakRange = 256.0;
    static constexpr qreal minimumStrokeWidth = 0.25;
    static constexpr qreal maximumStrokeWidth = 512.0;
    static constexpr qreal minimumBrushSpacing = 0.02;
    static constexpr qreal maximumBrushSpacing = 2.0;
    static constexpr qreal maximumBrushScatter = 2.0;
    static constexpr qreal minimumBrushParticleSize = 0.01;
    static constexpr qreal maximumBrushParticleSize = 1.0;
    static constexpr qreal minimumBrushDensity = 0.05;
    static constexpr qreal maximumBrushDensity = 4.0;
    static constexpr qreal minimumBrushWobbleScale = 0.0;
    static constexpr qreal maximumBrushWobbleScale = 2.0;
    static constexpr int maximumLayerNameLength = 256;
    static constexpr int maximumLayers = 256;
    static constexpr int maximumLayerDepth = 8;
    static constexpr int maximumStrokesPerLayer = 20000;
    static constexpr int maximumTotalStrokes = 20000;
    static constexpr int maximumPointsPerStroke = 200000;
    static constexpr qsizetype maximumTotalPoints = 250000;
    static constexpr qint64 maximumProjectBytes = 128LL * 1024LL * 1024LL;
    static constexpr quint64 maximumDistinctClipMaskBytes =
        256ULL * 1024ULL * 1024ULL;
    static constexpr quint64 maximumRasterAssetPixels = 4096ULL * 4096ULL;
    static constexpr int maximumRasterAssets = maximumTotalStrokes;
    static constexpr quint64 maximumDistinctRasterDecodedBytes =
        256ULL * 1024ULL * 1024ULL;
    static constexpr qint64 maximumDistinctRasterPayloadBytes =
        72LL * 1024LL * 1024LL;
};

}

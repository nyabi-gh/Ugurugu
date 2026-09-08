// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "render/ClassicStrokeMotion.hpp"

#include "render/DeterministicNoise.hpp"

#include <algorithm>

namespace ugurugu::ClassicStrokeMotion
{

namespace
{

// These constants and channels define v1.0.0 Classic rendering compatibility.
constexpr qreal swayWavelength = 26.0;
constexpr qreal detailWavelength = 9.0;
constexpr quint64 widthChannel = 0x1a67d3c4ULL;
constexpr quint64 swayChannel = 0xb5297a4dULL;
constexpr quint64 detailChannel = 0x1b56c4e9ULL;
constexpr quint64 tangentChannel = 0x68e31da4ULL;

}

qreal displacementAmplitude(qreal strokeWidth, qreal wobbleAmount)
{
    return std::max(0.0, wobbleAmount)
           * (0.82 + std::min(strokeWidth, 40.0) * 0.018);
}

qreal maximumDisplacement(qreal strokeWidth, qreal wobbleAmount)
{
    return displacementAmplitude(strokeWidth, wobbleAmount) * 1.3;
}

qreal renderedWidth(
    qreal strokeWidth, quint64 seed, int frame, qreal wobbleAmount)
{
    const qreal widthNoiseScale = std::clamp(wobbleAmount / 1.6, 0.0, 1.0);
    const qreal frameWidthNoise =
        widthNoiseScale
        * DeterministicNoise::signedValue(seed, frame, 0, widthChannel);
    return std::max(0.5, strokeWidth * (1.0 + frameWidthNoise * 0.025));
}

StrokePoint displacedSample(const StrokePoint &sample,
    const QPointF &unitTangent,
    qreal arcLength,
    qreal amplitude,
    quint64 seed,
    int frame)
{
    const QPointF normal(-unitTangent.y(), unitTangent.x());
    const qreal sway = DeterministicNoise::smoothValue(
        seed, frame, arcLength / swayWavelength, swayChannel);
    const qreal detail = DeterministicNoise::smoothValue(
        seed, frame, arcLength / detailWavelength + 31.0, detailChannel);
    const qreal normalOffset = sway * 0.72 + detail * 0.28;
    const qreal tangentOffset = DeterministicNoise::smoothValue(
        seed, frame, arcLength / swayWavelength + 57.0, tangentChannel);
    const qreal pressureFactor = 0.8 + sample.pressure * 0.2;
    StrokePoint displaced = sample;
    displaced.position += normal * normalOffset * amplitude * pressureFactor
                          + unitTangent * tangentOffset * amplitude * 0.3;
    return displaced;
}

}

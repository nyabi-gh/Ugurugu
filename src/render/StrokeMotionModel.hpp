// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#pragma once

#include "document/Document.hpp"

namespace ugurugu::StrokeMotionModel
{

qreal maximumDisplacement(
    qreal strokeWidth, qreal wobbleAmount, const MotionSettings &settings);
qreal renderedWidth(qreal strokeWidth,
    quint64 seed,
    int frameIndex,
    int frameCount,
    qreal wobbleAmount,
    const MotionSettings &settings);
StrokePoint displacedSample(const StrokePoint &sample,
    const QPointF &unitTangent,
    qreal arcLength,
    qsizetype sampleIndex,
    qreal amplitude,
    quint64 seed,
    int frameIndex,
    int frameCount,
    const MotionSettings &settings);
int visibilityPose(
    int frameIndex, int frameCount, const MotionSettings &settings);

}

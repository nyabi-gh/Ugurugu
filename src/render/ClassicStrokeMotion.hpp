// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#pragma once

#include "document/Document.hpp"

namespace ugurugu::ClassicStrokeMotion
{

qreal displacementAmplitude(qreal strokeWidth, qreal wobbleAmount);
qreal maximumDisplacement(qreal strokeWidth, qreal wobbleAmount);
qreal renderedWidth(
    qreal strokeWidth, quint64 seed, int frame, qreal wobbleAmount);
StrokePoint displacedSample(const StrokePoint &sample,
    const QPointF &unitTangent,
    qreal arcLength,
    qreal amplitude,
    quint64 seed,
    int frame);

}

// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#pragma once

#include "document/Document.hpp"

#include <QPainter>
#include <QPainterPath>

namespace ugurugu::StrokeRenderer
{

struct PreparedStroke
{
    QVector<StrokePoint> points;
    QVector<quint8> visibleSegments;
    qreal width = 0.0;
    int normalizedFrame = 0;
    bool variablePressure = false;
    bool valid = false;
};

struct GeometryUpdate
{
    qsizetype changedFrom = 0;
    quint64 sourcePointsProcessed = 0;
    bool rebuilt = false;
    bool renderingModeChanged = false;
    bool valid = false;
};

class IncrementalGeometry final
{
public:
    const PreparedStroke &prepared() const;
    GeometryUpdate update(const Stroke &stroke,
        int frameIndex,
        int frameCount,
        qreal wobbleAmount);
    GeometryUpdate update(
        const Stroke &stroke, int frameIndex, const Document &document);
    void clear();

private:
    bool matches(const Stroke &stroke,
        int normalizedFrame,
        int frameCount,
        qreal wobbleAmount,
        const MotionSettings &motion) const;
    GeometryUpdate updateConfigured(const Stroke &stroke,
        int frameIndex,
        int frameCount,
        qreal wobbleAmount,
        const MotionSettings &motion);
    bool rebuildSamples(const Stroke &stroke, qreal spacing, qsizetype maximum);
    bool appendSamples(const Stroke &stroke);
    void displaceSamples(const Stroke &stroke,
        int normalizedFrame,
        int frameCount,
        qreal wobbleAmount,
        const MotionSettings &motion,
        qsizetype changedFrom);

    PreparedStroke m_prepared;
    QVector<StrokePoint> m_regularSamples;
    QVector<StrokePoint> m_samples;
    QVector<qreal> m_arcLengths;
    Stroke m_identity;
    qreal m_spacing = 0.0;
    qreal m_distanceToNext = 0.0;
    long double m_totalLength = 0.0L;
    qsizetype m_sourcePointCount = 0;
    qsizetype m_maximumPoints = 0;
    int m_frameCount = 0;
    int m_normalizedFrame = 0;
    qreal m_wobbleAmount = 0.0;
    MotionSettings m_motion;
    qreal m_regularMinimumPressure = 1.0;
    qreal m_regularMaximumPressure = 0.0;
    bool m_capped = false;
};

PreparedStroke prepare(
    const Stroke &stroke, int frameIndex, int frameCount, qreal wobbleAmount);
PreparedStroke prepare(
    const Stroke &stroke, int frameIndex, const Document &document);
QPainterPath path(
    const Stroke &stroke, int frameIndex, int frameCount, qreal wobbleAmount);
QPainterPath path(
    const Stroke &stroke, int frameIndex, const Document &document);
void paint(
    QPainter &painter, const Stroke &stroke, const PreparedStroke &prepared);
void paintPrimitives(QPainter &painter,
    const Stroke &stroke,
    const PreparedStroke &prepared,
    const QVector<int> &primitiveIndexes);
QRectF primitiveBounds(
    const Stroke &stroke, const PreparedStroke &prepared, int primitiveIndex);
int primitiveCount(const Stroke &stroke, const PreparedStroke &prepared);
// True when painting an already painted primitive again lands on the pixels
// it left: aliased coverage is binary, saturated ink has nothing left to
// blend, and a constant-pressure path draws the same shape wherever its range
// happens to begin. Callers that want to reuse a painted prefix and draw the
// remainder over it need this, because paintPrimitives redraws a few
// neighbours around whatever it is asked for.
bool repaintIsIdempotent(const Stroke &stroke, const PreparedStroke &prepared);

}

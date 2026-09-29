// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#pragma once

#include <QPointF>
#include <QtGlobal>

namespace ugurugu
{

class StrokeStabilizer final
{
public:
    qreal strength() const;
    void setStrength(qreal strength);
    void reset();
    QPointF begin(const QPointF &position, quint64 timestamp);
    QPointF update(const QPointF &position, quint64 timestamp);
    QPointF finish(const QPointF &position, quint64 timestamp);

private:
    qreal sampleInterval(quint64 timestamp) const;
    void rememberRawSample(const QPointF &position, quint64 timestamp);

    qreal m_strength = 0.0;
    bool m_initialized = false;
    QPointF m_previousRawPosition;
    QPointF m_filteredPosition;
    QPointF m_filteredVelocity;
    quint64 m_previousTimestamp = 0;
};

}

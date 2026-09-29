// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#pragma once

#include "document/Document.hpp"

#include <QVector>

namespace ugurugu::BrokenLineModel
{

struct VisibleRun
{
    qsizetype firstPoint = 0;
    qsizetype lastPoint = 0;

    bool operator==(const VisibleRun &) const = default;
};

QVector<quint8> visibleSegments(const QVector<StrokePoint> &points,
    quint64 seed,
    int pose,
    qreal breakAmount,
    qreal breakRange);
QVector<VisibleRun> visibleRuns(const QVector<quint8> &segments);

}

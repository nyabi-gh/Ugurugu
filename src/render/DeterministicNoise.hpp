// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#pragma once

#include <QtTypes>

namespace ugurugu::DeterministicNoise
{

// Results are pure functions of these inputs so preview, export, incremental
// redraw and redo cannot diverge through shared generator state.
qreal signedValue(quint64 seed, int frame, qint64 index, quint64 channel);
qreal smoothValue(quint64 seed, int frame, qreal coordinate, quint64 channel);
qreal unitValue(quint64 seed, int frame, qint64 index, quint64 channel);

}

// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#pragma once

#include <QImage>
#include <QPolygonF>
#include <QVector>

namespace ugurugu
{

// Traces the boundary of the set pixels in an 8-bit mask, using the >= 128
// threshold every other selection consumer applies. Contours are closed rings
// in document coordinates running along pixel corners, so a single-pixel
// selection traces the unit square around it rather than its centre. A mask
// with several disjoint regions yields one contour per region boundary,
// including the boundaries of holes.
QVector<QPolygonF> selectionOutline(const QImage &mask);

}

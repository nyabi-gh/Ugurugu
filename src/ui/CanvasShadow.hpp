// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#pragma once

#include <QPolygonF>
#include <QRectF>

class QPainter;

namespace ugurugu
{

// The canvas drop shadow looks like fourteen widening, fading strokes centred
// on the outline shifted 2 px down. That alpha depends only on the distance to
// the shifted outline, so it is filled as gradient bands along the edges and
// around the corners. The cost follows the visible band rather than the
// outline's size, which reaches tens of thousands of pixels zoomed in.
namespace CanvasShadow
{

// How far the shadow reaches past the outline, the shift included.
inline constexpr qreal margin = 16.0;

// `outline` must be convex, as any affine image of the canvas is. Only the
// bands meeting `visibleArea` are drawn; callers clip the canvas itself out.
void paint(
    QPainter &painter, const QPolygonF &outline, const QRectF &visibleArea);

}

}

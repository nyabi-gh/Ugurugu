// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#pragma once

#include "render/RenderEngine.hpp"

#include <QPointF>
#include <QRect>
#include <QSize>
#include <QTransform>

#include <algorithm>
#include <cmath>

namespace ugurugu
{
namespace render_detail
{

// Maps native document coordinates onto a preview framebuffer.
//
// A preview may only be replayed at display scale when it is no larger than
// the document on either axis and smaller on at least one; upscaling would
// have to invent detail that the native replay would not produce. Scaling is
// allowed to be non-uniform, so every mapping below carries both axes
// separately rather than one factor.

struct PreviewScaleMapping
{
    QSize documentSize;
    QSize outputSize;
    qreal horizontalScale = 1.0;
    qreal verticalScale = 1.0;

    QSize displaySize(const QSize &nativeSize) const
    {
        if (!nativeSize.isValid())
        {
            return {};
        }
        if (nativeSize == documentSize)
        {
            return outputSize;
        }
        return QSize(std::max(1, qRound(nativeSize.width() * horizontalScale)),
            std::max(1, qRound(nativeSize.height() * verticalScale)));
    }

    QPointF displayPoint(const QPoint &nativePoint) const
    {
        return QPointF(
            nativePoint.x() * horizontalScale, nativePoint.y() * verticalScale);
    }

    QRect displayBounds(
        const QRect &nativeBounds, const QRect &displayCanvas) const
    {
        const QRectF mapped(nativeBounds.x() * horizontalScale,
            nativeBounds.y() * verticalScale,
            nativeBounds.width() * horizontalScale,
            nativeBounds.height() * verticalScale);
        return mapped.toAlignedRect().intersected(displayCanvas);
    }

    QPoint nativeSampleForDisplayPixel(const QPoint &displayPixel) const
    {
        // Sampling pixel centers keeps masks and framebuffer pixels in the
        // same display-space coordinate system, including non-integral zoom.
        return QPoint(static_cast<int>(std::floor(
                          (displayPixel.x() + 0.5) / horizontalScale)),
            static_cast<int>(
                std::floor((displayPixel.y() + 0.5) / verticalScale)));
    }

    QTransform displayTransform(const QTransform &nativeTransform) const
    {
        // D * T * D^-1, written explicitly for Qt's affine coefficient
        // layout. This is also correct for non-uniform preview scaling.
        return QTransform(nativeTransform.m11(),
            nativeTransform.m12() * verticalScale / horizontalScale,
            0.0,
            nativeTransform.m21() * horizontalScale / verticalScale,
            nativeTransform.m22(),
            0.0,
            nativeTransform.dx() * horizontalScale,
            nativeTransform.dy() * verticalScale,
            1.0);
    }
};

bool isNonUpscaledDisplaySize(const QSize &nativeSize, const QSize &outputSize);

bool canReplayAtDisplayScale(const Document &document, const QSize &outputSize);

// Diagnostics only: both record the renderer's own QImages so a caller can
// bound preview memory. Document-owned masks and paint-engine scratch buffers
// are deliberately excluded, so these under-report total process usage.
void notePreviewImage(
    RenderEngine::ScaledRenderStats *stats, const QImage &image);

template <typename... Images>
void notePreviewWorkingSet(
    RenderEngine::ScaledRenderStats *stats, const Images &...images)
{
    if (!stats)
    {
        return;
    }
    quint64 bytes = 0;
    const auto add = [&bytes](const QImage &image)
    {
        if (!image.isNull())
        {
            bytes += static_cast<quint64>(image.sizeInBytes());
        }
    };
    (add(images), ...);
    stats->maximumEstimatedWorkingSetBytes =
        std::max(stats->maximumEstimatedWorkingSetBytes, bytes);
}

}

}

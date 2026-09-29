// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "render/ImageResampler.hpp"

#include <algorithm>

namespace ugurugu
{
namespace
{

// Source coordinates are 16.16 fixed point throughout this file so that
// resampleRegion returns exactly the bytes resample would have produced for
// that rectangle. The renderer redraws only dirty regions, and float
// accumulation would let a region disagree with its neighbours by a level or
// two and leave a visible seam. Qt's own smooth scale gives no such guarantee,
// which is why this resampler exists at all.
constexpr qint64 fixedScale = qint64(1) << 16;
constexpr qint64 fixedHalf = fixedScale / 2;

// Maps the center of a target pixel to the center of the source pixel grid.
// The trailing half-pixel shift moves that center into the index space the
// bilinear taps expect; dropping it biases the whole image by half a pixel.
qint64 sourceCoordinate(
    int targetCoordinate, int sourceExtent, int targetExtent)
{
    const qint64 numerator =
        (qint64(targetCoordinate) * 2 + 1) * qint64(sourceExtent) * fixedScale;
    return numerator / qint64(2 * targetExtent) - fixedHalf;
}

int fixedFloor(qint64 coordinate)
{
    if (coordinate >= 0)
    {
        return int(coordinate / fixedScale);
    }
    return -int((-coordinate + fixedScale - 1) / fixedScale);
}

int nearestSourceCoordinate(
    int targetCoordinate, int sourceExtent, int targetExtent)
{
    const qint64 numerator =
        (qint64(targetCoordinate) * 2 + 1) * qint64(sourceExtent);
    return std::clamp(
        int(numerator / qint64(2 * targetExtent)), 0, sourceExtent - 1);
}

QRgb sourcePixel(const QImage &source, const QRect &sourceBounds, int x, int y)
{
    if (x < sourceBounds.left() || x > sourceBounds.right()
        || y < sourceBounds.top() || y > sourceBounds.bottom())
    {
        return 0;
    }
    const auto *line = reinterpret_cast<const QRgb *>(
        source.constScanLine(y - sourceBounds.top()));
    return line[x - sourceBounds.left()];
}

int interpolateChannel(int topLeft,
    int topRight,
    int bottomLeft,
    int bottomRight,
    int horizontalDistance,
    int verticalDistance)
{
    const int inverseVertical = 256 - verticalDistance;
    const int left =
        (topLeft * inverseVertical + bottomLeft * verticalDistance) >> 8;
    const int right =
        (topRight * inverseVertical + bottomRight * verticalDistance) >> 8;
    return (left * (256 - horizontalDistance) + right * horizontalDistance)
           >> 8;
}

QRgb smoothPixel(const QImage &source,
    const QRect &sourceBounds,
    const QSize &sourceCanvasSize,
    int targetX,
    int targetY,
    const QSize &targetCanvasSize)
{
    const qint64 fixedX = sourceCoordinate(
        targetX, sourceCanvasSize.width(), targetCanvasSize.width());
    const qint64 fixedY = sourceCoordinate(
        targetY, sourceCanvasSize.height(), targetCanvasSize.height());
    const int baseX = fixedFloor(fixedX);
    const int baseY = fixedFloor(fixedY);
    const int left = std::clamp(baseX, 0, sourceCanvasSize.width() - 1);
    const int right = std::clamp(baseX + 1, 0, sourceCanvasSize.width() - 1);
    const int top = std::clamp(baseY, 0, sourceCanvasSize.height() - 1);
    const int bottom = std::clamp(baseY + 1, 0, sourceCanvasSize.height() - 1);
    const int horizontalDistance =
        int((fixedX - qint64(baseX) * fixedScale) >> 8);
    const int verticalDistance =
        int((fixedY - qint64(baseY) * fixedScale) >> 8);
    const QRgb topLeft = sourcePixel(source, sourceBounds, left, top);
    const QRgb topRight = sourcePixel(source, sourceBounds, right, top);
    const QRgb bottomLeft = sourcePixel(source, sourceBounds, left, bottom);
    const QRgb bottomRight = sourcePixel(source, sourceBounds, right, bottom);
    return qRgba(interpolateChannel(qRed(topLeft),
                     qRed(topRight),
                     qRed(bottomLeft),
                     qRed(bottomRight),
                     horizontalDistance,
                     verticalDistance),
        interpolateChannel(qGreen(topLeft),
            qGreen(topRight),
            qGreen(bottomLeft),
            qGreen(bottomRight),
            horizontalDistance,
            verticalDistance),
        interpolateChannel(qBlue(topLeft),
            qBlue(topRight),
            qBlue(bottomLeft),
            qBlue(bottomRight),
            horizontalDistance,
            verticalDistance),
        interpolateChannel(qAlpha(topLeft),
            qAlpha(topRight),
            qAlpha(bottomLeft),
            qAlpha(bottomRight),
            horizontalDistance,
            verticalDistance));
}

}

QImage ImageResampler::resample(
    const QImage &source, const QSize &targetSize, SamplingMode sampling)
{
    return resampleRegion(source,
        source.rect(),
        source.size(),
        QRect(QPoint(), targetSize),
        targetSize,
        sampling);
}

QImage ImageResampler::resampleRegion(const QImage &source,
    const QRect &sourceBounds,
    const QSize &sourceCanvasSize,
    const QRect &targetBounds,
    const QSize &targetCanvasSize,
    SamplingMode sampling)
{
    const QRect sourceCanvas(QPoint(), sourceCanvasSize);
    const QRect targetCanvas(QPoint(), targetCanvasSize);
    if (source.isNull()
        || source.format() != QImage::Format_ARGB32_Premultiplied
        || !sourceCanvasSize.isValid() || !targetCanvasSize.isValid()
        || sourceBounds.isEmpty() || source.size() != sourceBounds.size()
        || !sourceCanvas.contains(sourceBounds) || targetBounds.isEmpty()
        || !targetCanvas.contains(targetBounds)
        || (sampling != SamplingMode::Nearest
            && sampling != SamplingMode::Smooth))
    {
        return {};
    }
    QImage target(targetBounds.size(), QImage::Format_ARGB32_Premultiplied);
    if (target.isNull())
    {
        return {};
    }
    for (int y = targetBounds.top(); y <= targetBounds.bottom(); ++y)
    {
        auto *targetLine =
            reinterpret_cast<QRgb *>(target.scanLine(y - targetBounds.top()));
        if (sampling == SamplingMode::Nearest)
        {
            const int sourceY = nearestSourceCoordinate(
                y, sourceCanvasSize.height(), targetCanvasSize.height());
            for (int x = targetBounds.left(); x <= targetBounds.right(); ++x)
            {
                const int sourceX = nearestSourceCoordinate(
                    x, sourceCanvasSize.width(), targetCanvasSize.width());
                targetLine[x - targetBounds.left()] =
                    sourcePixel(source, sourceBounds, sourceX, sourceY);
            }
            continue;
        }
        for (int x = targetBounds.left(); x <= targetBounds.right(); ++x)
        {
            targetLine[x - targetBounds.left()] = smoothPixel(
                source, sourceBounds, sourceCanvasSize, x, y, targetCanvasSize);
        }
    }
    return target;
}

}

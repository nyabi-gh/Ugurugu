// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "render/ImageAffineTransformer.hpp"

#include <algorithm>
#include <cmath>
#include <limits>

namespace ugurugu
{
namespace
{

// Same 16.16 fixed point contract as ImageResampler: transforming a target
// rectangle on its own has to match the corresponding part of transforming the
// whole target, so a regional repaint cannot seam against what is already on
// screen.
constexpr qint64 fixedScale = qint64(1) << 16;
constexpr qint64 fixedHalf = fixedScale / 2;

bool validSampling(SamplingMode sampling)
{
    return sampling == SamplingMode::Nearest
           || sampling == SamplingMode::Smooth;
}

SamplingMode effectiveSampling(
    const QTransform &transform, SamplingMode sampling)
{
    if (sampling == SamplingMode::Smooth && transform.m11() == 1.0
        && transform.m12() == 0.0 && transform.m21() == 0.0
        && transform.m22() == 1.0)
    {
        return SamplingMode::Nearest;
    }
    return sampling;
}

bool finiteRect(const QRectF &bounds)
{
    return std::isfinite(bounds.left()) && std::isfinite(bounds.top())
           && std::isfinite(bounds.right()) && std::isfinite(bounds.bottom());
}

QRect mappedBounds(const QRect &sourceBounds,
    const QTransform &transform,
    SamplingMode sampling,
    const QRect &clipBounds)
{
    if (sourceBounds.isEmpty() || clipBounds.isEmpty() || !transform.isAffine()
        || !transform.isInvertible() || !validSampling(sampling))
    {
        return {};
    }
    const QRectF mapped = transform.mapRect(QRectF(sourceBounds));
    if (!finiteRect(mapped))
    {
        return clipBounds;
    }
    const qreal left = std::clamp(std::floor(mapped.left()) - 1.0,
        qreal(clipBounds.left()),
        qreal(clipBounds.right()) + 1.0);
    const qreal top = std::clamp(std::floor(mapped.top()) - 1.0,
        qreal(clipBounds.top()),
        qreal(clipBounds.bottom()) + 1.0);
    const qreal right = std::clamp(std::ceil(mapped.right()) + 1.0,
        qreal(clipBounds.left()),
        qreal(clipBounds.right()) + 1.0);
    const qreal bottom = std::clamp(std::ceil(mapped.bottom()) + 1.0,
        qreal(clipBounds.top()),
        qreal(clipBounds.bottom()) + 1.0);
    const int pixelLeft = static_cast<int>(left);
    const int pixelTop = static_cast<int>(top);
    const int pixelRight = static_cast<int>(right);
    const int pixelBottom = static_cast<int>(bottom);
    return pixelRight > pixelLeft && pixelBottom > pixelTop
               ? QRect(pixelLeft,
                     pixelTop,
                     pixelRight - pixelLeft,
                     pixelBottom - pixelTop)
               : QRect();
}

QRgb sourcePixel(const QImage &source, const QRect &sourceBounds, int x, int y)
{
    x = std::clamp(x, sourceBounds.left(), sourceBounds.right());
    y = std::clamp(y, sourceBounds.top(), sourceBounds.bottom());
    const auto *line = reinterpret_cast<const QRgb *>(
        source.constScanLine(y - sourceBounds.top()));
    return line[x - sourceBounds.left()];
}

bool quantizeCoordinate(qreal coordinate, qint64 &fixed)
{
    const qreal scaled = coordinate * qreal(fixedScale);
    constexpr qint64 safeLimit = std::numeric_limits<qint64>::max() / 2;
    if (!std::isfinite(scaled) || scaled < -qreal(safeLimit)
        || scaled > qreal(safeLimit))
    {
        return false;
    }
    fixed = static_cast<qint64>(std::floor(scaled + 0.5));
    return true;
}

qint64 fixedFloor(qint64 coordinate)
{
    const qint64 quotient = coordinate / fixedScale;
    return quotient - (coordinate < 0 && coordinate % fixedScale != 0 ? 1 : 0);
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
    qint64 fixedSourceX,
    qint64 fixedSourceY)
{
    const qint64 localX =
        fixedSourceX - qint64(sourceBounds.left()) * fixedScale - fixedHalf;
    const qint64 localY =
        fixedSourceY - qint64(sourceBounds.top()) * fixedScale - fixedHalf;
    const qint64 floorX = fixedFloor(localX);
    const qint64 floorY = fixedFloor(localY);
    if (floorX < std::numeric_limits<int>::min()
        || floorX > std::numeric_limits<int>::max() - 1
        || floorY < std::numeric_limits<int>::min()
        || floorY > std::numeric_limits<int>::max() - 1)
    {
        return 0;
    }
    const int left = static_cast<int>(floorX) + sourceBounds.left();
    const int top = static_cast<int>(floorY) + sourceBounds.top();
    const int horizontalDistance = int((localX - floorX * fixedScale) >> 8);
    const int verticalDistance = int((localY - floorY * fixedScale) >> 8);
    const QRgb topLeft = sourcePixel(source, sourceBounds, left, top);
    const QRgb topRight = sourcePixel(source, sourceBounds, left + 1, top);
    const QRgb bottomLeft = sourcePixel(source, sourceBounds, left, top + 1);
    const QRgb bottomRight =
        sourcePixel(source, sourceBounds, left + 1, top + 1);
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

QRgb nearestPixel(const QImage &source,
    const QRect &sourceBounds,
    qint64 fixedSourceX,
    qint64 fixedSourceY)
{
    const qint64 floorX = fixedFloor(fixedSourceX);
    const qint64 floorY = fixedFloor(fixedSourceY);
    if (floorX < std::numeric_limits<int>::min()
        || floorX > std::numeric_limits<int>::max()
        || floorY < std::numeric_limits<int>::min()
        || floorY > std::numeric_limits<int>::max())
    {
        return 0;
    }
    return sourcePixel(source,
        sourceBounds,
        static_cast<int>(floorX),
        static_cast<int>(floorY));
}

int maskPixel(const QImage &source, const QRect &sourceBounds, int x, int y)
{
    x = std::clamp(x, sourceBounds.left(), sourceBounds.right());
    y = std::clamp(y, sourceBounds.top(), sourceBounds.bottom());
    return source.constScanLine(
        y - sourceBounds.top())[x - sourceBounds.left()];
}

int smoothMaskPixel(const QImage &source,
    const QRect &sourceBounds,
    qint64 fixedSourceX,
    qint64 fixedSourceY)
{
    const qint64 localX =
        fixedSourceX - qint64(sourceBounds.left()) * fixedScale - fixedHalf;
    const qint64 localY =
        fixedSourceY - qint64(sourceBounds.top()) * fixedScale - fixedHalf;
    const qint64 floorX = fixedFloor(localX);
    const qint64 floorY = fixedFloor(localY);
    if (floorX < std::numeric_limits<int>::min()
        || floorX > std::numeric_limits<int>::max() - 1
        || floorY < std::numeric_limits<int>::min()
        || floorY > std::numeric_limits<int>::max() - 1)
    {
        return 0;
    }
    const int left = static_cast<int>(floorX) + sourceBounds.left();
    const int top = static_cast<int>(floorY) + sourceBounds.top();
    const int horizontalDistance = int((localX - floorX * fixedScale) >> 8);
    const int verticalDistance = int((localY - floorY * fixedScale) >> 8);
    return interpolateChannel(maskPixel(source, sourceBounds, left, top),
        maskPixel(source, sourceBounds, left + 1, top),
        maskPixel(source, sourceBounds, left, top + 1),
        maskPixel(source, sourceBounds, left + 1, top + 1),
        horizontalDistance,
        verticalDistance);
}

int nearestMaskPixel(const QImage &source,
    const QRect &sourceBounds,
    qint64 fixedSourceX,
    qint64 fixedSourceY)
{
    const qint64 sourceX = fixedFloor(fixedSourceX);
    const qint64 sourceY = fixedFloor(fixedSourceY);
    if (sourceX < std::numeric_limits<int>::min()
        || sourceX > std::numeric_limits<int>::max()
        || sourceY < std::numeric_limits<int>::min()
        || sourceY > std::numeric_limits<int>::max())
    {
        return 0;
    }
    return maskPixel(source,
        sourceBounds,
        static_cast<int>(sourceX),
        static_cast<int>(sourceY));
}

int byteMultiply(int value, int alpha)
{
    const int product = value * alpha;
    return (product + (product >> 8) + 0x80) >> 8;
}

QRgb sourceOver(QRgb destination, QRgb source)
{
    const int sourceAlpha = qAlpha(source);
    if (sourceAlpha == 255)
    {
        return source;
    }
    if (source == 0)
    {
        return destination;
    }
    const int inverseAlpha = 255 - sourceAlpha;
    return qRgba(qRed(source) + byteMultiply(qRed(destination), inverseAlpha),
        qGreen(source) + byteMultiply(qGreen(destination), inverseAlpha),
        qBlue(source) + byteMultiply(qBlue(destination), inverseAlpha),
        sourceAlpha + byteMultiply(qAlpha(destination), inverseAlpha));
}

}

QRect ImageAffineTransformer::targetBounds(const QRect &sourceBounds,
    const QSize &targetCanvasSize,
    const QTransform &transform,
    SamplingMode sampling)
{
    if (!targetCanvasSize.isValid())
    {
        return {};
    }
    return mappedBounds(
        sourceBounds, transform, sampling, QRect(QPoint(), targetCanvasSize));
}

bool ImageAffineTransformer::compositeSourceOver(QImage &target,
    const QRect &targetImageBounds,
    const QImage &source,
    const QRect &sourceImageBounds,
    const QTransform &transform,
    SamplingMode sampling)
{
    if (target.isNull() || source.isNull() || targetImageBounds.isEmpty()
        || sourceImageBounds.isEmpty()
        || target.size() != targetImageBounds.size()
        || source.size() != sourceImageBounds.size()
        || target.format() != QImage::Format_ARGB32_Premultiplied
        || source.format() != QImage::Format_ARGB32_Premultiplied
        || !transform.isAffine() || !transform.isInvertible()
        || !validSampling(sampling))
    {
        return false;
    }
    bool invertible = false;
    const QTransform inverse = transform.inverted(&invertible);
    if (!invertible)
    {
        return false;
    }
    sampling = effectiveSampling(transform, sampling);
    const QRect effectBounds =
        mappedBounds(sourceImageBounds, transform, sampling, targetImageBounds);
    for (int y = effectBounds.top(); y <= effectBounds.bottom(); ++y)
    {
        auto *targetLine = reinterpret_cast<QRgb *>(
            target.scanLine(y - targetImageBounds.top()));
        for (int x = effectBounds.left(); x <= effectBounds.right(); ++x)
        {
            const QPointF sourcePoint =
                inverse.map(QPointF(qreal(x) + 0.5, qreal(y) + 0.5));
            if (!std::isfinite(sourcePoint.x())
                || !std::isfinite(sourcePoint.y()))
            {
                continue;
            }
            qint64 fixedSourceX = 0;
            qint64 fixedSourceY = 0;
            if (!quantizeCoordinate(sourcePoint.x(), fixedSourceX)
                || !quantizeCoordinate(sourcePoint.y(), fixedSourceY))
            {
                continue;
            }
            if (fixedSourceX < qint64(sourceImageBounds.left()) * fixedScale
                || fixedSourceX
                       >= qint64(sourceImageBounds.right() + 1) * fixedScale
                || fixedSourceY < qint64(sourceImageBounds.top()) * fixedScale
                || fixedSourceY
                       >= qint64(sourceImageBounds.bottom() + 1) * fixedScale)
            {
                continue;
            }
            const QRgb sampled =
                sampling == SamplingMode::Nearest
                    ? nearestPixel(
                          source, sourceImageBounds, fixedSourceX, fixedSourceY)
                    : smoothPixel(source,
                          sourceImageBounds,
                          fixedSourceX,
                          fixedSourceY);
            const int targetX = x - targetImageBounds.left();
            targetLine[targetX] = sourceOver(targetLine[targetX], sampled);
        }
    }
    return true;
}

QImage ImageAffineTransformer::transformMask(const QImage &source,
    const QRect &sourceImageBounds,
    const QRect &targetImageBounds,
    const QTransform &transform,
    SamplingMode sampling)
{
    if (source.isNull() || sourceImageBounds.isEmpty()
        || targetImageBounds.isEmpty()
        || source.size() != sourceImageBounds.size()
        || source.format() != QImage::Format_Grayscale8 || !transform.isAffine()
        || !transform.isInvertible() || !validSampling(sampling))
    {
        return {};
    }
    bool invertible = false;
    const QTransform inverse = transform.inverted(&invertible);
    if (!invertible)
    {
        return {};
    }
    sampling = effectiveSampling(transform, sampling);
    QImage target(targetImageBounds.size(), QImage::Format_Grayscale8);
    if (target.isNull())
    {
        return {};
    }
    target.fill(0);
    const QRect effectBounds =
        mappedBounds(sourceImageBounds, transform, sampling, targetImageBounds);
    for (int y = effectBounds.top(); y <= effectBounds.bottom(); ++y)
    {
        uchar *targetLine = target.scanLine(y - targetImageBounds.top());
        for (int x = effectBounds.left(); x <= effectBounds.right(); ++x)
        {
            const QPointF sourcePoint =
                inverse.map(QPointF(qreal(x) + 0.5, qreal(y) + 0.5));
            qint64 fixedSourceX = 0;
            qint64 fixedSourceY = 0;
            if (!quantizeCoordinate(sourcePoint.x(), fixedSourceX)
                || !quantizeCoordinate(sourcePoint.y(), fixedSourceY))
            {
                continue;
            }
            if (fixedSourceX < qint64(sourceImageBounds.left()) * fixedScale
                || fixedSourceX
                       >= qint64(sourceImageBounds.right() + 1) * fixedScale
                || fixedSourceY < qint64(sourceImageBounds.top()) * fixedScale
                || fixedSourceY
                       >= qint64(sourceImageBounds.bottom() + 1) * fixedScale)
            {
                continue;
            }
            targetLine[x - targetImageBounds.left()] =
                sampling == SamplingMode::Nearest
                    ? nearestMaskPixel(
                          source, sourceImageBounds, fixedSourceX, fixedSourceY)
                    : smoothMaskPixel(source,
                          sourceImageBounds,
                          fixedSourceX,
                          fixedSourceY);
        }
    }
    return target;
}

}

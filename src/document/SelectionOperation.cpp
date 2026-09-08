// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "document/SelectionOperation.hpp"

#include "document/DocumentLimits.hpp"
#include "document/StrokeMask.hpp"
#include "render/ImageAffineTransformer.hpp"

#include <QPainter>
#include <QSet>

#include <algorithm>
#include <cmath>
#include <limits>

namespace ugurugu
{

namespace
{

qsizetype packedStride(int width)
{
    return (static_cast<qsizetype>(width) + 7) / 8;
}

bool finiteTransform(const QTransform &transform)
{
    return std::isfinite(transform.m11()) && std::isfinite(transform.m12())
           && std::isfinite(transform.m13()) && std::isfinite(transform.m21())
           && std::isfinite(transform.m22()) && std::isfinite(transform.m23())
           && std::isfinite(transform.m31()) && std::isfinite(transform.m32())
           && std::isfinite(transform.m33());
}

bool validTransformTargetSize(const QSize &size)
{
    return size.width() >= DocumentLimits::minimumCanvasEdge
           && size.height() >= DocumentLimits::minimumCanvasEdge
           && size.width() <= DocumentLimits::maximumCanvasEdge
           && size.height() <= DocumentLimits::maximumCanvasEdge;
}

SamplingMode samplingForTransformImpl(const QTransform &transform)
{
    const auto integralUnit = [](qreal value)
    {
        return qFuzzyIsNull(value) || qFuzzyCompare(std::abs(value), 1.0);
    };
    const auto integralTranslation = [](qreal value)
    {
        return qFuzzyCompare(value + 1.0, std::round(value) + 1.0);
    };
    const bool orthogonalUnit =
        integralUnit(transform.m11()) && integralUnit(transform.m12())
        && integralUnit(transform.m21()) && integralUnit(transform.m22())
        && qFuzzyIsNull(transform.m11() * transform.m21()
                        + transform.m12() * transform.m22())
        && qFuzzyCompare(transform.m11() * transform.m11()
                             + transform.m12() * transform.m12() + 1.0,
            2.0)
        && qFuzzyCompare(transform.m21() * transform.m21()
                             + transform.m22() * transform.m22() + 1.0,
            2.0);
    return orthogonalUnit && integralTranslation(transform.dx())
                   && integralTranslation(transform.dy())
               ? SamplingMode::Nearest
               : SamplingMode::Smooth;
}

QRect selectionBounds(const QImage &mask)
{
    const int width = mask.width();
    const int height = mask.height();
    int left = width;
    int top = height;
    int right = -1;
    int bottom = -1;
    for (int y = 0; y < height; ++y)
    {
        const uchar *line = mask.constScanLine(y);
        int first = 0;
        while (first < width && line[first] < 128)
        {
            ++first;
        }
        if (first == width)
        {
            continue;
        }
        int last = width - 1;
        while (line[last] < 128)
        {
            --last;
        }
        if (top == height)
        {
            top = y;
        }
        bottom = y;
        if (first < left)
        {
            left = first;
        }
        if (last > right)
        {
            right = last;
        }
    }
    return right < left || bottom < top
               ? QRect()
               : QRect(QPoint(left, top), QPoint(right, bottom));
}

constexpr qreal transformedBoundsMargin = 4.0;

void resetMemoryStats(SelectionTransformMemoryStats *stats)
{
    if (stats)
    {
        *stats = {};
    }
}

QRect fullImageBounds(const QSize &size)
{
    return QRect(QPoint(), size);
}

QRect mappedTargetBounds(const QRect &sourceBounds,
    const QSize &targetSize,
    const QTransform &transform)
{
    const QRectF mapped = transform.mapRect(QRectF(sourceBounds));
    if (!std::isfinite(mapped.left()) || !std::isfinite(mapped.top())
        || !std::isfinite(mapped.right()) || !std::isfinite(mapped.bottom()))
    {
        return {};
    }
    const qreal width = targetSize.width();
    const qreal height = targetSize.height();
    const qreal leftValue =
        std::clamp(mapped.left() - transformedBoundsMargin, 0.0, width);
    const qreal topValue =
        std::clamp(mapped.top() - transformedBoundsMargin, 0.0, height);
    const qreal rightValue =
        std::clamp(mapped.right() + transformedBoundsMargin, 0.0, width);
    const qreal bottomValue =
        std::clamp(mapped.bottom() + transformedBoundsMargin, 0.0, height);
    const int left = static_cast<int>(std::floor(leftValue));
    const int top = static_cast<int>(std::floor(topValue));
    const int right = static_cast<int>(std::ceil(rightValue));
    const int bottom = static_cast<int>(std::ceil(bottomValue));
    if (right <= left || bottom <= top)
    {
        return {};
    }
    return QRect(QPoint(left, top), QSize(right - left, bottom - top));
}

bool integralOrthogonalTransform(const QTransform &transform)
{
    return finiteTransform(transform) && transform.isAffine()
           && transform.isInvertible()
           && samplingForTransformImpl(transform) == SamplingMode::Nearest;
}

QImage binarySourceFromMask(
    const QImage &mask, const QRect &bounds, QImage::Format format)
{
    QImage source(bounds.size(), format);
    if (source.isNull())
    {
        return {};
    }
    source.fill(0);
    for (int y = bounds.top(); y <= bounds.bottom(); ++y)
    {
        const uchar *input = mask.constScanLine(y);
        if (format == QImage::Format_ARGB32_Premultiplied)
        {
            auto *output =
                reinterpret_cast<QRgb *>(source.scanLine(y - bounds.top()));
            for (int x = bounds.left(); x <= bounds.right(); ++x)
            {
                if (input[x] >= 128)
                {
                    output[x - bounds.left()] =
                        qPremultiply(qRgba(255, 255, 255, 255));
                }
            }
        }
        else
        {
            uchar *output = source.scanLine(y - bounds.top());
            for (int x = bounds.left(); x <= bounds.right(); ++x)
            {
                output[x - bounds.left()] = input[x] >= 128 ? 255 : 0;
            }
        }
    }
    return source;
}

QImage binarySourceFromPacked(
    const PackedMaskRegion &region, const QRect &sourceBounds)
{
    QImage source(sourceBounds.size(), QImage::Format_Grayscale8);
    if (source.isNull())
    {
        return {};
    }
    source.fill(0);
    const qsizetype stride = packedStride(region.bounds.width());
    const QPoint offset = region.bounds.topLeft() - sourceBounds.topLeft();
    for (int y = 0; y < region.bounds.height(); ++y)
    {
        const auto *packed = reinterpret_cast<const uchar *>(
            region.packedMask.constData() + static_cast<qsizetype>(y) * stride);
        uchar *output = source.scanLine(y + offset.y());
        for (int x = 0; x < region.bounds.width(); ++x)
        {
            if ((packed[x / 8] & static_cast<uchar>(0x80U >> (x % 8))) != 0)
            {
                output[x + offset.x()] = 255;
            }
        }
    }
    return source;
}

struct RenderedMaskRegion
{
    QImage image;
    QRect bounds;
    quint64 sourceBytes = 0;
    quint64 targetBytes = 0;
    quint64 renderPeakBytes = 0;
    bool usedArgbSource = false;
    bool usedArgbTarget = false;
};

RenderedMaskRegion renderBinarySource(const QImage &source,
    const QRect &sourceBounds,
    const QRect &targetBounds,
    const QTransform &transform,
    bool smooth,
    QImage::Format targetFormat)
{
    RenderedMaskRegion rendered;
    rendered.bounds = targetBounds;
    rendered.sourceBytes = source.sizeInBytes();
    rendered.usedArgbSource =
        source.format() == QImage::Format_ARGB32_Premultiplied;
    rendered.usedArgbTarget =
        targetFormat == QImage::Format_ARGB32_Premultiplied;
    rendered.image = QImage(targetBounds.size(), targetFormat);
    if (source.isNull() || rendered.image.isNull())
    {
        rendered.image = {};
        return rendered;
    }
    rendered.image.fill(0);
    rendered.targetBytes = rendered.image.sizeInBytes();
    rendered.renderPeakBytes = rendered.sourceBytes + rendered.targetBytes;

    QPainter painter(&rendered.image);
    if (!painter.isActive())
    {
        rendered.image = {};
        return rendered;
    }
    painter.setRenderHint(QPainter::Antialiasing, false);
    painter.setRenderHint(QPainter::SmoothPixmapTransform, smooth);
    painter.setCompositionMode(QPainter::CompositionMode_SourceOver);
    painter.setWindow(targetBounds);
    painter.setViewport(QRect(QPoint(), targetBounds.size()));
    painter.setTransform(transform);
    painter.drawImage(sourceBounds.topLeft(), source);
    painter.end();
    return rendered;
}

void recordMemoryStats(SelectionTransformMemoryStats *stats,
    const QRect &sourceBounds,
    const RenderedMaskRegion &rendered,
    quint64 resultBytes,
    bool usedFullTargetFallback)
{
    if (!stats)
    {
        return;
    }
    stats->sourceBounds = sourceBounds;
    stats->targetBounds = rendered.bounds;
    stats->sourceImageBytes = rendered.sourceBytes;
    stats->targetImageBytes = rendered.targetBytes;
    stats->resultBytes = resultBytes;
    stats->usedArgbSource = rendered.usedArgbSource;
    stats->usedArgbTarget = rendered.usedArgbTarget;
    stats->usedFullTargetFallback = usedFullTargetFallback;
    stats->peakLiveImageBytes =
        std::max(rendered.renderPeakBytes, rendered.targetBytes + resultBytes);
}

QImage supportFromRenderedRegion(
    const RenderedMaskRegion &rendered, const QSize &targetSize)
{
    QImage support(targetSize, QImage::Format_Grayscale8);
    if (support.isNull())
    {
        return {};
    }
    support.fill(0);
    for (int y = 0; y < rendered.image.height(); ++y)
    {
        uchar *output =
            support.scanLine(y + rendered.bounds.y()) + rendered.bounds.x();
        if (rendered.image.format() == QImage::Format_Alpha8
            || rendered.image.format() == QImage::Format_Grayscale8)
        {
            const uchar *input = rendered.image.constScanLine(y);
            for (int x = 0; x < rendered.image.width(); ++x)
            {
                output[x] = input[x] >= 128 ? 255 : 0;
            }
        }
        else
        {
            const auto *input =
                reinterpret_cast<const QRgb *>(rendered.image.constScanLine(y));
            for (int x = 0; x < rendered.image.width(); ++x)
            {
                output[x] = qAlpha(input[x]) >= 128 ? 255 : 0;
            }
        }
    }
    return support;
}

std::optional<PackedMaskRegion> packRenderedRegion(
    const RenderedMaskRegion &rendered, const QSize &targetSize)
{
    int left = rendered.image.width();
    int top = rendered.image.height();
    int right = -1;
    int bottom = -1;
    for (int y = 0; y < rendered.image.height(); ++y)
    {
        const uchar *line = rendered.image.constScanLine(y);
        for (int x = 0; x < rendered.image.width(); ++x)
        {
            if (line[x] < 128)
            {
                continue;
            }
            left = std::min(left, x);
            top = std::min(top, y);
            right = std::max(right, x);
            bottom = std::max(bottom, y);
        }
    }
    if (right < left || bottom < top)
    {
        return std::nullopt;
    }

    const QRect localBounds(QPoint(left, top), QPoint(right, bottom));
    const qsizetype stride = packedStride(localBounds.width());
    if (stride <= 0
        || stride
               > std::numeric_limits<qsizetype>::max() / localBounds.height())
    {
        return std::nullopt;
    }
    PackedMaskRegion packed;
    packed.canvasSize = targetSize;
    packed.bounds = localBounds.translated(rendered.bounds.topLeft());
    packed.packedMask = QByteArray(stride * localBounds.height(), '\0');
    for (int y = 0; y < localBounds.height(); ++y)
    {
        const uchar *input = rendered.image.constScanLine(localBounds.y() + y);
        auto *output = reinterpret_cast<uchar *>(
            packed.packedMask.data() + static_cast<qsizetype>(y) * stride);
        for (int x = 0; x < localBounds.width(); ++x)
        {
            if (input[localBounds.x() + x] >= 128)
            {
                output[x / 8] |= static_cast<uchar>(0x80U >> (x % 8));
            }
        }
    }
    return packed;
}

}

SamplingMode samplingForSelectionTransform(const QTransform &transform)
{
    return samplingForTransformImpl(transform);
}

std::optional<PackedMaskRegion> packBinaryMask(const QImage &mask)
{
    if (mask.isNull() || mask.format() != QImage::Format_Grayscale8)
    {
        return std::nullopt;
    }
    const QRect bounds = selectionBounds(mask);
    if (bounds.isEmpty())
    {
        return std::nullopt;
    }
    const qsizetype stride = packedStride(bounds.width());
    if (stride <= 0
        || stride > std::numeric_limits<qsizetype>::max() / bounds.height())
    {
        return std::nullopt;
    }
    PackedMaskRegion region;
    region.canvasSize = mask.size();
    region.bounds = bounds;
    region.packedMask = QByteArray(stride * bounds.height(), '\0');
    const int boundsWidth = bounds.width();
    const int boundsHeight = bounds.height();
    const int boundsX = bounds.x();
    const int boundsY = bounds.y();
    uchar *packed = reinterpret_cast<uchar *>(region.packedMask.data());
    for (int y = 0; y < boundsHeight; ++y)
    {
        const uchar *source = mask.constScanLine(boundsY + y) + boundsX;
        uchar *target = packed + static_cast<qsizetype>(y) * stride;
        for (int x = 0; x < boundsWidth; ++x)
        {
            if (source[x] >= 128)
            {
                target[x / 8] |= static_cast<uchar>(0x80U >> (x % 8));
            }
        }
    }
    return region;
}

bool isValidPackedMaskRegion(const PackedMaskRegion &region)
{
    if (region.canvasSize.width() < DocumentLimits::minimumCanvasEdge
        || region.canvasSize.height() < DocumentLimits::minimumCanvasEdge
        || region.canvasSize.width() > DocumentLimits::maximumCanvasEdge
        || region.canvasSize.height() > DocumentLimits::maximumCanvasEdge
        || region.bounds.isEmpty()
        || !QRect(QPoint(), region.canvasSize).contains(region.bounds))
    {
        return false;
    }
    const qsizetype stride = packedStride(region.bounds.width());
    if (stride <= 0
        || stride
               > std::numeric_limits<qsizetype>::max() / region.bounds.height()
        || region.packedMask.size() != stride * region.bounds.height())
    {
        return false;
    }
    const int remainder = region.bounds.width() % 8;
    if (remainder != 0)
    {
        const uchar unusedMask =
            static_cast<uchar>((1U << (8 - remainder)) - 1U);
        for (int y = 0; y < region.bounds.height(); ++y)
        {
            const uchar last = static_cast<uchar>(region.packedMask.at(
                static_cast<qsizetype>(y) * stride + stride - 1));
            if ((last & unusedMask) != 0)
            {
                return false;
            }
        }
    }
    return std::any_of(region.packedMask.cbegin(),
        region.packedMask.cend(),
        [](char byte)
        {
            return byte != 0;
        });
}

bool packedMaskContains(
    const PackedMaskRegion &region, int documentX, int documentY)
{
    if (!region.bounds.contains(documentX, documentY))
    {
        return false;
    }
    const int localX = documentX - region.bounds.x();
    const int localY = documentY - region.bounds.y();
    const qsizetype stride = packedStride(region.bounds.width());
    const uchar byte = static_cast<uchar>(region.packedMask.at(
        static_cast<qsizetype>(localY) * stride + localX / 8));
    return (byte & static_cast<uchar>(0x80U >> (localX % 8))) != 0;
}

QImage unpackBinaryMask(const PackedMaskRegion &region)
{
    if (!isValidPackedMaskRegion(region))
    {
        return {};
    }
    QImage mask(region.canvasSize, QImage::Format_Grayscale8);
    if (mask.isNull())
    {
        return {};
    }
    mask.fill(0);
    const int boundsWidth = region.bounds.width();
    const int boundsHeight = region.bounds.height();
    const int boundsX = region.bounds.x();
    const int boundsY = region.bounds.y();
    const qsizetype stride = packedStride(boundsWidth);
    const auto *packed =
        reinterpret_cast<const uchar *>(region.packedMask.constData());
    for (int y = 0; y < boundsHeight; ++y)
    {
        const uchar *source = packed + static_cast<qsizetype>(y) * stride;
        uchar *line = mask.scanLine(boundsY + y) + boundsX;
        for (int x = 0; x < boundsWidth; ++x)
        {
            if (source[x / 8] & static_cast<uchar>(0x80U >> (x % 8)))
            {
                line[x] = 255;
            }
        }
    }
    return mask;
}

std::optional<PackedMaskRegion> transformedPackedMask(
    const PackedMaskRegion &region,
    const QSize &targetSize,
    const QTransform &transform,
    SelectionTransformMemoryStats *memoryStats)
{
    resetMemoryStats(memoryStats);
    if (!isValidPackedMaskRegion(region)
        || !validTransformTargetSize(targetSize) || !finiteTransform(transform)
        || !transform.isAffine() || !transform.isInvertible())
    {
        return std::nullopt;
    }

    const bool useMappedTarget = integralOrthogonalTransform(transform);
    const QRect sourceBounds =
        useMappedTarget ? region.bounds
                        : region.bounds.adjusted(-1, -1, 1, 1)
                              .intersected(fullImageBounds(region.canvasSize));
    const QRect targetBounds =
        useMappedTarget
            ? mappedTargetBounds(sourceBounds, targetSize, transform)
            : fullImageBounds(targetSize);
    if (targetBounds.isEmpty())
    {
        RenderedMaskRegion empty;
        empty.bounds = targetBounds;
        recordMemoryStats(
            memoryStats, region.bounds, empty, 0, !useMappedTarget);
        return std::nullopt;
    }

    QImage source = binarySourceFromPacked(region, sourceBounds);
    if (source.isNull())
    {
        return std::nullopt;
    }
    const RenderedMaskRegion rendered = renderBinarySource(source,
        sourceBounds,
        targetBounds,
        transform,
        false,
        QImage::Format_Grayscale8);
    if (rendered.image.isNull())
    {
        return std::nullopt;
    }
    std::optional<PackedMaskRegion> packed =
        packRenderedRegion(rendered, targetSize);
    recordMemoryStats(memoryStats,
        sourceBounds,
        rendered,
        packed ? static_cast<quint64>(packed->packedMask.size()) : 0,
        !useMappedTarget);
    return packed;
}

std::optional<PixelSelectionOp> makePixelSelectionOp(
    const QImage &selectionMask,
    const QTransform &transform,
    bool clearSource,
    bool drawDestination)
{
    return makePixelSelectionOp(selectionMask,
        transform,
        clearSource,
        drawDestination,
        samplingForSelectionTransform(transform));
}

std::optional<PixelSelectionOp> makePixelSelectionOp(
    const QImage &selectionMask,
    const QTransform &transform,
    bool clearSource,
    bool drawDestination,
    SamplingMode sampling)
{
    if (selectionMask.isNull()
        || selectionMask.format() != QImage::Format_Grayscale8
        || !finiteTransform(transform) || (!clearSource && !drawDestination))
    {
        return std::nullopt;
    }
    const std::optional<PackedMaskRegion> packed =
        packBinaryMask(selectionMask);
    if (!packed)
    {
        return std::nullopt;
    }

    PixelSelectionOp operation;
    operation.canvasSize = selectionMask.size();
    operation.sourceBounds = packed->bounds;
    operation.transform = transform;
    operation.sampling = sampling;
    operation.clearSource = clearSource;
    operation.drawDestination = drawDestination;
    operation.packedMask = packed->packedMask;
    return isValidPixelSelectionOp(operation)
               ? std::optional<PixelSelectionOp>(std::move(operation))
               : std::nullopt;
}

bool isValidPixelSelectionOp(const PixelSelectionOp &operation)
{
    const qreal maximumCoordinate =
        DocumentLimits::maximumStoredCoordinateMagnitude;
    const auto safeCoefficient = [maximumCoordinate](qreal value)
    {
        return std::abs(value) <= maximumCoordinate;
    };
    const QRectF mappedBounds =
        operation.transform.mapRect(QRectF(operation.sourceBounds));
    if (operation.canvasSize.width() < DocumentLimits::minimumCanvasEdge
        || operation.canvasSize.height() < DocumentLimits::minimumCanvasEdge
        || operation.canvasSize.width() > DocumentLimits::maximumCanvasEdge
        || operation.canvasSize.height() > DocumentLimits::maximumCanvasEdge
        || operation.sourceBounds.isEmpty()
        || !QRect(QPoint(), operation.canvasSize)
            .contains(operation.sourceBounds)
        || !finiteTransform(operation.transform)
        || !operation.transform.isInvertible()
        || !operation.transform.isAffine()
        || !safeCoefficient(operation.transform.m11())
        || !safeCoefficient(operation.transform.m12())
        || !safeCoefficient(operation.transform.m21())
        || !safeCoefficient(operation.transform.m22())
        || !safeCoefficient(operation.transform.dx())
        || !safeCoefficient(operation.transform.dy())
        || !std::isfinite(mappedBounds.left())
        || !std::isfinite(mappedBounds.top())
        || !std::isfinite(mappedBounds.right())
        || !std::isfinite(mappedBounds.bottom())
        || std::abs(mappedBounds.left()) > maximumCoordinate
        || std::abs(mappedBounds.top()) > maximumCoordinate
        || std::abs(mappedBounds.right()) > maximumCoordinate
        || std::abs(mappedBounds.bottom()) > maximumCoordinate
        || (operation.sampling != SamplingMode::Nearest
            && operation.sampling != SamplingMode::Smooth)
        || (!operation.clearSource && !operation.drawDestination))
    {
        return false;
    }
    const qsizetype stride = packedStride(operation.sourceBounds.width());
    if (stride <= 0
        || stride > std::numeric_limits<qsizetype>::max()
                        / operation.sourceBounds.height()
        || operation.packedMask.size()
               != stride * operation.sourceBounds.height())
    {
        return false;
    }
    const int remainder = operation.sourceBounds.width() % 8;
    if (remainder != 0)
    {
        const uchar unusedMask =
            static_cast<uchar>((1U << (8 - remainder)) - 1U);
        for (int y = 0; y < operation.sourceBounds.height(); ++y)
        {
            const uchar last = static_cast<uchar>(operation.packedMask.at(
                static_cast<qsizetype>(y) * stride + stride - 1));
            if ((last & unusedMask) != 0)
            {
                return false;
            }
        }
    }
    return !operation.packedMask.isEmpty()
           && std::any_of(operation.packedMask.cbegin(),
               operation.packedMask.cend(),
               [](char byte)
               {
                   return byte != 0;
               });
}

QImage transformedSelectionSupport(const QImage &selectionMask,
    const QSize &targetSize,
    const QTransform &transform,
    SamplingMode sampling,
    SelectionTransformMemoryStats *memoryStats)
{
    resetMemoryStats(memoryStats);
    if (selectionMask.isNull()
        || selectionMask.format() != QImage::Format_Grayscale8
        || !validTransformTargetSize(targetSize) || !finiteTransform(transform)
        || !transform.isAffine() || !transform.isInvertible()
        || (sampling != SamplingMode::Nearest
            && sampling != SamplingMode::Smooth))
    {
        return {};
    }
    const QRect bounds = selectionBounds(selectionMask);
    if (bounds.isEmpty())
    {
        return {};
    }

    const QRect targetBounds = ImageAffineTransformer::targetBounds(
        bounds, targetSize, transform, sampling);
    if (targetBounds.isEmpty())
    {
        QImage support(targetSize, QImage::Format_Grayscale8);
        if (support.isNull())
        {
            return {};
        }
        support.fill(0);
        RenderedMaskRegion empty;
        empty.bounds = targetBounds;
        recordMemoryStats(
            memoryStats, bounds, empty, support.sizeInBytes(), false);
        return support;
    }

    QImage source =
        binarySourceFromMask(selectionMask, bounds, QImage::Format_Grayscale8);
    if (source.isNull())
    {
        return {};
    }
    RenderedMaskRegion rendered;
    rendered.bounds = targetBounds;
    rendered.sourceBytes = source.sizeInBytes();
    rendered.image = ImageAffineTransformer::transformMask(
        source, bounds, targetBounds, transform, sampling);
    if (rendered.image.isNull())
    {
        return {};
    }
    rendered.targetBytes = rendered.image.sizeInBytes();
    rendered.renderPeakBytes = rendered.sourceBytes + rendered.targetBytes;
    QImage support = supportFromRenderedRegion(rendered, targetSize);
    if (support.isNull())
    {
        return {};
    }
    recordMemoryStats(
        memoryStats, bounds, rendered, support.sizeInBytes(), false);
    return support;
}

bool pixelSelectionContains(
    const PixelSelectionOp &operation, int documentX, int documentY)
{
    if (!operation.sourceBounds.contains(documentX, documentY))
    {
        return false;
    }
    const int localX = documentX - operation.sourceBounds.x();
    const int localY = documentY - operation.sourceBounds.y();
    const qsizetype stride = packedStride(operation.sourceBounds.width());
    const uchar byte = static_cast<uchar>(operation.packedMask.at(
        static_cast<qsizetype>(localY) * stride + localX / 8));
    return (byte & static_cast<uchar>(0x80U >> (localX % 8))) != 0;
}

QImage unpackPixelSelectionMask(const PixelSelectionOp &operation)
{
    if (!isValidPixelSelectionOp(operation))
    {
        return {};
    }
    QImage mask(operation.canvasSize, QImage::Format_Grayscale8);
    if (mask.isNull())
    {
        return {};
    }
    mask.fill(0);
    for (int y = operation.sourceBounds.top();
        y <= operation.sourceBounds.bottom();
        ++y)
    {
        uchar *line = mask.scanLine(y);
        for (int x = operation.sourceBounds.left();
            x <= operation.sourceBounds.right();
            ++x)
        {
            if (pixelSelectionContains(operation, x, y))
            {
                line[x] = 255;
            }
        }
    }
    return mask;
}

quint64 packedSelectionBytes(const Document &document)
{
    quint64 total = 0;
    QSet<quintptr> seenBackings;
    for (const Layer &layer : document.layers)
    {
        for (const Stroke &operation : layer.strokes)
        {
            const auto account = [&total, &seenBackings](
                                     const QByteArray &packed)
            {
                const quintptr backing =
                    reinterpret_cast<quintptr>(packed.constData());
                if (seenBackings.contains(backing))
                {
                    return true;
                }
                const quint64 bytes = static_cast<quint64>(packed.size());
                if (bytes > std::numeric_limits<quint64>::max() - total)
                {
                    return false;
                }
                seenBackings.insert(backing);
                total += bytes;
                return true;
            };
            if ((operation.pixelSelectionOp
                    && !account(operation.pixelSelectionOp->packedMask))
                || (operation.fillCoverage
                    && !account(operation.fillCoverage->packedMask)))
            {
                return std::numeric_limits<quint64>::max();
            }
        }
    }
    return total;
}

bool isValidReframeOp(const ReframeOp &operation)
{
    const auto validSize = [](const QSize &size)
    {
        return size.width() >= DocumentLimits::minimumCanvasEdge
               && size.height() >= DocumentLimits::minimumCanvasEdge
               && size.width() <= DocumentLimits::maximumCanvasEdge
               && size.height() <= DocumentLimits::maximumCanvasEdge;
    };
    if (!validSize(operation.sourceSize) || !validSize(operation.targetSize)
        || (operation.sourceSize == operation.targetSize
            && operation.mode == ReframeMode::Image)
        || (operation.sampling != SamplingMode::Nearest
            && operation.sampling != SamplingMode::Smooth)
        || std::abs(static_cast<qreal>(operation.contentOffset.x()))
               > DocumentLimits::maximumStoredCoordinateMagnitude
        || std::abs(static_cast<qreal>(operation.contentOffset.y()))
               > DocumentLimits::maximumStoredCoordinateMagnitude)
    {
        return false;
    }
    switch (operation.mode)
    {
    case ReframeMode::Canvas:
        return operation.sampling == SamplingMode::Nearest;
    case ReframeMode::Image:
        return operation.contentOffset.isNull();
    }
    return false;
}

}

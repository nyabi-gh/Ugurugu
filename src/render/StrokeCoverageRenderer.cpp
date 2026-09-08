// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "render/StrokeCoverageRenderer.hpp"

#include "document/DocumentLimits.hpp"
#include "document/DocumentOperations.hpp"
#include "document/SelectionOperation.hpp"
#include "render/ImageAffineTransformer.hpp"
#include "render/ImageResampler.hpp"
#include "render/StrokeMotionModel.hpp"

#include <QPainter>

#include <algorithm>
#include <cmath>
#include <limits>
#include <optional>

namespace ugurugu
{
namespace
{

struct CoverageFrame
{
    QImage pixels;
    QRect bounds;
    QSize canvasSize;
};

constexpr int coverageCellSize = 128;
constexpr int maximumIndexedCellsPerEffect = 128;
constexpr int coverageEffectPadding = 8;

RenderEngine::StrokeCoveragePlan::Epoch coverageEpoch(const QSize &canvasSize)
{
    RenderEngine::StrokeCoveragePlan::Epoch epoch;
    epoch.canvasSize = canvasSize;
    epoch.columns =
        (canvasSize.width() + coverageCellSize - 1) / coverageCellSize;
    return epoch;
}

QRect coverageCellRange(const QRect &bounds, const QSize &canvasSize)
{
    const QRect clipped = bounds.intersected(QRect(QPoint(), canvasSize));
    if (clipped.isEmpty())
    {
        return {};
    }
    return QRect(QPoint(clipped.left() / coverageCellSize,
                     clipped.top() / coverageCellSize),
        QPoint(clipped.right() / coverageCellSize,
            clipped.bottom() / coverageCellSize));
}

void addCoverageEffect(RenderEngine::StrokeCoveragePlan::Epoch &epoch,
    int strokeIndex,
    const QRect &bounds,
    bool global)
{
    const QRect cells = coverageCellRange(bounds, epoch.canvasSize);
    const qint64 cellCount = qint64(cells.width()) * qint64(cells.height());
    if (cells.isEmpty())
    {
        return;
    }
    if (global || cellCount > maximumIndexedCellsPerEffect)
    {
        epoch.globalEffectIndexes.append(strokeIndex);
        return;
    }
    for (int y = cells.top(); y <= cells.bottom(); ++y)
    {
        for (int x = cells.left(); x <= cells.right(); ++x)
        {
            epoch.effectIndexesByCell[y * epoch.columns + x].append(
                strokeIndex);
        }
    }
}

int nextCoverageEffect(const RenderEngine::StrokeCoveragePlan &plan,
    int epochIndex,
    int afterIndex,
    const QRect &bounds,
    RenderEngine::StrokeCoverageStats *stats = nullptr)
{
    if (epochIndex < 0 || epochIndex >= plan.epochs.size() || bounds.isEmpty())
    {
        return -1;
    }
    const RenderEngine::StrokeCoveragePlan::Epoch &epoch =
        plan.epochs[epochIndex];
    int next = std::numeric_limits<int>::max();
    const auto consider = [&](const QVector<int> &indexes)
    {
        const auto found =
            std::upper_bound(indexes.cbegin(), indexes.cend(), afterIndex);
        if (found != indexes.cend())
        {
            next = std::min(next, *found);
        }
    };
    consider(epoch.globalEffectIndexes);
    const QRect cells = coverageCellRange(bounds, epoch.canvasSize);
    for (int y = cells.top(); y <= cells.bottom(); ++y)
    {
        for (int x = cells.left(); x <= cells.right(); ++x)
        {
            const auto indexes =
                epoch.effectIndexesByCell.constFind(y * epoch.columns + x);
            if (indexes != epoch.effectIndexesByCell.cend())
            {
                consider(indexes.value());
            }
        }
    }
    if (next == std::numeric_limits<int>::max())
    {
        return -1;
    }
    if (stats)
    {
        ++stats->effectCandidatesExamined;
    }
    return next;
}

QRect boundedCoverageRect(const QRect &bounds,
    const QTransform &transform,
    const QSize &canvasSize,
    qreal margin)
{
    if (bounds.isEmpty() || !canvasSize.isValid())
    {
        return {};
    }
    const QRectF mapped = transform.mapRect(QRectF(bounds));
    if (!std::isfinite(mapped.left()) || !std::isfinite(mapped.top())
        || !std::isfinite(mapped.right()) || !std::isfinite(mapped.bottom()))
    {
        return QRect(QPoint(), canvasSize);
    }
    const qreal left =
        std::clamp(mapped.left() - margin, 0.0, qreal(canvasSize.width()));
    const qreal top =
        std::clamp(mapped.top() - margin, 0.0, qreal(canvasSize.height()));
    const qreal right =
        std::clamp(mapped.right() + margin, 0.0, qreal(canvasSize.width()));
    const qreal bottom =
        std::clamp(mapped.bottom() + margin, 0.0, qreal(canvasSize.height()));
    const int pixelLeft = static_cast<int>(std::floor(left));
    const int pixelTop = static_cast<int>(std::floor(top));
    const int pixelRight = static_cast<int>(std::ceil(right));
    const int pixelBottom = static_cast<int>(std::ceil(bottom));
    return pixelRight > pixelLeft && pixelBottom > pixelTop
               ? QRect(pixelLeft,
                     pixelTop,
                     pixelRight - pixelLeft,
                     pixelBottom - pixelTop)
               : QRect();
}

std::optional<QRect> binaryMaskBounds(
    const QImage &mask, const QSize &canvasSize)
{
    if (mask.isNull() || mask.size() != canvasSize
        || mask.format() != QImage::Format_Grayscale8)
    {
        return std::nullopt;
    }
    int left = mask.width();
    int top = mask.height();
    int right = -1;
    int bottom = -1;
    for (int y = 0; y < mask.height(); ++y)
    {
        const uchar *line = mask.constScanLine(y);
        for (int x = 0; x < mask.width(); ++x)
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
    return right >= left && bottom >= top
               ? QRect(QPoint(left, top), QPoint(right, bottom))
               : QRect();
}

std::optional<QRect> cachedBinaryMaskBounds(
    const QImage &mask, const QSize &canvasSize, QHash<qint64, QRect> *cache)
{
    if (!cache || mask.isNull() || mask.size() != canvasSize
        || mask.format() != QImage::Format_Grayscale8)
    {
        return binaryMaskBounds(mask, canvasSize);
    }
    const qint64 key = mask.cacheKey();
    const auto found = cache->constFind(key);
    if (found != cache->cend())
    {
        return found.value();
    }
    const std::optional<QRect> bounds = binaryMaskBounds(mask, canvasSize);
    if (bounds)
    {
        cache->insert(key, *bounds);
    }
    return bounds;
}

QRect primitiveCoverageBounds(const Document &document,
    const Stroke &stroke,
    const QSize &canvasSize,
    QHash<qint64, QRect> *maskBoundsCache)
{
    const QRect canvasBounds(QPoint(), canvasSize);
    if (!canvasSize.isValid())
    {
        return {};
    }
    if (stroke.mode == StrokeMode::Image)
    {
        if (!stroke.imageOp)
        {
            return {};
        }
        const auto asset =
            document.rasterAssets.constFind(stroke.imageOp->assetId);
        return asset == document.rasterAssets.cend()
                   ? QRect()
                   : ImageAffineTransformer::targetBounds(
                         QRect(QPoint(), asset->size),
                         canvasSize,
                         stroke.imageOp->transform,
                         stroke.imageOp->sampling);
    }
    if (stroke.points.isEmpty() || !std::isfinite(stroke.width)
        || !isValidBrushSettings(stroke.brush))
    {
        return {};
    }
    QRect bounds;
    if (stroke.mode == StrokeMode::Fill)
    {
        if (stroke.fillCoverage)
        {
            bounds = stroke.fillCoverage->bounds.adjusted(-1, -1, 1, 1)
                         .intersected(canvasBounds);
        }
        else if (stroke.fillMask.isNull())
        {
            bounds = canvasBounds;
        }
        else
        {
            const std::optional<QRect> fillBounds = cachedBinaryMaskBounds(
                stroke.fillMask, canvasSize, maskBoundsCache);
            if (!fillBounds)
            {
                return canvasBounds;
            }
            bounds = fillBounds->isEmpty() ? QRect()
                                           : fillBounds->adjusted(-1, -1, 1, 1)
                                                 .intersected(canvasBounds);
        }
        if (!stroke.clipMask.isNull())
        {
            const std::optional<QRect> clipBounds = cachedBinaryMaskBounds(
                stroke.clipMask, canvasSize, maskBoundsCache);
            if (!clipBounds)
            {
                return canvasBounds;
            }
            bounds = clipBounds->isEmpty() ? QRect()
                                           : bounds.intersected(*clipBounds);
        }
    }
    else
    {
        qreal left = std::numeric_limits<qreal>::max();
        qreal top = std::numeric_limits<qreal>::max();
        qreal right = std::numeric_limits<qreal>::lowest();
        qreal bottom = std::numeric_limits<qreal>::lowest();
        for (const StrokePoint &point : stroke.points)
        {
            if (!std::isfinite(point.position.x())
                || !std::isfinite(point.position.y()))
            {
                return canvasBounds;
            }
            left = std::min(left, point.position.x());
            top = std::min(top, point.position.y());
            right = std::max(right, point.position.x());
            bottom = std::max(bottom, point.position.y());
        }
        const qreal displacement =
            StrokeMotionModel::maximumDisplacement(stroke.width,
                document.wobbleAmount * stroke.brush.wobbleScale,
                document.motion);
        const qreal brushReach = std::max(0.5, stroke.width) * 1.025 * 2.1;
        const qreal margin = displacement + brushReach + 4.0;
        bounds = QRectF(QPointF(left, top), QPointF(right, bottom))
                     .normalized()
                     .adjusted(-margin, -margin, margin, margin)
                     .toAlignedRect()
                     .intersected(canvasBounds);
    }
    if (stroke.visibilityClip)
    {
        bounds = bounds.intersected(*stroke.visibilityClip);
    }
    return bounds.intersected(canvasBounds);
}

void noteCoverageImage(
    RenderEngine::StrokeCoverageStats *stats, const QImage &image)
{
    if (!stats || image.isNull())
    {
        return;
    }
    stats->maximumExplicitImageBytes = std::max(
        stats->maximumExplicitImageBytes, quint64(image.sizeInBytes()));
}

QRect alphaBounds(const QImage &image)
{
    int left = image.width();
    int top = image.height();
    int right = -1;
    int bottom = -1;
    for (int y = 0; y < image.height(); ++y)
    {
        const auto *line =
            reinterpret_cast<const QRgb *>(image.constScanLine(y));
        for (int x = 0; x < image.width(); ++x)
        {
            if (qAlpha(line[x]) == 0)
            {
                continue;
            }
            left = std::min(left, x);
            top = std::min(top, y);
            right = std::max(right, x);
            bottom = std::max(bottom, y);
        }
    }
    return right >= left && bottom >= top
               ? QRect(QPoint(left, top), QPoint(right, bottom))
               : QRect();
}

bool trimCoverageFrame(CoverageFrame &frame,
    RenderEngine::StrokeCoverageStats *stats,
    int padding = 4)
{
    if (frame.pixels.isNull() || frame.pixels.size() != frame.bounds.size()
        || frame.pixels.format() != QImage::Format_ARGB32_Premultiplied)
    {
        return false;
    }
    const QRect localAlpha = alphaBounds(frame.pixels);
    if (localAlpha.isEmpty())
    {
        frame.pixels = {};
        frame.bounds = {};
        return true;
    }
    const QRect canvasBounds(QPoint(), frame.canvasSize);
    const QRect globalAlpha = localAlpha.translated(frame.bounds.topLeft());
    const QRect padded =
        globalAlpha.adjusted(-padding, -padding, padding, padding)
            .intersected(canvasBounds);
    if (padded == frame.bounds)
    {
        return true;
    }
    QImage trimmed(padded.size(), QImage::Format_ARGB32_Premultiplied);
    if (trimmed.isNull())
    {
        return false;
    }
    trimmed.fill(Qt::transparent);
    QPainter painter(&trimmed);
    painter.setCompositionMode(QPainter::CompositionMode_Source);
    painter.drawImage(frame.bounds.topLeft() - padded.topLeft(), frame.pixels);
    painter.end();
    noteCoverageImage(stats, trimmed);
    frame.pixels = std::move(trimmed);
    frame.bounds = padded;
    return true;
}

bool expandCoverageFrame(CoverageFrame &frame,
    const QRect &bounds,
    RenderEngine::StrokeCoverageStats *stats)
{
    const QRect canvasBounds(QPoint(), frame.canvasSize);
    const QRect expanded =
        frame.bounds.united(bounds).intersected(canvasBounds);
    if (expanded == frame.bounds)
    {
        return true;
    }
    if (expanded.isEmpty() || !canvasBounds.contains(expanded))
    {
        return false;
    }
    QImage pixels(expanded.size(), QImage::Format_ARGB32_Premultiplied);
    if (pixels.isNull())
    {
        return false;
    }
    pixels.fill(Qt::transparent);
    if (!frame.bounds.isEmpty())
    {
        QPainter painter(&pixels);
        painter.setCompositionMode(QPainter::CompositionMode_Source);
        painter.drawImage(
            frame.bounds.topLeft() - expanded.topLeft(), frame.pixels);
    }
    noteCoverageImage(stats, pixels);
    frame.pixels = std::move(pixels);
    frame.bounds = expanded;
    return true;
}

bool applyPixelSelectionOperation(CoverageFrame &frame,
    const PixelSelectionOp &operation,
    RenderEngine::StrokeCoverageStats *stats)
{
    if (!isValidPixelSelectionOp(operation)
        || operation.canvasSize != frame.canvasSize)
    {
        return false;
    }
    if (stats)
    {
        ++stats->pixelSelectionOperationsReplayed;
    }
    if (frame.bounds.isEmpty())
    {
        return true;
    }

    const QRect payloadBounds =
        frame.bounds.intersected(operation.sourceBounds);
    QImage payload;
    bool hasPayloadPixels = false;
    if (operation.drawDestination && !payloadBounds.isEmpty())
    {
        payload =
            QImage(payloadBounds.size(), QImage::Format_ARGB32_Premultiplied);
        if (payload.isNull())
        {
            return false;
        }
        payload.fill(Qt::transparent);
        for (int y = payloadBounds.top(); y <= payloadBounds.bottom(); ++y)
        {
            const auto *sourceLine = reinterpret_cast<const QRgb *>(
                frame.pixels.constScanLine(y - frame.bounds.top()));
            auto *payloadLine = reinterpret_cast<QRgb *>(
                payload.scanLine(y - payloadBounds.top()));
            for (int x = payloadBounds.left(); x <= payloadBounds.right(); ++x)
            {
                if (!pixelSelectionContains(operation, x, y))
                {
                    continue;
                }
                const QRgb pixel = sourceLine[x - frame.bounds.left()];
                payloadLine[x - payloadBounds.left()] = pixel;
                hasPayloadPixels = hasPayloadPixels || qAlpha(pixel) != 0;
            }
        }
        noteCoverageImage(stats, payload);
    }

    if (operation.clearSource && !payloadBounds.isEmpty())
    {
        for (int y = payloadBounds.top(); y <= payloadBounds.bottom(); ++y)
        {
            auto *line = reinterpret_cast<QRgb *>(
                frame.pixels.scanLine(y - frame.bounds.top()));
            for (int x = payloadBounds.left(); x <= payloadBounds.right(); ++x)
            {
                if (pixelSelectionContains(operation, x, y))
                {
                    line[x - frame.bounds.left()] = 0;
                }
            }
        }
        if (!trimCoverageFrame(frame, stats))
        {
            return false;
        }
    }

    if (!hasPayloadPixels)
    {
        return true;
    }
    const QRect movedBounds =
        ImageAffineTransformer::targetBounds(payloadBounds,
            operation.canvasSize,
            operation.transform,
            operation.sampling);
    const QRect targetBounds =
        frame.bounds.united(movedBounds)
            .intersected(QRect(QPoint(), frame.canvasSize));
    if (targetBounds.isEmpty())
    {
        frame.pixels = {};
        frame.bounds = {};
        return true;
    }
    QImage target(targetBounds.size(), QImage::Format_ARGB32_Premultiplied);
    if (target.isNull())
    {
        return false;
    }
    target.fill(Qt::transparent);
    QPainter painter(&target);
    painter.setRenderHint(QPainter::Antialiasing, false);
    painter.setCompositionMode(QPainter::CompositionMode_Source);
    if (!frame.bounds.isEmpty())
    {
        painter.drawImage(
            frame.bounds.topLeft() - targetBounds.topLeft(), frame.pixels);
    }
    painter.end();
    if (!ImageAffineTransformer::compositeSourceOver(target,
            targetBounds,
            payload,
            payloadBounds,
            operation.transform,
            operation.sampling))
    {
        return false;
    }
    noteCoverageImage(stats, target);
    frame.pixels = std::move(target);
    frame.bounds = targetBounds;
    return trimCoverageFrame(frame, stats);
}

bool applyReframeOperation(CoverageFrame &frame,
    const ReframeOp &operation,
    RenderEngine::StrokeCoverageStats *stats)
{
    if (!isValidReframeOp(operation)
        || operation.sourceSize != frame.canvasSize)
    {
        return false;
    }
    if (stats)
    {
        ++stats->reframeOperationsReplayed;
    }
    if (frame.bounds.isEmpty())
    {
        frame.canvasSize = operation.targetSize;
        return true;
    }

    QRect targetBounds;
    QTransform transform;
    if (operation.mode == ReframeMode::Canvas)
    {
        targetBounds = frame.bounds.translated(operation.contentOffset)
                           .intersected(QRect(QPoint(), operation.targetSize));
    }
    else
    {
        transform.scale(
            qreal(operation.targetSize.width()) / operation.sourceSize.width(),
            qreal(operation.targetSize.height())
                / operation.sourceSize.height());
        targetBounds = boundedCoverageRect(frame.bounds,
            transform,
            operation.targetSize,
            operation.sampling == SamplingMode::Smooth ? 4.0 : 1.0);
    }
    if (targetBounds.isEmpty())
    {
        frame.pixels = {};
        frame.bounds = {};
        frame.canvasSize = operation.targetSize;
        return true;
    }
    if (operation.mode == ReframeMode::Image)
    {
        QImage target = ImageResampler::resampleRegion(frame.pixels,
            frame.bounds,
            operation.sourceSize,
            targetBounds,
            operation.targetSize,
            operation.sampling);
        if (target.isNull())
        {
            return false;
        }
        noteCoverageImage(stats, target);
        frame.pixels = std::move(target);
        frame.bounds = targetBounds;
        frame.canvasSize = operation.targetSize;
        return trimCoverageFrame(frame, stats);
    }
    QImage target(targetBounds.size(), QImage::Format_ARGB32_Premultiplied);
    if (target.isNull())
    {
        return false;
    }
    target.fill(Qt::transparent);
    QPainter painter(&target);
    painter.setRenderHint(QPainter::Antialiasing, false);
    painter.setRenderHint(QPainter::SmoothPixmapTransform,
        operation.sampling == SamplingMode::Smooth);
    painter.setCompositionMode(QPainter::CompositionMode_Source);
    painter.drawImage(frame.bounds.topLeft() + operation.contentOffset
                          - targetBounds.topLeft(),
        frame.pixels);
    painter.end();
    noteCoverageImage(stats, target);
    frame.pixels = std::move(target);
    frame.bounds = targetBounds;
    frame.canvasSize = operation.targetSize;
    return trimCoverageFrame(frame, stats);
}

bool renderFillCoverage(CoverageFrame &frame,
    const Stroke &stroke,
    RenderEngine::StrokeCoverageStats *stats)
{
    const QImage packedCoverage =
        stroke.fillCoverage ? unpackBinaryMask(*stroke.fillCoverage) : QImage();
    const QImage &fillMask =
        stroke.fillCoverage ? packedCoverage : stroke.fillMask;
    if (frame.bounds.isEmpty()
        || !QRect(QPoint(), frame.canvasSize).contains(frame.bounds)
        || (!fillMask.isNull()
            && (fillMask.size() != frame.canvasSize
                || fillMask.format() != QImage::Format_Grayscale8))
        || (!stroke.clipMask.isNull()
            && (stroke.clipMask.size() != frame.canvasSize
                || stroke.clipMask.format() != QImage::Format_Grayscale8)))
    {
        return false;
    }
    frame.pixels =
        QImage(frame.bounds.size(), QImage::Format_ARGB32_Premultiplied);
    if (frame.pixels.isNull())
    {
        return false;
    }
    frame.pixels.fill(Qt::transparent);
    const QRgb fill =
        qPremultiply(QColor(255, 255, 255, stroke.color.alpha()).rgba());
    for (int y = frame.bounds.top(); y <= frame.bounds.bottom(); ++y)
    {
        auto *target = reinterpret_cast<QRgb *>(
            frame.pixels.scanLine(y - frame.bounds.top()));
        const uchar *fillLine =
            fillMask.isNull() ? nullptr : fillMask.constScanLine(y);
        const uchar *fillAbove =
            !fillLine || y == 0 ? nullptr : fillMask.constScanLine(y - 1);
        const uchar *fillBelow = !fillLine || y == frame.canvasSize.height() - 1
                                     ? nullptr
                                     : fillMask.constScanLine(y + 1);
        const uchar *clipLine = stroke.clipMask.isNull()
                                    ? nullptr
                                    : stroke.clipMask.constScanLine(y);
        for (int x = frame.bounds.left(); x <= frame.bounds.right(); ++x)
        {
            if ((stroke.visibilityClip
                    && !stroke.visibilityClip->contains(x, y))
                || (clipLine && clipLine[x] < 128))
            {
                continue;
            }
            const bool includeEdgeCoverage =
                !stroke.fillCoverage || stroke.brush.antialiasing;
            const bool covered =
                !fillLine || fillLine[x] >= 128
                || (includeEdgeCoverage
                    && ((x > 0 && fillLine[x - 1] >= 128)
                        || (x < frame.canvasSize.width() - 1
                            && fillLine[x + 1] >= 128)
                        || (fillAbove && fillAbove[x] >= 128)
                        || (fillBelow && fillBelow[x] >= 128)));
            if (covered)
            {
                target[x - frame.bounds.left()] = fill;
            }
        }
    }
    noteCoverageImage(stats, frame.pixels);
    return trimCoverageFrame(frame, stats);
}

}

RenderEngine::StrokeCoveragePlan StrokeCoverageRenderer::prepare(
    const Document &document, const Layer &layer)
{
    RenderEngine::StrokeCoveragePlan plan;
    if (!document.size.isValid() || layer.kind != LayerKind::Paint)
    {
        return plan;
    }
    QSize canvasSize = layer.initialCanvasSize.isValid()
                           ? layer.initialCanvasSize
                           : DocumentOperations::initialCanvasSize(
                                 layer.strokes, document.size);
    if (!canvasSize.isValid())
    {
        return plan;
    }
    // Bounds carry the wobble margin, so they have to be measured against the
    // motion this layer actually renders with rather than the document's.
    const Document layerDocument = documentForLayer(document, layer);
    plan.canvasBefore.resize(layer.strokes.size());
    plan.primitiveBounds.resize(layer.strokes.size());
    plan.epochBefore.resize(layer.strokes.size());
    plan.sectionBefore.resize(layer.strokes.size());
    plan.epochs.append(coverageEpoch(canvasSize));
    int epochIndex = 0;
    int sectionIndex = 0;
    QHash<qint64, QRect> maskBoundsCache;
    for (int index = 0; index < layer.strokes.size(); ++index)
    {
        const Stroke &stroke = layer.strokes[index];
        plan.canvasBefore[index] = canvasSize;
        plan.epochBefore[index] = epochIndex;
        plan.sectionBefore[index] = sectionIndex;
        if (stroke.mode == StrokeMode::CompositeBoundary)
        {
            if (stroke.pixelSelectionOp || stroke.reframeOp || stroke.imageOp
                || !stroke.points.isEmpty())
            {
                return {};
            }
            ++sectionIndex;
            continue;
        }
        if (stroke.mode == StrokeMode::Paint || stroke.mode == StrokeMode::Erase
            || stroke.mode == StrokeMode::Fill
            || stroke.mode == StrokeMode::Image)
        {
            if (stroke.pixelSelectionOp || stroke.reframeOp
                || (stroke.mode == StrokeMode::Image && !stroke.imageOp)
                || (stroke.mode != StrokeMode::Image && stroke.imageOp))
            {
                return {};
            }
            plan.primitiveBounds[index] = primitiveCoverageBounds(
                layerDocument, stroke, canvasSize, &maskBoundsCache);
            if (stroke.mode == StrokeMode::Erase)
            {
                addCoverageEffect(plan.epochs[epochIndex],
                    index,
                    plan.primitiveBounds[index],
                    false);
            }
            continue;
        }
        if (stroke.mode == StrokeMode::PixelSelection)
        {
            if (!stroke.pixelSelectionOp || stroke.reframeOp
                || !isValidPixelSelectionOp(*stroke.pixelSelectionOp)
                || stroke.pixelSelectionOp->canvasSize != canvasSize)
            {
                return {};
            }
            addCoverageEffect(plan.epochs[epochIndex],
                index,
                stroke.pixelSelectionOp->sourceBounds,
                false);
            continue;
        }
        if (stroke.mode != StrokeMode::Reframe || !stroke.reframeOp
            || stroke.pixelSelectionOp || !isValidReframeOp(*stroke.reframeOp)
            || stroke.reframeOp->sourceSize != canvasSize)
        {
            return {};
        }
        addCoverageEffect(
            plan.epochs[epochIndex], index, QRect(QPoint(), canvasSize), true);
        canvasSize = stroke.reframeOp->targetSize;
        plan.epochs.append(coverageEpoch(canvasSize));
        ++epochIndex;
    }
    plan.tailSection = sectionIndex;
    plan.valid = canvasSize == document.size;
    return plan;
}

QRect StrokeCoverageRenderer::conservativeBounds(const Document &document,
    const Layer &layer,
    int strokeIndex,
    const RenderEngine::StrokeCoveragePlan &plan)
{
    if (!plan.valid || strokeIndex < 0 || strokeIndex >= layer.strokes.size()
        || plan.canvasBefore.size() != layer.strokes.size()
        || plan.primitiveBounds.size() != layer.strokes.size()
        || plan.epochBefore.size() != layer.strokes.size()
        || plan.sectionBefore.size() != layer.strokes.size())
    {
        return {};
    }
    QRect bounds = plan.primitiveBounds[strokeIndex];
    QSize canvasSize = plan.canvasBefore[strokeIndex];
    int epochIndex = plan.epochBefore[strokeIndex];
    int afterIndex = strokeIndex;
    while (!bounds.isEmpty())
    {
        const int effect =
            nextCoverageEffect(plan, epochIndex, afterIndex, bounds);
        if (effect < 0)
        {
            break;
        }
        if (effect <= afterIndex || effect >= layer.strokes.size())
        {
            return {};
        }
        afterIndex = effect;
        const Stroke &operation = layer.strokes[effect];
        const bool reachesSource =
            plan.sectionBefore[effect] == plan.sectionBefore[strokeIndex]
            || plan.sectionBefore[effect] == plan.tailSection;
        if (operation.mode == StrokeMode::PixelSelection)
        {
            if (!reachesSource)
            {
                continue;
            }
            if (!operation.pixelSelectionOp
                || operation.pixelSelectionOp->canvasSize != canvasSize)
            {
                return {};
            }
            const PixelSelectionOp &pixel = *operation.pixelSelectionOp;
            const QRect sourceIntersection =
                bounds.intersected(pixel.sourceBounds);
            if (sourceIntersection.isEmpty())
            {
                continue;
            }
            QRect moved;
            if (pixel.drawDestination)
            {
                moved = ImageAffineTransformer::targetBounds(sourceIntersection,
                    canvasSize,
                    pixel.transform,
                    pixel.sampling);
            }
            bounds =
                bounds.united(moved).intersected(QRect(QPoint(), canvasSize));
        }
        else if (operation.mode == StrokeMode::Reframe)
        {
            if (!operation.reframeOp
                || operation.reframeOp->sourceSize != canvasSize)
            {
                return {};
            }
            const ReframeOp &reframe = *operation.reframeOp;
            if (reframe.mode == ReframeMode::Canvas)
            {
                bounds = bounds.translated(reframe.contentOffset)
                             .intersected(QRect(QPoint(), reframe.targetSize));
            }
            else
            {
                QTransform transform;
                transform.scale(qreal(reframe.targetSize.width())
                                    / reframe.sourceSize.width(),
                    qreal(reframe.targetSize.height())
                        / reframe.sourceSize.height());
                bounds = boundedCoverageRect(bounds,
                    transform,
                    reframe.targetSize,
                    reframe.sampling == SamplingMode::Smooth ? 4.0 : 1.0);
            }
            canvasSize = reframe.targetSize;
            ++epochIndex;
        }
    }
    return canvasSize == document.size ? bounds : QRect();
}

RenderEngine::StrokeCoverageRegion StrokeCoverageRenderer::render(
    const Document &document,
    const Layer &layer,
    int strokeIndex,
    int frameIndex,
    const QRect &outputBounds,
    const RenderEngine::StrokeCoveragePlan &plan,
    RenderEngine::StrokeCoverageStats *stats)
{
    RenderEngine::StrokeCoverageRegion result;
    const QRect documentBounds(QPoint(), document.size);
    if (!document.size.isValid() || layer.kind != LayerKind::Paint
        || strokeIndex < 0 || strokeIndex >= layer.strokes.size()
        || outputBounds.isEmpty() || !documentBounds.contains(outputBounds))
    {
        return result;
    }
    const Stroke &source = layer.strokes[strokeIndex];
    if (source.mode != StrokeMode::Paint && source.mode != StrokeMode::Erase
        && source.mode != StrokeMode::Fill && source.mode != StrokeMode::Image)
    {
        return result;
    }

    const auto exactRegion = [&]()
    {
        RenderEngine::StrokeCoverageRegion exact;
        if (stats)
        {
            ++stats->fullCanvasFallbacks;
        }
        const QImage coverage = RenderEngine::renderStrokeCoverage(
            document, layer, strokeIndex, frameIndex);
        noteCoverageImage(stats, coverage);
        if (coverage.isNull())
        {
            return exact;
        }
        const QRect visible = alphaBounds(coverage).intersected(outputBounds);
        exact.valid = true;
        if (visible.isEmpty())
        {
            return exact;
        }
        exact.image = coverage.copy(visible);
        exact.bounds = visible;
        noteCoverageImage(stats, exact.image);
        if (exact.image.isNull())
        {
            return RenderEngine::StrokeCoverageRegion{};
        }
        return exact;
    };

    if (source.mode == StrokeMode::Image)
    {
        return exactRegion();
    }

    if (!plan.valid || plan.canvasBefore.size() != layer.strokes.size()
        || plan.primitiveBounds.size() != layer.strokes.size()
        || plan.epochBefore.size() != layer.strokes.size()
        || plan.sectionBefore.size() != layer.strokes.size())
    {
        return exactRegion();
    }
    CoverageFrame frame;
    frame.bounds = plan.primitiveBounds[strokeIndex];
    frame.canvasSize = plan.canvasBefore[strokeIndex];
    if (frame.bounds.isEmpty())
    {
        result.valid = true;
        if (stats)
        {
            ++stats->regionalRenders;
        }
        return result;
    }
    if (!QRect(QPoint(), frame.canvasSize).contains(frame.bounds))
    {
        return exactRegion();
    }
    if (source.mode == StrokeMode::Fill)
    {
        if (nextCoverageEffect(
                plan, plan.epochBefore[strokeIndex], strokeIndex, frame.bounds)
            < 0)
        {
            frame.bounds = frame.bounds.intersected(outputBounds);
        }
        if (frame.bounds.isEmpty())
        {
            result.valid = true;
            if (stats)
            {
                ++stats->regionalRenders;
            }
            return result;
        }
        if (!renderFillCoverage(frame, source, stats))
        {
            return exactRegion();
        }
    }
    else
    {
        frame.bounds = frame.bounds
                           .adjusted(-coverageEffectPadding,
                               -coverageEffectPadding,
                               coverageEffectPadding,
                               coverageEffectPadding)
                           .intersected(QRect(QPoint(), frame.canvasSize));
        Stroke probe = source;
        if (probe.mode == StrokeMode::Erase)
        {
            probe.mode = StrokeMode::Paint;
            probe.color = Qt::white;
        }
        else
        {
            probe.color = QColor(255, 255, 255, source.color.alpha());
        }
        frame.pixels =
            QImage(frame.bounds.size(), QImage::Format_ARGB32_Premultiplied);
        if (frame.pixels.isNull())
        {
            return exactRegion();
        }
        frame.pixels.fill(Qt::transparent);
        noteCoverageImage(stats, frame.pixels);
        Document epochDocument = documentForLayer(document, layer);
        epochDocument.size = frame.canvasSize;
        if (!RenderEngine::renderStrokesOnLayerRegion(
                frame.pixels, epochDocument, {probe}, frameIndex, frame.bounds)
            || !trimCoverageFrame(frame, stats))
        {
            return exactRegion();
        }
    }

    int epochIndex = plan.epochBefore[strokeIndex];
    int afterIndex = strokeIndex;
    while (!frame.bounds.isEmpty())
    {
        const int effect = nextCoverageEffect(
            plan, epochIndex, afterIndex, frame.bounds, stats);
        if (effect < 0)
        {
            break;
        }
        if (effect <= afterIndex || effect >= layer.strokes.size())
        {
            return exactRegion();
        }
        afterIndex = effect;
        const Stroke &operation = layer.strokes[effect];
        const bool reachesSource =
            plan.sectionBefore[effect] == plan.sectionBefore[strokeIndex]
            || plan.sectionBefore[effect] == plan.tailSection;
        if (operation.mode == StrokeMode::PixelSelection)
        {
            if (!reachesSource || !operation.pixelSelectionOp
                || !frame.bounds.intersects(
                    operation.pixelSelectionOp->sourceBounds))
            {
                continue;
            }
            if (!applyPixelSelectionOperation(
                    frame, *operation.pixelSelectionOp, stats))
            {
                return exactRegion();
            }
        }
        else if (operation.mode == StrokeMode::Reframe)
        {
            if (!operation.reframeOp
                || !applyReframeOperation(frame, *operation.reframeOp, stats))
            {
                return exactRegion();
            }
            ++epochIndex;
        }
        else if (operation.mode == StrokeMode::Erase && !frame.bounds.isEmpty())
        {
            if (!reachesSource
                || !plan.primitiveBounds[effect].intersects(frame.bounds))
            {
                continue;
            }
            if (!expandCoverageFrame(frame,
                    frame.bounds.adjusted(-coverageEffectPadding,
                        -coverageEffectPadding,
                        coverageEffectPadding,
                        coverageEffectPadding),
                    stats))
            {
                return exactRegion();
            }
            Document epochDocument = documentForLayer(document, layer);
            epochDocument.size = frame.canvasSize;
            if (!RenderEngine::renderStrokesOnLayerRegion(frame.pixels,
                    epochDocument,
                    {operation},
                    frameIndex,
                    frame.bounds)
                || !trimCoverageFrame(frame, stats))
            {
                return exactRegion();
            }
            if (stats)
            {
                ++stats->eraseOperationsReplayed;
            }
        }
    }
    if (frame.bounds.isEmpty())
    {
        frame.canvasSize = document.size;
    }
    if (frame.canvasSize != document.size)
    {
        return exactRegion();
    }
    result.valid = true;
    if (stats)
    {
        ++stats->regionalRenders;
    }
    const QRect visibleBounds = frame.bounds.intersected(outputBounds);
    if (visibleBounds.isEmpty())
    {
        return result;
    }
    result.image = frame.pixels.copy(
        QRect(visibleBounds.topLeft() - frame.bounds.topLeft(),
            visibleBounds.size()));
    result.bounds = visibleBounds;
    noteCoverageImage(stats, result.image);
    if (result.image.isNull())
    {
        return {};
    }
    return result;
}

}

// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "document/SelectionVisibility.hpp"

#include "render/RenderEngine.hpp"

#include <algorithm>
#include <cmath>

namespace ugurugu
{
namespace
{

QRect selectedBounds(const QImage &mask)
{
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

bool layerVariesByFrame(const Document &document, const Layer &layer)
{
    if (document.animationFrames <= 1)
    {
        return false;
    }
    const qreal wobbleAmount = effectiveWobbleAmount(document, layer);
    return std::any_of(layer.strokes.cbegin(),
        layer.strokes.cend(),
        [wobbleAmount](const Stroke &stroke)
        {
            if (stroke.mode == StrokeMode::Fill
                || stroke.mode == StrokeMode::Image
                || stroke.mode == StrokeMode::CompositeBoundary
                || stroke.pixelSelectionOp || stroke.reframeOp)
            {
                return false;
            }
            const bool displaced =
                wobbleAmount > 0.0 && stroke.brush.wobbleScale > 0.0;
            const bool animatedSpray = stroke.brush.engine == BrushEngine::Spray
                                       && stroke.brush.animatedJitter;
            return displaced || animatedSpray;
        });
}

bool intersectsVisiblePixelsInRegion(const QImage &layerImage,
    const QImage &selectionMask,
    const QRect &imageBounds)
{
    if (layerImage.size() != imageBounds.size())
    {
        return false;
    }
    for (int y = 0; y < layerImage.height(); ++y)
    {
        const auto *pixels =
            reinterpret_cast<const QRgb *>(layerImage.constScanLine(y));
        const uchar *selection =
            selectionMask.constScanLine(imageBounds.top() + y);
        for (int x = 0; x < layerImage.width(); ++x)
        {
            if (selection[imageBounds.left() + x] >= 128
                && qAlpha(pixels[x]) != 0)
            {
                return true;
            }
        }
    }
    return false;
}

QRect boundedMappedRect(const QRect &bounds,
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

QRect pixelSelectionPreimage(
    const QRect &required, const PixelSelectionOp &operation)
{
    const QRect canvasBounds(QPoint(), operation.canvasSize);
    QRect preimage = required.intersected(canvasBounds);
    if (!operation.drawDestination || required.isEmpty())
    {
        return preimage;
    }
    bool invertible = false;
    const QTransform inverse = operation.transform.inverted(&invertible);
    if (!invertible)
    {
        return canvasBounds;
    }
    const QRect expanded =
        required.adjusted(-4, -4, 4, 4).intersected(canvasBounds);
    const QRect movedSource =
        boundedMappedRect(expanded, inverse, operation.canvasSize, 4.0)
            .intersected(operation.sourceBounds);
    return preimage.united(movedSource).intersected(canvasBounds);
}

QRect reframePreimage(const QRect &required, const ReframeOp &operation)
{
    const QRect sourceBounds(QPoint(), operation.sourceSize);
    const QRect targetBounds(QPoint(), operation.targetSize);
    const QRect clipped = required.intersected(targetBounds);
    if (clipped.isEmpty())
    {
        return {};
    }
    if (operation.mode == ReframeMode::Canvas)
    {
        return clipped.translated(-operation.contentOffset)
            .intersected(sourceBounds);
    }
    QTransform inverseScale;
    inverseScale.scale(
        qreal(operation.sourceSize.width()) / operation.targetSize.width(),
        qreal(operation.sourceSize.height()) / operation.targetSize.height());
    return boundedMappedRect(
        clipped.adjusted(-4, -4, 4, 4).intersected(targetBounds),
        inverseScale,
        operation.sourceSize,
        4.0);
}

}

SelectionVisibility::Result SelectionVisibility::evaluate(
    const Document &document,
    const Layer &layer,
    const QImage &selectionMask,
    int preferredFrame,
    const std::atomic_bool *cancelled)
{
    Result result;
    if (!document.size.isValid() || selectionMask.isNull()
        || selectionMask.size() != document.size
        || selectionMask.format() != QImage::Format_Grayscale8)
    {
        return result;
    }
    const QRect bounds = selectedBounds(selectionMask);
    if (bounds.isEmpty())
    {
        result.renderSucceeded = true;
        return result;
    }

    const int frameCount = std::max(1, document.animationFrames);
    const int normalizedPreferred =
        ((preferredFrame % frameCount) + frameCount) % frameCount;
    const bool animated = layerVariesByFrame(document, layer);
    const int framesToInspect = animated ? frameCount : 1;
    const bool regional = std::none_of(layer.strokes.cbegin(),
        layer.strokes.cend(),
        [](const Stroke &stroke)
        {
            return stroke.mode == StrokeMode::Fill
                   || stroke.mode == StrokeMode::Image
                   || stroke.mode == StrokeMode::CompositeBoundary
                   || stroke.pixelSelectionOp || stroke.reframeOp;
        });
    for (int offset = 0; offset < framesToInspect; ++offset)
    {
        if (cancelled && cancelled->load(std::memory_order_relaxed))
        {
            return result;
        }
        const int frame = (normalizedPreferred + offset) % frameCount;
        const QSize renderSize = regional ? bounds.size() : document.size;
        QImage layerImage(renderSize, QImage::Format_ARGB32_Premultiplied);
        if (layerImage.isNull())
        {
            return result;
        }
        layerImage.fill(Qt::transparent);
        ++result.renderedFrames;
        result.renderedPixels += static_cast<quint64>(renderSize.width())
                                 * static_cast<quint64>(renderSize.height());
        result.maximumExplicitImageBytes =
            std::max(result.maximumExplicitImageBytes,
                static_cast<quint64>(layerImage.sizeInBytes()));
        const bool rendered =
            regional ? RenderEngine::renderStrokesOnLayerRegion(layerImage,
                           document,
                           layer.strokes,
                           frame,
                           document.size,
                           bounds)
                     : RenderEngine::renderStrokesOnLayer(layerImage,
                           document,
                           layer.strokes,
                           frame,
                           document.size);
        if (!rendered)
        {
            return result;
        }
        const QRect imageBounds =
            regional ? bounds : QRect(QPoint(), document.size);
        if (intersectsVisiblePixelsInRegion(
                layerImage, selectionMask, imageBounds))
        {
            result.hasVisiblePixels = true;
            result.renderSucceeded = true;
            return result;
        }
    }
    result.renderSucceeded = true;
    return result;
}

QVector<QUuid> SelectionVisibility::editableStrokeIds(const Document &document,
    const Layer &layer,
    const QImage &selectionMask,
    int frameIndex,
    EditableStrokeStats *stats)
{
    QVector<QUuid> ids;
    if (stats)
    {
        *stats = {};
    }
    if (!document.size.isValid() || layer.kind != LayerKind::Paint
        || selectionMask.isNull() || selectionMask.size() != document.size
        || selectionMask.format() != QImage::Format_Grayscale8)
    {
        return ids;
    }
    const QRect bounds = selectedBounds(selectionMask);
    if (bounds.isEmpty())
    {
        return ids;
    }

    const RenderEngine::StrokeCoveragePlan coveragePlan =
        RenderEngine::prepareStrokeCoverage(document, layer);
    const bool canRejectByBounds = coveragePlan.valid;

    QVector<quint8> candidates(layer.strokes.size(), 0);
    if (canRejectByBounds)
    {
        QRect required = bounds;
        for (int index = static_cast<int>(layer.strokes.size()) - 1; index >= 0;
            --index)
        {
            const Stroke &stroke = layer.strokes[index];
            if (stroke.mode == StrokeMode::PixelSelection
                && stroke.pixelSelectionOp)
            {
                required =
                    pixelSelectionPreimage(required, *stroke.pixelSelectionOp);
            }
            else if (stroke.mode == StrokeMode::Reframe && stroke.reframeOp)
            {
                required = reframePreimage(required, *stroke.reframeOp);
            }
            else if ((stroke.mode == StrokeMode::Paint
                         || stroke.mode == StrokeMode::Erase
                         || stroke.mode == StrokeMode::Fill))
            {
                if (coveragePlan.primitiveBounds[index].intersects(required))
                {
                    candidates[index] = 1;
                }
            }
        }
    }
    else
    {
        std::fill(candidates.begin(), candidates.end(), 1);
    }

    for (int index = 0; index < layer.strokes.size(); ++index)
    {
        const Stroke &stroke = layer.strokes[index];
        if ((stroke.mode != StrokeMode::Paint
                && stroke.mode != StrokeMode::Erase
                && stroke.mode != StrokeMode::Fill)
            || !candidates[index])
        {
            continue;
        }
        QRect coverageBounds = bounds;
        if (canRejectByBounds)
        {
            coverageBounds = coverageBounds.intersected(
                RenderEngine::conservativeStrokeCoverageBounds(
                    document, layer, index, coveragePlan));
        }
        if (coverageBounds.isEmpty())
        {
            continue;
        }
        RenderEngine::StrokeCoverageStats coverageStats;
        const RenderEngine::StrokeCoverageRegion coverage =
            RenderEngine::renderSparseStrokeCoverage(document,
                layer,
                index,
                frameIndex,
                coverageBounds,
                coveragePlan,
                &coverageStats);
        if (stats)
        {
            stats->fullCanvasFallbacks += coverageStats.fullCanvasFallbacks;
            stats->regionalRenders += coverageStats.regionalRenders;
            stats->pixelSelectionOperationsReplayed +=
                coverageStats.pixelSelectionOperationsReplayed;
            stats->reframeOperationsReplayed +=
                coverageStats.reframeOperationsReplayed;
            stats->eraseOperationsReplayed +=
                coverageStats.eraseOperationsReplayed;
            stats->effectCandidatesExamined +=
                coverageStats.effectCandidatesExamined;
            stats->maximumExplicitImageBytes =
                std::max(stats->maximumExplicitImageBytes,
                    coverageStats.maximumExplicitImageBytes);
        }
        if (coverage.valid && !coverage.image.isNull()
            && intersectsVisiblePixelsInRegion(
                coverage.image, selectionMask, coverage.bounds))
        {
            ids.append(stroke.id);
        }
    }
    return ids;
}

}

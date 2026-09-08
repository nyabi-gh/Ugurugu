// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "render/engine/DisplayScaleReplay.hpp"

#include "document/DocumentOperations.hpp"
#include "document/SelectionOperation.hpp"
#include "render/ImageAffineTransformer.hpp"
#include "render/ImageResampler.hpp"
#include "render/RasterAssetCache.hpp"
#include "render/engine/LayerOperationReplay.hpp"

#include <QHash>
#include <QPainter>
#include <QPainterPath>

#include <algorithm>
#include <cmath>

namespace ugurugu
{
namespace render_detail
{

bool buildDisplaySelectionMask(DisplaySelectionMask &result,
    const PixelSelectionOp &operation,
    const PreviewScaleMapping &mapping,
    const QSize &nativeCanvasSize,
    const QSize &displayCanvasSize,
    RenderEngine::ScaledRenderStats *stats)
{
    if (!isValidPixelSelectionOp(operation)
        || operation.canvasSize != nativeCanvasSize
        || mapping.displaySize(nativeCanvasSize) != displayCanvasSize)
    {
        return false;
    }
    result.bounds = mapping.displayBounds(
        operation.sourceBounds, QRect(QPoint(), displayCanvasSize));
    if (result.bounds.isEmpty())
    {
        return true;
    }
    result.mask = QImage(result.bounds.size(), QImage::Format_Grayscale8);
    if (result.mask.isNull())
    {
        return false;
    }
    result.mask.fill(0);
    notePreviewImage(stats, result.mask);
    for (int y = result.bounds.top(); y <= result.bounds.bottom(); ++y)
    {
        uchar *line = result.mask.scanLine(y - result.bounds.top());
        for (int x = result.bounds.left(); x <= result.bounds.right(); ++x)
        {
            const QPoint native =
                mapping.nativeSampleForDisplayPixel(QPoint(x, y));
            if (stats)
            {
                ++stats->packedSelectionSamples;
            }
            if (pixelSelectionContains(operation, native.x(), native.y()))
            {
                line[x - result.bounds.left()] = 255;
            }
        }
    }
    return true;
}

bool applyPixelSelectionOperationAtDisplayScale(QImage &layerImage,
    const PixelSelectionOp &operation,
    const PreviewScaleMapping &mapping,
    const QSize &nativeCanvasSize,
    RenderEngine::ScaledRenderStats *stats)
{
    if (layerImage.format() != QImage::Format_ARGB32_Premultiplied
        || (!operation.drawDestination && !operation.clearSource))
    {
        return false;
    }
    if (stats)
    {
        ++stats->pixelSelectionOperationsReplayed;
    }
    DisplaySelectionMask selection;
    if (!buildDisplaySelectionMask(selection,
            operation,
            mapping,
            nativeCanvasSize,
            layerImage.size(),
            stats))
    {
        return false;
    }
    if (selection.mask.isNull())
    {
        return true;
    }

    QImage payload;
    if (operation.drawDestination)
    {
        payload = QImage(
            selection.bounds.size(), QImage::Format_ARGB32_Premultiplied);
        if (payload.isNull())
        {
            return false;
        }
        payload.fill(Qt::transparent);
        notePreviewImage(stats, payload);
        notePreviewWorkingSet(stats, layerImage, selection.mask, payload);
    }
    else
    {
        notePreviewWorkingSet(stats, layerImage, selection.mask);
    }

    bool hasPixels = false;
    for (int y = selection.bounds.top(); y <= selection.bounds.bottom(); ++y)
    {
        QRgb *layerLine = reinterpret_cast<QRgb *>(layerImage.scanLine(y));
        const uchar *maskLine =
            selection.mask.constScanLine(y - selection.bounds.top());
        QRgb *payloadLine = payload.isNull()
                                ? nullptr
                                : reinterpret_cast<QRgb *>(payload.scanLine(
                                      y - selection.bounds.top()));
        for (int x = selection.bounds.left(); x <= selection.bounds.right();
            ++x)
        {
            const int localX = x - selection.bounds.left();
            if (maskLine[localX] < 128)
            {
                continue;
            }
            const QRgb pixel = layerLine[x];
            if (payloadLine)
            {
                payloadLine[localX] = pixel;
                hasPixels = hasPixels || qAlpha(pixel) != 0;
            }
            if (operation.clearSource)
            {
                layerLine[x] = 0;
            }
        }
    }
    if (!operation.drawDestination || !hasPixels)
    {
        return true;
    }

    QPainter painter(&layerImage);
    painter.setRenderHint(QPainter::Antialiasing, false);
    painter.setRenderHint(QPainter::SmoothPixmapTransform,
        operation.sampling == SamplingMode::Smooth);
    painter.setCompositionMode(QPainter::CompositionMode_SourceOver);
    painter.setTransform(mapping.displayTransform(operation.transform));
    painter.drawImage(selection.bounds.topLeft(), payload);
    painter.end();
    return true;
}

bool applyReframeOperationAtDisplayScale(QImage &layerImage,
    const ReframeOp &operation,
    const PreviewScaleMapping &mapping,
    const QSize &nativeCanvasSize,
    RenderEngine::ScaledRenderStats *stats)
{
    if (!isValidReframeOp(operation) || operation.sourceSize != nativeCanvasSize
        || layerImage.size() != mapping.displaySize(nativeCanvasSize)
        || layerImage.format() != QImage::Format_ARGB32_Premultiplied)
    {
        return false;
    }
    const QSize targetDisplaySize = mapping.displaySize(operation.targetSize);
    QImage target(targetDisplaySize, QImage::Format_ARGB32_Premultiplied);
    if (target.isNull())
    {
        return false;
    }
    target.fill(Qt::transparent);
    notePreviewImage(stats, target);
    notePreviewWorkingSet(stats, layerImage, target);

    QPainter painter(&target);
    painter.setRenderHint(QPainter::Antialiasing, false);
    painter.setRenderHint(QPainter::SmoothPixmapTransform,
        operation.sampling == SamplingMode::Smooth);
    painter.setCompositionMode(QPainter::CompositionMode_Source);
    if (operation.mode == ReframeMode::Canvas)
    {
        painter.drawImage(
            mapping.displayPoint(operation.contentOffset), layerImage);
    }
    else
    {
        painter.drawImage(QRectF(QPointF(), QSizeF(targetDisplaySize)),
            layerImage,
            QRectF(QPointF(), QSizeF(layerImage.size())));
    }
    painter.end();
    layerImage = std::move(target);
    return true;
}

bool applyImageOperationAtDisplayScale(QImage &layerImage,
    const Document &document,
    const ImageOp &operation,
    const PreviewScaleMapping &mapping,
    RenderEngine::ScaledRenderStats *stats)
{
    const QTransform displayTransform(
        operation.transform.m11() * mapping.horizontalScale,
        operation.transform.m12() * mapping.verticalScale,
        0.0,
        operation.transform.m21() * mapping.horizontalScale,
        operation.transform.m22() * mapping.verticalScale,
        0.0,
        operation.transform.dx() * mapping.horizontalScale,
        operation.transform.dy() * mapping.verticalScale,
        1.0);
    const QImage transformed = RasterAssetCache::transformedImage(document,
        operation.assetId,
        layerImage.size(),
        displayTransform,
        operation.sampling);
    if (transformed.isNull())
    {
        return false;
    }
    notePreviewImage(stats, transformed);
    notePreviewWorkingSet(stats, layerImage, transformed);
    QPainter painter(&layerImage);
    painter.setCompositionMode(QPainter::CompositionMode_SourceOver);
    painter.drawImage(QPoint(), transformed);
    painter.end();
    return true;
}

bool renderLayerOperationsAtDisplayScale(QImage &layerImage,
    const Document &document,
    const QVector<Stroke> &operations,
    int normalizedFrame,
    int frameCount,
    const QSize &initialCanvasSize,
    const PreviewScaleMapping &mapping,
    RenderEngine::ScaledRenderStats *stats,
    const std::atomic_bool *cancellation)
{
    if (!initialCanvasSize.isValid())
    {
        return false;
    }
    layerImage = QImage(mapping.displaySize(initialCanvasSize),
        QImage::Format_ARGB32_Premultiplied);
    if (layerImage.isNull())
    {
        return false;
    }
    layerImage.fill(Qt::transparent);
    notePreviewImage(stats, layerImage);
    notePreviewWorkingSet(stats, layerImage);

    QSize nativeCanvasSize = initialCanvasSize;
    QHash<qint64, QPainterPath> clipPaths;
    QHash<qint64, QImage> scaledClipMasks;
    QVector<QImage> completedSections;
    QVector<Stroke> primitiveRun;
    // A flush is the operation boundary cancellation observes: the pending
    // primitive run is skipped whole rather than checked per stroke.
    const auto flush = [&]() -> bool
    {
        if (isRenderCancelled(cancellation))
        {
            return false;
        }
        if (primitiveRun.isEmpty())
        {
            return true;
        }
        if (stats)
        {
            stats->primitiveStrokesRendered +=
                static_cast<quint64>(primitiveRun.size());
        }
        renderLayerStrokes(layerImage,
            document,
            primitiveRun,
            normalizedFrame,
            frameCount,
            mapping.horizontalScale,
            mapping.verticalScale,
            clipPaths,
            scaledClipMasks);
        primitiveRun.clear();
        notePreviewImage(stats, layerImage);
        return true;
    };

    // Sections sealed by composite boundaries render on their own transparent
    // surfaces; strokes after the last boundary draw directly on the
    // flattened composite. Mirrors renderLayerOperations.
    int lastBoundaryIndex = -1;
    for (int index = 0; index < operations.size(); ++index)
    {
        if (operations[index].mode == StrokeMode::CompositeBoundary)
        {
            lastBoundaryIndex = index;
        }
    }

    for (int index = 0; index < operations.size(); ++index)
    {
        const Stroke &operation = operations[index];
        if (operation.mode == StrokeMode::PixelSelection)
        {
            if (!flush())
            {
                return false;
            }
            if (!operation.pixelSelectionOp || operation.reframeOp
                || !applyPixelSelectionOperationAtDisplayScale(layerImage,
                    *operation.pixelSelectionOp,
                    mapping,
                    nativeCanvasSize,
                    stats))
            {
                return false;
            }
        }
        else if (operation.mode == StrokeMode::Reframe)
        {
            if (!flush())
            {
                return false;
            }
            if (!operation.reframeOp || operation.pixelSelectionOp
                || !applyReframeOperationAtDisplayScale(layerImage,
                    *operation.reframeOp,
                    mapping,
                    nativeCanvasSize,
                    stats))
            {
                return false;
            }
            // A reframe spans the whole layer, not just the section it was
            // recorded in, so completed sections must follow the canvas too.
            for (QImage &section : completedSections)
            {
                if (!applyReframeOperationAtDisplayScale(section,
                        *operation.reframeOp,
                        mapping,
                        nativeCanvasSize,
                        stats))
                {
                    return false;
                }
            }
            nativeCanvasSize = operation.reframeOp->targetSize;
            // Cached masks and paths belong to the previous framebuffer
            // epoch and therefore to a different display surface.
            clipPaths.clear();
            scaledClipMasks.clear();
        }
        else if (operation.mode == StrokeMode::CompositeBoundary)
        {
            if (!flush())
            {
                return false;
            }
            if (operation.pixelSelectionOp || operation.reframeOp
                || operation.imageOp || !operation.points.isEmpty())
            {
                return false;
            }
            completedSections.append(std::move(layerImage));
            if (index == lastBoundaryIndex)
            {
                QImage composed = std::move(completedSections.first());
                QPainter painter(&composed);
                painter.setRenderHint(QPainter::Antialiasing, false);
                painter.setCompositionMode(
                    QPainter::CompositionMode_SourceOver);
                for (int section = 1; section < completedSections.size();
                    ++section)
                {
                    painter.drawImage(QPoint(), completedSections.at(section));
                }
                painter.end();
                layerImage = std::move(composed);
                completedSections.clear();
            }
            else
            {
                QImage next(completedSections.last().size(),
                    QImage::Format_ARGB32_Premultiplied);
                if (next.isNull())
                {
                    return false;
                }
                next.fill(Qt::transparent);
                notePreviewImage(stats, next);
                layerImage = std::move(next);
            }
            if (stats)
            {
                quint64 bytes = static_cast<quint64>(layerImage.sizeInBytes());
                for (const QImage &section : completedSections)
                {
                    bytes += static_cast<quint64>(section.sizeInBytes());
                }
                stats->maximumEstimatedWorkingSetBytes =
                    std::max(stats->maximumEstimatedWorkingSetBytes, bytes);
            }
        }
        else if (operation.mode == StrokeMode::Image)
        {
            if (!flush())
            {
                return false;
            }
            if (!operation.imageOp || operation.pixelSelectionOp
                || operation.reframeOp
                || !applyImageOperationAtDisplayScale(
                    layerImage, document, *operation.imageOp, mapping, stats))
            {
                return false;
            }
        }
        else
        {
            if (operation.pixelSelectionOp || operation.reframeOp
                || operation.imageOp)
            {
                return false;
            }
            primitiveRun.append(operation);
        }
    }
    if (!flush())
    {
        return false;
    }
    return nativeCanvasSize == document.size
           && layerImage.size() == mapping.outputSize;
}

}

}

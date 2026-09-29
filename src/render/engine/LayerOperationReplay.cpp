// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "render/engine/LayerOperationReplay.hpp"

#include "document/DocumentOperations.hpp"
#include "document/SelectionOperation.hpp"
#include "document/StrokeMask.hpp"
#include "render/ImageAffineTransformer.hpp"
#include "render/ImageResampler.hpp"
#include "render/RasterAssetCache.hpp"
#include "render/RenderEngine.hpp"
#include "render/StrokeRenderer.hpp"

#include <QPainter>
#include <QRadialGradient>
#include <QSet>
#include <QtMath>

#include <algorithm>
#include <cmath>
#include <numbers>

namespace ugurugu
{
namespace render_detail
{

QPainterPath maskPath(const QImage &mask)
{
    QPainterPath path;
    if (mask.isNull() || mask.format() != QImage::Format_Grayscale8)
    {
        return path;
    }
    for (int y = 0; y < mask.height(); ++y)
    {
        const uchar *line = mask.constScanLine(y);
        int x = 0;
        while (x < mask.width())
        {
            while (x < mask.width() && line[x] < 128)
            {
                ++x;
            }
            const int left = x;
            while (x < mask.width() && line[x] >= 128)
            {
                ++x;
            }
            if (left < x)
            {
                path.addRect(left, y, x - left, 1);
            }
        }
    }
    return path;
}

QImage scaledMask(
    const QImage &mask, const QSize &outputSize, QHash<qint64, QImage> &cache)
{
    if (mask.isNull())
    {
        return {};
    }
    const qint64 key = mask.cacheKey();
    const auto cached = cache.constFind(key);
    if (cached != cache.cend())
    {
        return cached.value();
    }
    QImage scaled = mask.size() == outputSize ? mask
                                              : mask.scaled(outputSize,
                                                    Qt::IgnoreAspectRatio,
                                                    Qt::FastTransformation);
    if (!scaled.isNull())
    {
        cache.insert(key, scaled);
    }
    return scaled;
}

QImage scaledTransientMask(const QImage &mask, const QSize &outputSize)
{
    if (mask.isNull())
    {
        return {};
    }
    return mask.size() == outputSize
               ? mask
               : mask.scaled(
                     outputSize, Qt::IgnoreAspectRatio, Qt::FastTransformation);
}

void noteStrokeMaskBackings(LayerStrokeReplayStats *stats,
    const QHash<qint64, QImage> &cachedMasks,
    const QImage &firstTransient,
    const QImage &secondTransient)
{
    if (!stats)
    {
        return;
    }
    QSet<qint64> backingKeys;
    for (const QImage &mask : cachedMasks)
    {
        if (!mask.isNull())
        {
            backingKeys.insert(mask.cacheKey());
        }
    }
    if (!firstTransient.isNull())
    {
        backingKeys.insert(firstTransient.cacheKey());
    }
    if (!secondTransient.isNull())
    {
        backingKeys.insert(secondTransient.cacheKey());
    }
    stats->peakStrokeMaskBackingCount =
        std::max(stats->peakStrokeMaskBackingCount,
            static_cast<int>(backingKeys.size()));
}

std::optional<QRect> scaledVisibilityClip(const Stroke &stroke,
    const QSize &outputSize,
    qreal horizontalScale,
    qreal verticalScale)
{
    if (!stroke.visibilityClip)
    {
        return std::nullopt;
    }
    const QRectF source(*stroke.visibilityClip);
    QRect scaled = QRectF(source.x() * horizontalScale,
        source.y() * verticalScale,
        source.width() * horizontalScale,
        source.height() * verticalScale)
                       .toAlignedRect()
                       .intersected(QRect(QPoint(), outputSize));
    return scaled;
}

void applyFillStroke(QImage &layerImage,
    const Stroke &stroke,
    const QImage &coverageMask,
    const QImage &clipMask,
    const std::optional<QRect> &visibilityClip,
    qreal horizontalScale,
    qreal verticalScale)
{
    QImage proceduralMask;
    if (coverageMask.isNull())
    {
        const QPointF seedPosition = stroke.points.first().position;
        const QPoint seed(
            std::clamp(static_cast<int>(seedPosition.x() * horizontalScale),
                0,
                layerImage.width() - 1),
            std::clamp(static_cast<int>(seedPosition.y() * verticalScale),
                0,
                layerImage.height() - 1));
        proceduralMask = RenderEngine::fillRegionMask(layerImage, seed);
    }
    const QImage &mask = coverageMask.isNull() ? proceduralMask : coverageMask;
    if (mask.isNull())
    {
        return;
    }
    const QRgb fill = qPremultiply(stroke.color.rgba());
    const int fillRed = qRed(fill);
    const int fillGreen = qGreen(fill);
    const int fillBlue = qBlue(fill);
    const int fillAlpha = qAlpha(fill);
    const int width = layerImage.width();
    const int height = layerImage.height();

    for (int y = 0; y < height; ++y)
    {
        QRgb *line = reinterpret_cast<QRgb *>(layerImage.scanLine(y));
        const uchar *maskLine = mask.constScanLine(y);
        const uchar *maskAbove = y > 0 ? mask.constScanLine(y - 1) : nullptr;
        const uchar *maskBelow =
            y < height - 1 ? mask.constScanLine(y + 1) : nullptr;
        const uchar *clipLine =
            clipMask.isNull() ? nullptr : clipMask.constScanLine(y);
        for (int x = 0; x < width; ++x)
        {
            if ((visibilityClip && !visibilityClip->contains(x, y))
                || (clipLine && clipLine[x] < 128))
            {
                continue;
            }
            if (maskLine[x] >= 128)
            {
                line[x] = fill;
                continue;
            }
            if (stroke.fillCoverage && !stroke.brush.antialiasing)
            {
                continue;
            }
            const bool touchesRegion =
                (x > 0 && maskLine[x - 1] >= 128)
                || (x < width - 1 && maskLine[x + 1] >= 128)
                || (maskAbove && maskAbove[x] >= 128)
                || (maskBelow && maskBelow[x] >= 128);
            if (!touchesRegion)
            {
                continue;
            }
            const QRgb existing = line[x];
            const int inverse = 255 - qAlpha(existing);
            line[x] = qRgba(qRed(existing) + fillRed * inverse / 255,
                qGreen(existing) + fillGreen * inverse / 255,
                qBlue(existing) + fillBlue * inverse / 255,
                qAlpha(existing) + fillAlpha * inverse / 255);
        }
    }
}

void renderLayerStrokes(QImage &layerImage,
    const Document &document,
    const QVector<Stroke> &strokes,
    int normalizedFrame,
    int frameCount,
    qreal horizontalScale,
    qreal verticalScale,
    QHash<qint64, QPainterPath> &clipPaths,
    QHash<qint64, QImage> &scaledClipMasks,
    const QPointF &logicalOrigin,
    LayerStrokeReplayStats *stats)
{
    static_cast<void>(frameCount);
    const QSize outputSize = layerImage.size();
    QPainter painter(&layerImage);
    painter.setRenderHint(QPainter::Antialiasing, false);
    painter.scale(horizontalScale, verticalScale);
    painter.translate(-logicalOrigin);

    for (const Stroke &stroke : strokes)
    {
        if (stroke.points.isEmpty() || !isValidBrushSettings(stroke.brush))
        {
            continue;
        }

        if (stroke.mode == StrokeMode::Fill)
        {
            const QImage scaledClipMask =
                scaledMask(stroke.clipMask, outputSize, scaledClipMasks);
            if (!stroke.clipMask.isNull() && scaledClipMask.isNull())
            {
                continue;
            }
            const QImage packedCoverage =
                stroke.fillCoverage ? unpackBinaryMask(*stroke.fillCoverage)
                                    : QImage();
            const QImage &coverage =
                stroke.fillCoverage ? packedCoverage : stroke.fillMask;
            const QImage scaledCoverageMask =
                stroke.fillCoverage
                    ? scaledTransientMask(coverage, outputSize)
                    : scaledMask(coverage, outputSize, scaledClipMasks);
            noteStrokeMaskBackings(
                stats, scaledClipMasks, packedCoverage, scaledCoverageMask);
            if ((!stroke.fillMask.isNull() || stroke.fillCoverage)
                && scaledCoverageMask.isNull())
            {
                continue;
            }
            const std::optional<QRect> visibility = scaledVisibilityClip(
                stroke, outputSize, horizontalScale, verticalScale);
            if (visibility && visibility->isEmpty())
            {
                continue;
            }
            painter.end();
            applyFillStroke(layerImage,
                stroke,
                scaledCoverageMask,
                scaledClipMask,
                visibility,
                horizontalScale,
                verticalScale);
            painter.begin(&layerImage);
            painter.setRenderHint(QPainter::Antialiasing, false);
            painter.scale(horizontalScale, verticalScale);
            painter.translate(-logicalOrigin);
            continue;
        }

        painter.save();
        painter.setRenderHint(
            QPainter::Antialiasing, stroke.brush.antialiasing);
        if (stroke.visibilityClip)
        {
            painter.setClipRect(
                QRectF(*stroke.visibilityClip), Qt::IntersectClip);
        }
        if (!stroke.clipMask.isNull())
        {
            const qint64 key = stroke.clipMask.cacheKey();
            auto cached = clipPaths.constFind(key);
            if (cached == clipPaths.cend())
            {
                cached = clipPaths.insert(key, maskPath(stroke.clipMask));
            }
            painter.setClipPath(cached.value(), Qt::IntersectClip);
        }
        painter.setCompositionMode(
            stroke.mode == StrokeMode::Erase
                ? QPainter::CompositionMode_DestinationOut
                : QPainter::CompositionMode_SourceOver);

        painter.setBrush(Qt::NoBrush);
        const StrokeRenderer::PreparedStroke prepared =
            StrokeRenderer::prepare(stroke, normalizedFrame, document);
        if (!prepared.valid)
        {
            painter.restore();
            continue;
        }
        StrokeRenderer::paint(painter, stroke, prepared);
        painter.restore();
    }

    painter.end();
}

bool applyPixelSelectionOperation(
    QImage &layerImage, const PixelSelectionOp &operation)
{
    if (!isValidPixelSelectionOp(operation)
        || layerImage.size() != operation.canvasSize
        || layerImage.format() != QImage::Format_ARGB32_Premultiplied)
    {
        return false;
    }
    if (!operation.drawDestination)
    {
        if (!operation.clearSource)
        {
            return false;
        }
        for (int y = operation.sourceBounds.top();
            y <= operation.sourceBounds.bottom();
            ++y)
        {
            QRgb *layerLine = reinterpret_cast<QRgb *>(layerImage.scanLine(y));
            for (int x = operation.sourceBounds.left();
                x <= operation.sourceBounds.right();
                ++x)
            {
                if (pixelSelectionContains(operation, x, y))
                {
                    layerLine[x] = 0;
                }
            }
        }
        return true;
    }
    QImage payload(
        operation.sourceBounds.size(), QImage::Format_ARGB32_Premultiplied);
    if (payload.isNull())
    {
        return false;
    }
    payload.fill(Qt::transparent);
    bool hasPixels = false;
    for (int y = operation.sourceBounds.top();
        y <= operation.sourceBounds.bottom();
        ++y)
    {
        QRgb *layerLine = reinterpret_cast<QRgb *>(layerImage.scanLine(y));
        QRgb *payloadLine = reinterpret_cast<QRgb *>(
            payload.scanLine(y - operation.sourceBounds.top()));
        for (int x = operation.sourceBounds.left();
            x <= operation.sourceBounds.right();
            ++x)
        {
            if (!pixelSelectionContains(operation, x, y))
            {
                continue;
            }
            const QRgb pixel = layerLine[x];
            payloadLine[x - operation.sourceBounds.left()] = pixel;
            hasPixels = hasPixels || qAlpha(pixel) != 0;
            if (operation.clearSource)
            {
                layerLine[x] = 0;
            }
        }
    }
    if (!hasPixels)
    {
        return true;
    }
    return ImageAffineTransformer::compositeSourceOver(layerImage,
        QRect(QPoint(), layerImage.size()),
        payload,
        operation.sourceBounds,
        operation.transform,
        operation.sampling);
}

bool applyReframeOperation(QImage &layerImage, const ReframeOp &operation)
{
    if (!isValidReframeOp(operation)
        || layerImage.size() != operation.sourceSize
        || layerImage.format() != QImage::Format_ARGB32_Premultiplied)
    {
        return false;
    }
    if (operation.mode == ReframeMode::Image)
    {
        QImage target = ImageResampler::resample(
            layerImage, operation.targetSize, operation.sampling);
        if (target.isNull())
        {
            return false;
        }
        layerImage = std::move(target);
        return true;
    }
    QImage target(operation.targetSize, QImage::Format_ARGB32_Premultiplied);
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
    painter.drawImage(operation.contentOffset, layerImage);
    painter.end();
    layerImage = std::move(target);
    return true;
}

bool applyImageOperation(
    QImage &layerImage, const Document &document, const ImageOp &operation)
{
    if (!isValidImageOp(operation)
        || layerImage.format() != QImage::Format_ARGB32_Premultiplied)
    {
        return false;
    }
    const QImage transformed = RasterAssetCache::transformedImage(document,
        operation.assetId,
        layerImage.size(),
        operation.transform,
        operation.sampling);
    if (transformed.isNull())
    {
        return false;
    }
    QPainter painter(&layerImage);
    painter.setCompositionMode(QPainter::CompositionMode_SourceOver);
    painter.drawImage(QPoint(), transformed);
    painter.end();
    return true;
}

bool renderLayerOperations(QImage &layerImage,
    const Document &document,
    const QVector<Stroke> &operations,
    int normalizedFrame,
    int frameCount,
    const QSize &initialCanvasSize,
    const std::atomic_bool *cancellation)
{
    if (!initialCanvasSize.isValid())
    {
        return false;
    }
    layerImage = QImage(initialCanvasSize, QImage::Format_ARGB32_Premultiplied);
    if (layerImage.isNull())
    {
        return false;
    }
    layerImage.fill(Qt::transparent);

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
        renderLayerStrokes(layerImage,
            document,
            primitiveRun,
            normalizedFrame,
            frameCount,
            1.0,
            1.0,
            clipPaths,
            scaledClipMasks);
        primitiveRun.clear();
        return true;
    };

    // Sections sealed by composite boundaries render on their own transparent
    // surfaces so an eraser recorded before the last boundary cannot reach the
    // artwork it was merged with. Strokes after the last boundary draw
    // directly on the flattened composite: an eraser added after the merge
    // erases everything already on the layer.
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
                || !applyPixelSelectionOperation(
                    layerImage, *operation.pixelSelectionOp))
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
                || !applyReframeOperation(layerImage, *operation.reframeOp))
            {
                return false;
            }
            // A reframe spans the whole layer, not just the section it was
            // recorded in, so completed sections must follow the canvas too.
            for (QImage &section : completedSections)
            {
                if (!applyReframeOperation(section, *operation.reframeOp))
                {
                    return false;
                }
            }
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
                layerImage = std::move(next);
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
                || !applyImageOperation(
                    layerImage, document, *operation.imageOp))
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
    return layerImage.size() == document.size;
}

}

}

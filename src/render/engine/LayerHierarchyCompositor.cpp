// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "render/engine/LayerHierarchyCompositor.hpp"

#include "document/DocumentOperations.hpp"
#include "render/ImageAffineTransformer.hpp"
#include "render/ImageResampler.hpp"
#include "render/engine/DisplayScaleReplay.hpp"
#include "render/engine/LayerOperationReplay.hpp"
#include "render/engine/StaticLayerCache.hpp"

#include <QHash>
#include <QPainterPath>

#include <algorithm>
#include <cmath>

namespace ugurugu
{
namespace render_detail
{

QPainter::CompositionMode compositionMode(LayerBlendMode mode)
{
    switch (mode)
    {
    case LayerBlendMode::Multiply:
        return QPainter::CompositionMode_Multiply;
    case LayerBlendMode::Screen:
        return QPainter::CompositionMode_Screen;
    case LayerBlendMode::Overlay:
        return QPainter::CompositionMode_Overlay;
    case LayerBlendMode::Normal:
        return QPainter::CompositionMode_SourceOver;
    }
    return QPainter::CompositionMode_SourceOver;
}

void prepareLayerComposition(
    QPainter &painter, LayerBlendMode mode, qreal opacity)
{
    painter.setCompositionMode(compositionMode(mode));
    painter.setOpacity(std::clamp(opacity, 0.0, 1.0));
}

PaintLayerImage renderPaintLayerImage(const Document &document,
    const Layer &layer,
    int normalizedFrame,
    const QSize &outputSize,
    const PreviewScaleMapping *mapping,
    RenderEngine::ScaledRenderStats *stats,
    const std::atomic_bool *cancellation)
{
    if (layer.strokes.isEmpty())
    {
        return PaintLayerImage::empty();
    }
    const int frameCount = std::max(1, document.animationFrames);
    const QSize initialSize = layer.initialCanvasSize.isValid()
                                  ? layer.initialCanvasSize
                                  : DocumentOperations::initialCanvasSize(
                                        layer.strokes, document.size);
    const Document layerDocument = documentForLayer(document, layer);
    const auto render = [&]() -> QImage
    {
        if (mapping)
        {
            QImage displayLayer;
            if (!renderLayerOperationsAtDisplayScale(displayLayer,
                    layerDocument,
                    layer.strokes,
                    normalizedFrame,
                    frameCount,
                    initialSize,
                    *mapping,
                    stats,
                    cancellation))
            {
                return {};
            }
            return displayLayer;
        }
        QImage native;
        if (!renderLayerOperations(native,
                layerDocument,
                layer.strokes,
                normalizedFrame,
                frameCount,
                initialSize,
                cancellation))
        {
            return {};
        }
        return native.size() == outputSize ? native
                                           : native.scaled(outputSize,
                                                 Qt::IgnoreAspectRatio,
                                                 Qt::FastTransformation);
    };
    if (!isLayerFrameInvariant(layerDocument, layer))
    {
        return render();
    }
    const StaticLayerCache::Key key{layer.id,
        outputSize,
        document.size,
        initialSize,
        mapping != nullptr,
        frameCount,
        layerDocument.wobbleAmount,
        layerDocument.motion};
    return StaticLayerCache::raster(
        layerDocument, layer, key, render, cancellation);
}

QImage renderAtDisplayScale(const Document &document,
    int frameIndex,
    const QSize &outputSize,
    RenderEngine::ScaledRenderStats *stats,
    const std::atomic_bool *cancellation)
{
    if (document.size.isEmpty() || outputSize.isEmpty())
    {
        return {};
    }
    const PreviewScaleMapping mapping{document.size,
        outputSize,
        static_cast<qreal>(outputSize.width()) / document.size.width(),
        static_cast<qreal>(outputSize.height()) / document.size.height()};

    const int frameCount = std::max(1, document.animationFrames);
    const int normalizedFrame =
        ((frameIndex % frameCount) + frameCount) % frameCount;
    const auto renderPaintLayer = [&](const Layer &layer)
    {
        return renderPaintLayerImage(document,
            layer,
            normalizedFrame,
            outputSize,
            &mapping,
            stats,
            cancellation);
    };
    return renderLayerHierarchy(document,
        outputSize,
        document.background,
        renderPaintLayer,
        stats,
        cancellation);
}

QImage renderAtSize(const Document &document,
    int frameIndex,
    const QSize &outputSize,
    const std::atomic_bool *cancellation)
{
    if (document.size.isEmpty() || outputSize.isEmpty())
    {
        return {};
    }

    const int frameCount = std::max(1, document.animationFrames);
    const int normalizedFrame =
        ((frameIndex % frameCount) + frameCount) % frameCount;

    const auto renderPaintLayer = [&](const Layer &layer)
    {
        return renderPaintLayerImage(document,
            layer,
            normalizedFrame,
            outputSize,
            nullptr,
            nullptr,
            cancellation);
    };
    return renderLayerHierarchy(document,
        outputSize,
        document.background,
        renderPaintLayer,
        nullptr,
        cancellation);
}

QImage renderRegion(const Document &document,
    int frameIndex,
    const QSize &outputSize,
    const QRect &outputRegion,
    RenderEngine::ScaledRenderStats *stats,
    const std::atomic_bool *cancellation)
{
    if (document.size.isEmpty() || outputSize.isEmpty()
        || outputRegion.isEmpty()
        || !QRect(QPoint(), outputSize).contains(outputRegion))
    {
        return {};
    }
    const bool displayScaleReplay =
        canReplayAtDisplayScale(document, outputSize);
    if (!displayScaleReplay && outputSize != document.size)
    {
        return {};
    }
    if (stats)
    {
        stats->usedDisplayScaleReplay = displayScaleReplay;
        stats->usedNativeExactFallback = !displayScaleReplay;
    }
    const PreviewScaleMapping mapping{document.size,
        outputSize,
        static_cast<qreal>(outputSize.width()) / document.size.width(),
        static_cast<qreal>(outputSize.height()) / document.size.height()};

    const int frameCount = std::max(1, document.animationFrames);
    const int normalizedFrame =
        ((frameIndex % frameCount) + frameCount) % frameCount;
    // Cropping the framebuffer alone saves only rasterization; a stroke's
    // wobble geometry costs the same wherever it lands. Strokes that cannot
    // reach the region are skipped outright, which is what makes a small
    // region cheap on a layer carrying hundreds of them.
    const QRect nativeRegion =
        QRectF(outputRegion.x() / mapping.horizontalScale,
            outputRegion.y() / mapping.verticalScale,
            outputRegion.width() / mapping.horizontalScale,
            outputRegion.height() / mapping.verticalScale)
            .toAlignedRect()
            .adjusted(-2, -2, 2, 2);

    const auto renderPaintLayer = [&](const Layer &layer) -> PaintLayerImage
    {
        if (layer.strokes.isEmpty())
        {
            return PaintLayerImage::empty();
        }
        QImage region(outputRegion.size(), QImage::Format_ARGB32_Premultiplied);
        if (region.isNull())
        {
            return QImage();
        }
        region.fill(Qt::transparent);
        const Document layerDocument = documentForLayer(document, layer);
        const QSize initialSize = layer.initialCanvasSize.isValid()
                                      ? layer.initialCanvasSize
                                      : DocumentOperations::initialCanvasSize(
                                            layer.strokes, document.size);
        bool paintsInPlace = true;
        bool movesPixels = false;
        for (const Stroke &stroke : layer.strokes)
        {
            if (stroke.mode == StrokeMode::PixelSelection)
            {
                movesPixels = true;
                paintsInPlace = false;
            }
            else if (stroke.mode != StrokeMode::Paint
                     && stroke.mode != StrokeMode::Erase)
            {
                paintsInPlace = false;
                movesPixels = false;
                break;
            }
        }
        const bool culled =
            initialSize == document.size && (paintsInPlace || movesPixels);
        QVector<Stroke> reaching;
        if (culled)
        {
            const RenderEngine::StrokeCoveragePlan plan =
                RenderEngine::prepareStrokeCoverage(document, layer);
            if (!plan.valid
                || plan.primitiveBounds.size() != layer.strokes.size())
            {
                reaching = layer.strokes;
            }
            else
            {
                // A pixel-selection operation can only carry ink it reads out
                // of its own source, and only when it draws a destination at
                // all — an operation that just clears reaches nowhere new.
                // Walking backwards is what makes a chain of them work: an
                // operation only widens the reach once a later one has shown
                // that its destination is wanted.
                QRect reach = nativeRegion;
                for (qsizetype index = layer.strokes.size() - 1; index >= 0;
                    --index)
                {
                    const Stroke &stroke = layer.strokes[index];
                    if (stroke.mode != StrokeMode::PixelSelection
                        || !stroke.pixelSelectionOp
                        || !stroke.pixelSelectionOp->drawDestination)
                    {
                        continue;
                    }
                    const PixelSelectionOp &pixel = *stroke.pixelSelectionOp;
                    const QRect destination =
                        ImageAffineTransformer::targetBounds(pixel.sourceBounds,
                            document.size,
                            pixel.transform,
                            pixel.sampling);
                    if (destination.intersects(reach))
                    {
                        reach = reach.united(pixel.sourceBounds);
                    }
                }
                reaching.reserve(layer.strokes.size());
                for (int index = 0; index < layer.strokes.size(); ++index)
                {
                    const Stroke &stroke = layer.strokes[index];
                    if (stroke.mode == StrokeMode::PixelSelection
                        || plan.primitiveBounds[index].intersects(reach))
                    {
                        reaching.append(stroke);
                    }
                }
            }
        }
        const QVector<Stroke> &strokes = culled ? reaching : layer.strokes;

        if (paintsInPlace && initialSize == document.size
            && RenderEngine::renderStrokesOnLayerRegion(region,
                layerDocument,
                strokes,
                normalizedFrame,
                outputSize,
                outputRegion))
        {
            notePreviewImage(stats, region);
            return region;
        }

        QImage whole;
        const bool rendered = displayScaleReplay
                                  ? renderLayerOperationsAtDisplayScale(whole,
                                        layerDocument,
                                        strokes,
                                        normalizedFrame,
                                        frameCount,
                                        initialSize,
                                        mapping,
                                        stats,
                                        cancellation)
                                  : renderLayerOperations(whole,
                                        layerDocument,
                                        strokes,
                                        normalizedFrame,
                                        frameCount,
                                        initialSize,
                                        cancellation);
        if (!rendered)
        {
            return QImage();
        }
        if (whole.size() != outputSize)
        {
            whole = whole.scaled(
                outputSize, Qt::IgnoreAspectRatio, Qt::FastTransformation);
        }
        if (whole.isNull())
        {
            return QImage();
        }
        return whole.copy(outputRegion);
    };
    return renderLayerHierarchy(document,
        outputRegion.size(),
        document.background,
        renderPaintLayer,
        stats,
        cancellation);
}

}

}

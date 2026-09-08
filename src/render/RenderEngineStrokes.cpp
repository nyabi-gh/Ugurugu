// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "document/DocumentOperations.hpp"
#include "document/SelectionOperation.hpp"
#include "document/StrokeMask.hpp"
#include "render/RenderEngine.hpp"
#include "render/StrokeCoverageRenderer.hpp"
#include "render/StrokeRenderer.hpp"
#include "render/engine/LayerOperationReplay.hpp"
#include "render/engine/PreviewScale.hpp"

#include <QHash>
#include <QPainter>
#include <QPainterPath>
#include <QtMath>

#include <algorithm>
#include <cmath>

namespace ugurugu
{

using namespace render_detail;

bool RenderEngine::renderStrokesOnLayer(QImage &layerImage,
    const Document &document,
    const QVector<Stroke> &strokes,
    int frameIndex,
    const QSize &outputSize)
{
    if (document.size.isEmpty() || outputSize.isEmpty())
    {
        return false;
    }
    const bool containsFramebufferOperations = std::any_of(strokes.cbegin(),
        strokes.cend(),
        [](const Stroke &stroke)
        {
            return stroke.mode == StrokeMode::PixelSelection
                   || stroke.mode == StrokeMode::Reframe
                   || stroke.mode == StrokeMode::CompositeBoundary
                   || stroke.mode == StrokeMode::Image;
        });
    if (containsFramebufferOperations)
    {
        const int frameCount = std::max(1, document.animationFrames);
        const int normalizedFrame =
            ((frameIndex % frameCount) + frameCount) % frameCount;
        QImage native;
        if (!renderLayerOperations(native,
                document,
                strokes,
                normalizedFrame,
                frameCount,
                DocumentOperations::initialCanvasSize(strokes, document.size)))
        {
            return false;
        }
        layerImage = native.size() == outputSize ? native
                                                 : native.scaled(outputSize,
                                                       Qt::IgnoreAspectRatio,
                                                       Qt::FastTransformation);
        return !layerImage.isNull();
    }
    if (layerImage.isNull())
    {
        layerImage = QImage(outputSize, QImage::Format_ARGB32_Premultiplied);
        if (layerImage.isNull())
        {
            return false;
        }
        layerImage.fill(Qt::transparent);
    }
    if (layerImage.size() != outputSize
        || layerImage.format() != QImage::Format_ARGB32_Premultiplied)
    {
        return false;
    }

    const qreal horizontalScale =
        static_cast<qreal>(outputSize.width()) / document.size.width();
    const qreal verticalScale =
        static_cast<qreal>(outputSize.height()) / document.size.height();
    const int frameCount = std::max(1, document.animationFrames);
    const int normalizedFrame =
        ((frameIndex % frameCount) + frameCount) % frameCount;
    QHash<qint64, QPainterPath> clipPaths;
    QHash<qint64, QImage> scaledClipMasks;
    renderLayerStrokes(layerImage,
        document,
        strokes,
        normalizedFrame,
        frameCount,
        horizontalScale,
        verticalScale,
        clipPaths,
        scaledClipMasks);
    return true;
}

bool RenderEngine::renderStrokesOnLayerRegion(QImage &layerImage,
    const Document &document,
    const QVector<Stroke> &strokes,
    int frameIndex,
    const QRect &outputRegion)
{
    return renderStrokesOnLayerRegion(
        layerImage, document, strokes, frameIndex, document.size, outputRegion);
}

bool RenderEngine::renderStrokesOnLayerRegion(QImage &layerImage,
    const Document &document,
    const QVector<Stroke> &strokes,
    int frameIndex,
    const QSize &outputSize,
    const QRect &outputRegion,
    StrokeRenderCache *cache)
{
    if (document.size.isEmpty() || outputSize.isEmpty()
        || outputRegion.isEmpty()
        || !QRect(QPoint(), outputSize).contains(outputRegion)
        || layerImage.isNull() || layerImage.size() != outputRegion.size()
        || layerImage.format() != QImage::Format_ARGB32_Premultiplied
        || std::any_of(strokes.cbegin(),
            strokes.cend(),
            [](const Stroke &stroke)
            {
                return stroke.mode == StrokeMode::PixelSelection
                       || stroke.mode == StrokeMode::Reframe
                       || stroke.mode == StrokeMode::CompositeBoundary
                       || stroke.mode == StrokeMode::Fill
                       || stroke.mode == StrokeMode::Image;
            }))
    {
        return false;
    }
    const int frameCount = std::max(1, document.animationFrames);
    const int normalizedFrame =
        ((frameIndex % frameCount) + frameCount) % frameCount;
    QHash<qint64, QPainterPath> localClipPaths;
    QHash<qint64, QPainterPath> &clipPaths =
        cache ? cache->clipPaths : localClipPaths;
    QHash<qint64, QImage> scaledClipMasks;
    const qreal horizontalScale =
        static_cast<qreal>(outputSize.width()) / document.size.width();
    const qreal verticalScale =
        static_cast<qreal>(outputSize.height()) / document.size.height();
    const QPointF logicalOrigin(
        outputRegion.x() / horizontalScale, outputRegion.y() / verticalScale);
    renderLayerStrokes(layerImage,
        document,
        strokes,
        normalizedFrame,
        frameCount,
        horizontalScale,
        verticalScale,
        clipPaths,
        scaledClipMasks,
        logicalOrigin);
    return true;
}

QRect RenderEngine::strokePreviewBounds(
    const Document &document, const Stroke &stroke, const QSize &outputSize)
{
    if (document.size.isEmpty() || outputSize.isEmpty()
        || (stroke.mode != StrokeMode::Paint
            && stroke.mode != StrokeMode::Erase))
    {
        return {};
    }
    Layer layer;
    layer.initialCanvasSize = document.size;
    layer.strokes.append(stroke);
    const StrokeCoveragePlan plan = prepareStrokeCoverage(document, layer);
    const QRect nativeBounds =
        conservativeStrokeCoverageBounds(document, layer, 0, plan);
    if (nativeBounds.isEmpty())
    {
        return {};
    }
    const qreal horizontalScale =
        static_cast<qreal>(outputSize.width()) / document.size.width();
    const qreal verticalScale =
        static_cast<qreal>(outputSize.height()) / document.size.height();
    const QRectF mapped(nativeBounds.x() * horizontalScale,
        nativeBounds.y() * verticalScale,
        nativeBounds.width() * horizontalScale,
        nativeBounds.height() * verticalScale);
    return mapped.toAlignedRect()
        .adjusted(-1, -1, 1, 1)
        .intersected(QRect(QPoint(), outputSize));
}

RenderEngine::RegionalStrokeRefresh RenderEngine::prepareRegionalStrokeRefresh(
    const Document &document,
    const QUuid &layerId,
    const QUuid &strokeId,
    const QSize &outputSize,
    const QRect &additionalNativeBounds)
{
    RegionalStrokeRefresh refresh;
    if (document.size.isEmpty() || outputSize.isEmpty())
    {
        return refresh;
    }
    const QRect canvasBounds(QPoint(), document.size);
    // A reframe rewrites the whole framebuffer and its own epoch's coordinate
    // system, so the native bounds this plan is expressed in stop meaning the
    // same thing partway through the document. Nothing here can describe that.
    for (const Layer &layer : document.layers)
    {
        if (layer.initialCanvasSize.isValid()
            && layer.initialCanvasSize != document.size)
        {
            return refresh;
        }
        for (const Stroke &stroke : layer.strokes)
        {
            if (stroke.reframeOp || stroke.mode == StrokeMode::Reframe)
            {
                return refresh;
            }
        }
    }

    const Layer *targetLayer = document.layer(layerId);
    if (!targetLayer || targetLayer->kind != LayerKind::Paint)
    {
        return refresh;
    }
    int strokeIndex = -1;
    for (int index = 0; index < targetLayer->strokes.size(); ++index)
    {
        if (targetLayer->strokes[index].id == strokeId)
        {
            strokeIndex = index;
            break;
        }
    }
    if (strokeIndex < 0)
    {
        return refresh;
    }
    const Stroke &target = targetLayer->strokes[strokeIndex];
    if (target.mode != StrokeMode::Paint && target.mode != StrokeMode::Erase)
    {
        return refresh;
    }
    const StrokeCoveragePlan targetPlan =
        prepareStrokeCoverage(document, *targetLayer);
    if (!targetPlan.valid)
    {
        return refresh;
    }
    QRect nativeBounds = conservativeStrokeCoverageBounds(
        document, *targetLayer, strokeIndex, targetPlan);
    if (nativeBounds.isEmpty())
    {
        return refresh;
    }
    if (!additionalNativeBounds.isEmpty())
    {
        nativeBounds = nativeBounds.united(
            additionalNativeBounds.intersected(canvasBounds));
    }

    const qreal horizontalScale =
        static_cast<qreal>(outputSize.width()) / document.size.width();
    const qreal verticalScale =
        static_cast<qreal>(outputSize.height()) / document.size.height();
    // Kept strokes must cover the native sampling footprint of every output
    // pixel inside outputBounds, including the resampler's neighborhood on
    // the native-exact fallback path.
    const int marginX = qCeil(2.0 / std::min<qreal>(1.0, horizontalScale)) + 4;
    const int marginY = qCeil(2.0 / std::min<qreal>(1.0, verticalScale)) + 4;
    const QRect filterBounds =
        nativeBounds.adjusted(-marginX, -marginY, marginX, marginY)
            .intersected(canvasBounds);

    refresh.filteredDocument = document;
    for (int layerIndex = 0; layerIndex < document.layers.size(); ++layerIndex)
    {
        const Layer &layer = document.layers[layerIndex];
        if (layer.kind != LayerKind::Paint || layer.strokes.isEmpty())
        {
            continue;
        }
        const StrokeCoveragePlan plan = prepareStrokeCoverage(document, layer);
        if (!plan.valid || plan.primitiveBounds.size() != layer.strokes.size())
        {
            // Keeping every stroke of a layer the plan cannot describe is
            // always correct, only slower.
            continue;
        }
        // A pixel-selection operation carries pixels away from where they were
        // painted, so a stroke this layer draws far outside filterBounds can
        // still end up inside it. Such a layer keeps every stroke. The
        // refreshed stroke's own bounds are a separate matter that
        // conservativeStrokeCoverageBounds already follows through them.
        if (std::any_of(layer.strokes.cbegin(),
                layer.strokes.cend(),
                [](const Stroke &stroke)
                {
                    return stroke.mode == StrokeMode::PixelSelection
                           || stroke.pixelSelectionOp.has_value();
                }))
        {
            continue;
        }
        QVector<Stroke> kept;
        kept.reserve(layer.strokes.size());
        for (int index = 0; index < layer.strokes.size(); ++index)
        {
            const Stroke &stroke = layer.strokes[index];
            const bool droppable = stroke.mode == StrokeMode::Paint
                                   || stroke.mode == StrokeMode::Erase
                                   || stroke.mode == StrokeMode::Fill;
            if (!droppable
                || plan.primitiveBounds[index].intersects(filterBounds))
            {
                kept.append(stroke);
            }
        }
        refresh.filteredDocument.layers[layerIndex].strokes = std::move(kept);
    }

    const QRectF mappedRefresh(nativeBounds.x() * horizontalScale,
        nativeBounds.y() * verticalScale,
        nativeBounds.width() * horizontalScale,
        nativeBounds.height() * verticalScale);
    refresh.nativeBounds = nativeBounds;
    refresh.outputBounds = mappedRefresh.toAlignedRect()
                               .adjusted(-1, -1, 1, 1)
                               .intersected(QRect(QPoint(), outputSize));
    refresh.valid = !refresh.outputBounds.isEmpty();
    return refresh;
}

QImage RenderEngine::renderStrokeCoverage(const Document &document,
    const Layer &layer,
    int strokeIndex,
    int frameIndex)
{
    if (!document.size.isValid() || layer.kind != LayerKind::Paint
        || strokeIndex < 0 || strokeIndex >= layer.strokes.size())
    {
        return {};
    }
    const Stroke &source = layer.strokes[strokeIndex];
    if (source.mode != StrokeMode::Paint && source.mode != StrokeMode::Erase
        && source.mode != StrokeMode::Fill && source.mode != StrokeMode::Image)
    {
        return {};
    }

    QSize epochSize = layer.initialCanvasSize.isValid()
                          ? layer.initialCanvasSize
                          : DocumentOperations::initialCanvasSize(
                                layer.strokes, document.size);
    // Erase and pixel-selection effects reach a stroke only from its own
    // section or from the strokes after the last composite boundary, which
    // draw on the flattened composite.
    int totalBoundaries = 0;
    for (const Stroke &operation : layer.strokes)
    {
        if (operation.mode == StrokeMode::CompositeBoundary)
        {
            ++totalBoundaries;
        }
    }
    int sourceSection = 0;
    for (int index = 0; index < strokeIndex; ++index)
    {
        const Stroke &operation = layer.strokes[index];
        if (operation.mode == StrokeMode::Reframe)
        {
            if (!operation.reframeOp
                || operation.reframeOp->sourceSize != epochSize)
            {
                return {};
            }
            epochSize = operation.reframeOp->targetSize;
        }
        else if (operation.mode == StrokeMode::CompositeBoundary)
        {
            ++sourceSection;
        }
    }
    if (!epochSize.isValid())
    {
        return {};
    }

    QImage coverage(epochSize, QImage::Format_ARGB32_Premultiplied);
    if (coverage.isNull())
    {
        return {};
    }
    coverage.fill(Qt::transparent);
    const int frameCount = std::max(1, document.animationFrames);
    const int normalizedFrame =
        ((frameIndex % frameCount) + frameCount) % frameCount;
    // Coverage has to move exactly like the layer it belongs to.
    const Document layerDocument = documentForLayer(document, layer);
    QHash<qint64, QPainterPath> clipPaths;
    QHash<qint64, QImage> scaledClipMasks;
    if (source.mode == StrokeMode::Image)
    {
        if (!source.imageOp
            || !applyImageOperation(coverage, document, *source.imageOp))
        {
            return {};
        }
    }
    else
    {
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
        renderLayerStrokes(coverage,
            layerDocument,
            {probe},
            normalizedFrame,
            frameCount,
            1.0,
            1.0,
            clipPaths,
            scaledClipMasks);
    }

    int currentSection = sourceSection;
    for (int index = strokeIndex + 1; index < layer.strokes.size(); ++index)
    {
        const Stroke &operation = layer.strokes[index];
        if (operation.mode == StrokeMode::CompositeBoundary)
        {
            ++currentSection;
            continue;
        }
        const bool reachesSource = currentSection == sourceSection
                                   || currentSection == totalBoundaries;
        if (operation.mode == StrokeMode::PixelSelection)
        {
            if (!reachesSource)
            {
                continue;
            }
            if (!operation.pixelSelectionOp
                || !applyPixelSelectionOperation(
                    coverage, *operation.pixelSelectionOp))
            {
                return {};
            }
        }
        else if (operation.mode == StrokeMode::Reframe)
        {
            if (!operation.reframeOp
                || !applyReframeOperation(coverage, *operation.reframeOp))
            {
                return {};
            }
        }
        else if (operation.mode == StrokeMode::Erase && reachesSource)
        {
            renderLayerStrokes(coverage,
                layerDocument,
                {operation},
                normalizedFrame,
                frameCount,
                1.0,
                1.0,
                clipPaths,
                scaledClipMasks);
        }
    }
    return coverage.size() == document.size ? coverage : QImage();
}

RenderEngine::StrokeCoveragePlan RenderEngine::prepareStrokeCoverage(
    const Document &document, const Layer &layer)
{
    return StrokeCoverageRenderer::prepare(document, layer);
}

QRect RenderEngine::conservativeStrokeCoverageBounds(const Document &document,
    const Layer &layer,
    int strokeIndex,
    const StrokeCoveragePlan &plan)
{
    return StrokeCoverageRenderer::conservativeBounds(
        document, layer, strokeIndex, plan);
}

RenderEngine::StrokeCoverageRegion RenderEngine::renderSparseStrokeCoverage(
    const Document &document,
    const Layer &layer,
    int strokeIndex,
    int frameIndex,
    const QRect &outputBounds,
    const StrokeCoveragePlan &plan,
    StrokeCoverageStats *stats)
{
    return StrokeCoverageRenderer::render(
        document, layer, strokeIndex, frameIndex, outputBounds, plan, stats);
}

QImage RenderEngine::renderStrokeCoverageRegion(const Document &document,
    const Layer &layer,
    int strokeIndex,
    int frameIndex,
    const QRect &outputBounds)
{
    const QRect documentBounds(QPoint(), document.size);
    if (!document.size.isValid() || layer.kind != LayerKind::Paint
        || strokeIndex < 0 || strokeIndex >= layer.strokes.size()
        || outputBounds.isEmpty() || !documentBounds.contains(outputBounds))
    {
        return {};
    }
    const StrokeCoveragePlan plan = prepareStrokeCoverage(document, layer);
    const StrokeCoverageRegion sparse = renderSparseStrokeCoverage(
        document, layer, strokeIndex, frameIndex, outputBounds, plan);
    if (!sparse.valid)
    {
        return {};
    }
    QImage coverage(outputBounds.size(), QImage::Format_ARGB32_Premultiplied);
    if (coverage.isNull())
    {
        return {};
    }
    coverage.fill(Qt::transparent);
    if (!sparse.image.isNull())
    {
        QPainter painter(&coverage);
        painter.setCompositionMode(QPainter::CompositionMode_Source);
        painter.drawImage(
            sparse.bounds.topLeft() - outputBounds.topLeft(), sparse.image);
    }
    return coverage;
}

QPainterPath RenderEngine::strokePath(
    const Stroke &stroke, int frameIndex, int frameCount, qreal wobbleAmount)
{
    return StrokeRenderer::path(stroke, frameIndex, frameCount, wobbleAmount);
}
}

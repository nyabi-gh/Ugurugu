// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "document/DocumentOperations.hpp"

#include "document/DocumentLimits.hpp"
#include "document/SelectionOperation.hpp"

#include <QSet>

#include <utility>

namespace ugurugu
{
namespace DocumentOperations
{

namespace
{

bool validCanvasSize(const QSize &size)
{
    return size.width() >= DocumentLimits::minimumCanvasEdge
           && size.height() >= DocumentLimits::minimumCanvasEdge
           && size.width() <= DocumentLimits::maximumCanvasEdge
           && size.height() <= DocumentLimits::maximumCanvasEdge;
}

}

QSize initialCanvasSize(
    const QVector<Stroke> &operations, const QSize &fallback)
{
    for (const Stroke &operation : operations)
    {
        if (operation.reframeOp)
        {
            return operation.reframeOp->sourceSize;
        }
        if (operation.pixelSelectionOp)
        {
            return operation.pixelSelectionOp->canvasSize;
        }
    }
    return fallback;
}

bool normalizeAndValidate(Document &document)
{
    if (!validCanvasSize(document.size))
    {
        return false;
    }

    Document candidate = document;
    quint64 rasterDecodedBytes = 0;
    qint64 rasterPayloadBytes = 0;
    for (auto asset = candidate.rasterAssets.cbegin();
        asset != candidate.rasterAssets.cend();
        ++asset)
    {
        if (asset.key() != asset->id || !isValidRasterAssetMetadata(*asset))
        {
            return false;
        }
        const quint64 decoded = static_cast<quint64>(asset->size.width())
                                * static_cast<quint64>(asset->size.height())
                                * 4ULL;
        if (decoded > DocumentLimits::maximumDistinctRasterDecodedBytes
                          - rasterDecodedBytes
            || asset->compressedRgba.size()
                   > DocumentLimits::maximumDistinctRasterPayloadBytes
                         - rasterPayloadBytes)
        {
            return false;
        }
        rasterDecodedBytes += decoded;
        rasterPayloadBytes += asset->compressedRgba.size();
    }
    QSet<qint64> maskKeys;
    quint64 distinctMaskBytes = 0;
    const auto registerMask = [&maskKeys, &distinctMaskBytes](
                                  const QImage &mask)
    {
        if (mask.isNull() || maskKeys.contains(mask.cacheKey()))
        {
            return true;
        }
        const quint64 bytes = mask.sizeInBytes();
        if (bytes
            > DocumentLimits::maximumDistinctClipMaskBytes - distinctMaskBytes)
        {
            return false;
        }
        maskKeys.insert(mask.cacheKey());
        distinctMaskBytes += bytes;
        return true;
    };

    for (Layer &layer : candidate.layers)
    {
        const QSize initialSize =
            layer.initialCanvasSize.isValid()
                ? layer.initialCanvasSize
                : initialCanvasSize(layer.strokes, candidate.size);
        if (!validCanvasSize(initialSize))
        {
            return false;
        }
        layer.initialCanvasSize = initialSize;
        QSize epochSize = initialSize;
        for (const Stroke &stroke : std::as_const(layer.strokes))
        {
            if (stroke.mode == StrokeMode::PixelSelection)
            {
                if (!stroke.pixelSelectionOp || stroke.reframeOp
                    || stroke.imageOp || !stroke.points.isEmpty()
                    || stroke.visibilityClip || !stroke.clipMask.isNull()
                    || !stroke.fillMask.isNull() || stroke.fillCoverage
                    || stroke.pixelSelectionOp->canvasSize != epochSize
                    || !isValidPixelSelectionOp(*stroke.pixelSelectionOp))
                {
                    return false;
                }
                continue;
            }
            if (stroke.mode == StrokeMode::Reframe)
            {
                if (!stroke.reframeOp || stroke.pixelSelectionOp
                    || stroke.imageOp || !stroke.points.isEmpty()
                    || stroke.visibilityClip || !stroke.clipMask.isNull()
                    || !stroke.fillMask.isNull() || stroke.fillCoverage
                    || stroke.reframeOp->sourceSize != epochSize
                    || !isValidReframeOp(*stroke.reframeOp))
                {
                    return false;
                }
                epochSize = stroke.reframeOp->targetSize;
                continue;
            }
            if (stroke.mode == StrokeMode::CompositeBoundary)
            {
                if (stroke.pixelSelectionOp || stroke.reframeOp
                    || stroke.imageOp || !stroke.points.isEmpty()
                    || stroke.visibilityClip || !stroke.clipMask.isNull()
                    || !stroke.fillMask.isNull() || stroke.fillCoverage)
                {
                    return false;
                }
                continue;
            }
            if (stroke.mode == StrokeMode::Image)
            {
                if (!stroke.imageOp || stroke.pixelSelectionOp
                    || stroke.reframeOp || !stroke.points.isEmpty()
                    || stroke.visibilityClip || !stroke.clipMask.isNull()
                    || !stroke.fillMask.isNull() || stroke.fillCoverage
                    || !isValidImageOp(*stroke.imageOp)
                    || !candidate.rasterAssets.contains(
                        stroke.imageOp->assetId))
                {
                    return false;
                }
                continue;
            }
            if ((stroke.mode != StrokeMode::Paint
                    && stroke.mode != StrokeMode::Erase
                    && stroke.mode != StrokeMode::Fill)
                || stroke.pixelSelectionOp || stroke.reframeOp || stroke.imageOp
                || stroke.points.isEmpty()
                || (!stroke.clipMask.isNull()
                    && (stroke.clipMask.size() != epochSize
                        || stroke.clipMask.format()
                               != QImage::Format_Grayscale8))
                || (!stroke.fillMask.isNull()
                    && (stroke.mode != StrokeMode::Fill
                        || stroke.fillMask.size() != epochSize
                        || stroke.fillMask.format()
                               != QImage::Format_Grayscale8))
                || (stroke.fillCoverage
                    && (stroke.mode != StrokeMode::Fill
                        || !stroke.fillMask.isNull()
                        || !isValidPackedMaskRegion(*stroke.fillCoverage)
                        || stroke.fillCoverage->canvasSize != epochSize))
                || !registerMask(stroke.clipMask)
                || !registerMask(stroke.fillMask))
            {
                return false;
            }
        }
        if (epochSize != candidate.size)
        {
            return false;
        }
    }

    const quint64 packedBytes = packedSelectionBytes(candidate);
    if (packedBytes
        > DocumentLimits::maximumDistinctClipMaskBytes - distinctMaskBytes)
    {
        return false;
    }
    document = std::move(candidate);
    return true;
}

bool isLayerRenderable(const Document &document, const Layer &layer)
{
    if (!layer.visible || layer.opacity <= 0.0)
    {
        return false;
    }
    QUuid parentId = layer.parentGroupId;
    // Bounded by the layer count so a malformed hierarchy with a parent cycle
    // answers instead of spinning; an unterminated walk is not visible.
    for (qsizetype step = 0; step < document.layers.size(); ++step)
    {
        if (parentId.isNull())
        {
            return true;
        }
        const Layer *parent = document.layer(parentId);
        if (!parent || !parent->visible || parent->opacity <= 0.0)
        {
            return false;
        }
        parentId = parent->parentGroupId;
    }
    return parentId.isNull();
}

Document isolatedLayerDocument(const Document &document, const Layer &layer)
{
    Document single = document;
    single.background = QColor(0, 0, 0, 0);
    Layer isolated = layer;
    isolated.visible = true;
    isolated.opacity = 1.0;
    // A layer kept inside its group, clipped to the layer below, or carrying a
    // blend mode cannot composite on its own: the hierarchy compositor rejects
    // a missing parent outright and skips a clipped layer that has no base, so
    // both used to render nothing at all.
    isolated.parentGroupId = QUuid();
    isolated.clipToLayerBelow = false;
    isolated.blendMode = LayerBlendMode::Normal;
    single.activeLayerId = isolated.id;
    single.layers = {std::move(isolated)};
    return single;
}

}

}

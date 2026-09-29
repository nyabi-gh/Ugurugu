// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "io/serializer/DocumentValidation.hpp"

// Messages keep using DocumentSerializer::tr so their translation context
// stays ugurugu::DocumentSerializer, matching the existing i18n catalogues.
#include "document/DocumentLimits.hpp"
#include "document/DocumentOperations.hpp"
#include "document/LayerHierarchy.hpp"
#include "document/SelectionOperation.hpp"
#include "io/DocumentSerializer.hpp"
#include "io/serializer/DocumentJsonCodec.hpp"
#include "io/serializer/RasterAssetTable.hpp"

#include <QHash>
#include <QJsonObject>
#include <QSet>

#include <algorithm>
#include <cmath>
#include <limits>

namespace ugurugu
{
namespace serializer_detail
{

bool validateCollectionBudgets(const QJsonArray &layers, QString *error)
{
    qsizetype remainingStrokes = DocumentLimits::maximumTotalStrokes;
    qsizetype remainingPoints = DocumentLimits::maximumTotalPoints;

    for (const auto &layerValue : layers)
    {
        if (!layerValue.isObject())
        {
            continue;
        }
        const QJsonValue strokesValue =
            layerValue.toObject().value(QStringLiteral("strokes"));
        if (!strokesValue.isArray())
        {
            continue;
        }
        const QJsonArray strokes = strokesValue.toArray();
        if (strokes.size() > DocumentLimits::maximumStrokesPerLayer
            || strokes.size() > remainingStrokes)
        {
            setError(error,
                DocumentSerializer::tr(
                    "The project contains too many strokes."));
            return false;
        }
        remainingStrokes -= strokes.size();

        for (const auto &strokeValue : strokes)
        {
            if (!strokeValue.isObject())
            {
                continue;
            }
            const QJsonValue pointsValue =
                strokeValue.toObject().value(QStringLiteral("points"));
            if (!pointsValue.isArray())
            {
                continue;
            }
            const qsizetype pointCount = pointsValue.toArray().size();
            if (pointCount > DocumentLimits::maximumPointsPerStroke
                || pointCount > remainingPoints)
            {
                setError(error,
                    DocumentSerializer::tr(
                        "The project contains too many points."));
                return false;
            }
            remainingPoints -= pointCount;
        }
    }
    return true;
}

bool validateDocument(const Document &document,
    int fileSchemaVersion,
    QString *error,
    DocumentValidationStats *stats,
    const RasterAssetTable *validatedRasterAssets)
{
    if (document.size.width() < DocumentLimits::minimumCanvasEdge
        || document.size.height() < DocumentLimits::minimumCanvasEdge
        || document.size.width() > DocumentLimits::maximumCanvasEdge
        || document.size.height() > DocumentLimits::maximumCanvasEdge)
    {
        setError(error, DocumentSerializer::tr("The canvas size is invalid."));
        return false;
    }
    if (!document.background.isValid())
    {
        setError(
            error, DocumentSerializer::tr("The canvas background is invalid."));
        return false;
    }
    if (document.animationFrames < DocumentLimits::minimumAnimationFrames
        || document.animationFrames > DocumentLimits::maximumAnimationFrames
        || !std::isfinite(document.framesPerSecond)
        || document.framesPerSecond < DocumentLimits::minimumFramesPerSecond
        || document.framesPerSecond > DocumentLimits::maximumFramesPerSecond
        || !std::isfinite(document.wobbleAmount)
        || document.wobbleAmount < DocumentLimits::minimumWobbleAmount
        || document.wobbleAmount > DocumentLimits::maximumWobbleAmount
        || !isValidMotionSettings(document.motion, document.animationFrames))
    {
        setError(error,
            DocumentSerializer::tr("The animation settings are invalid."));
        return false;
    }
    if (document.layers.size() > DocumentLimits::maximumLayers)
    {
        setError(error, DocumentSerializer::tr("The layer count is invalid."));
        return false;
    }
    if (document.rasterAssets.size() > DocumentLimits::maximumRasterAssets)
    {
        setError(error,
            DocumentSerializer::tr(
                "The project contains too many raster assets."));
        return false;
    }
    if (fileSchemaVersion < 11 && !document.rasterAssets.isEmpty())
    {
        setError(error,
            DocumentSerializer::tr(
                "Raster assets require a newer project version."));
        return false;
    }
    if (validatedRasterAssets
        && validatedRasterAssets->entries().size()
               != document.rasterAssets.size())
    {
        setError(error,
            DocumentSerializer::tr(
                "The project contains an invalid raster asset."));
        return false;
    }
    RasterAssetTable rasterAssets;
    for (auto asset = document.rasterAssets.cbegin();
        asset != document.rasterAssets.cend();
        ++asset)
    {
        bool valid = asset.key() == asset->id;
        if (validatedRasterAssets)
        {
            const auto entry =
                validatedRasterAssets->entries().constFind(asset.key());
            valid = valid && entry != validatedRasterAssets->entries().cend()
                    && entry->id == asset->id && entry->size == asset->size
                    && entry->compressedRgba == asset->compressedRgba;
        }
        else
        {
            const RasterAssetRegistrationResult result =
                rasterAssets.registerPayload(
                    asset->id, asset->size, asset->compressedRgba);
            valid =
                valid
                && result.status == RasterAssetRegistrationStatus::Registered;
        }
        if (!valid)
        {
            setError(error,
                DocumentSerializer::tr(
                    "The project contains an invalid raster asset."));
            return false;
        }
    }

    QSet<QUuid> layerIds;
    QSet<QUuid> strokeIds;
    QSet<qint64> clipMaskKeys;
    QSet<quintptr> packedMaskKeys;
    qsizetype totalStrokes = 0;
    qsizetype totalPoints = 0;
    quint64 clipMaskBytes = 0;
    bool activeLayerFound = false;
    for (const Layer &layer : document.layers)
    {
        if (layer.id.isNull() || layerIds.contains(layer.id))
        {
            setError(error,
                DocumentSerializer::tr(
                    "The project contains invalid layer IDs."));
            return false;
        }
        layerIds.insert(layer.id);
        activeLayerFound = activeLayerFound
                           || (layer.id == document.activeLayerId
                               && layer.kind == LayerKind::Paint);
        if (layer.name.trimmed().isEmpty()
            || layer.name.size() > DocumentLimits::maximumLayerNameLength
            || !std::isfinite(layer.opacity) || layer.opacity < 0.0
            || layer.opacity > 1.0 || !isValidLayerBlendMode(layer.blendMode)
            || !isValidLayerKind(layer.kind)
            || (layer.kind == LayerKind::Group
                && (!layer.strokes.isEmpty() || layer.clipToLayerBelow
                    || layer.reference))
            || layer.strokes.size() > DocumentLimits::maximumStrokesPerLayer
            || layer.strokes.size()
                   > DocumentLimits::maximumTotalStrokes - totalStrokes
            || (layer.wobbleAmount
                && (!std::isfinite(*layer.wobbleAmount)
                    || *layer.wobbleAmount < DocumentLimits::minimumWobbleAmount
                    || *layer.wobbleAmount
                           > DocumentLimits::maximumWobbleAmount))
            || (layer.motion
                && !isValidMotionSettings(
                    *layer.motion, document.animationFrames))
            || layer.wobbleAmount.has_value() != layer.motion.has_value()
            || (layer.kind == LayerKind::Group
                && (layer.wobbleAmount || layer.motion)))
        {
            setError(error,
                DocumentSerializer::tr("A layer contains invalid data."));
            return false;
        }
        totalStrokes += layer.strokes.size();

        const auto validCanvasSize = [](const QSize &size)
        {
            return size.width() >= DocumentLimits::minimumCanvasEdge
                   && size.height() >= DocumentLimits::minimumCanvasEdge
                   && size.width() <= DocumentLimits::maximumCanvasEdge
                   && size.height() <= DocumentLimits::maximumCanvasEdge;
        };
        if (!validCanvasSize(layer.initialCanvasSize))
        {
            setError(error,
                DocumentSerializer::tr(
                    "A layer has an invalid initial canvas size."));
            return false;
        }
        QSize epochSize = layer.initialCanvasSize;
        for (const Stroke &stroke : layer.strokes)
        {
            if (stroke.id.isNull() || strokeIds.contains(stroke.id))
            {
                setError(error,
                    DocumentSerializer::tr(
                        "The project contains invalid stroke IDs."));
                return false;
            }
            strokeIds.insert(stroke.id);
            const auto validateMaskBudget =
                [&clipMaskKeys, &clipMaskBytes, error](const QImage &mask)
            {
                if (mask.isNull() || clipMaskKeys.contains(mask.cacheKey()))
                {
                    return true;
                }
                const quint64 maskBytes = mask.sizeInBytes();
                if (maskBytes > DocumentLimits::maximumDistinctClipMaskBytes
                                    - clipMaskBytes)
                {
                    setError(error,
                        DocumentSerializer::tr(
                            "The project contains too much mask data."));
                    return false;
                }
                clipMaskKeys.insert(mask.cacheKey());
                clipMaskBytes += maskBytes;
                return true;
            };
            if (!validateMaskBudget(stroke.clipMask)
                || !validateMaskBudget(stroke.fillMask))
            {
                return false;
            }
            const auto validatePackedBudget =
                [&packedMaskKeys, &clipMaskBytes, error](
                    const QByteArray &packed)
            {
                const quintptr backing =
                    reinterpret_cast<quintptr>(packed.constData());
                if (packedMaskKeys.contains(backing))
                {
                    return true;
                }
                const quint64 bytes = static_cast<quint64>(packed.size());
                if (bytes > DocumentLimits::maximumDistinctClipMaskBytes
                                - clipMaskBytes)
                {
                    setError(error,
                        DocumentSerializer::tr(
                            "The project contains too much binary mask data."));
                    return false;
                }
                packedMaskKeys.insert(backing);
                clipMaskBytes += bytes;
                return true;
            };
            if (stroke.pixelSelectionOp
                && !validatePackedBudget(stroke.pixelSelectionOp->packedMask))
            {
                return false;
            }
            if (stroke.fillCoverage
                && !validatePackedBudget(stroke.fillCoverage->packedMask))
            {
                return false;
            }

            const bool validCommonFields =
                stroke.color.isValid() && std::isfinite(stroke.width)
                && stroke.width >= DocumentLimits::minimumStrokeWidth
                && stroke.width <= DocumentLimits::maximumStrokeWidth
                && isValidBrushSettings(stroke.brush);
            if (!validCommonFields)
            {
                setError(error,
                    DocumentSerializer::tr("A stroke contains invalid data."));
                return false;
            }

            if (stroke.mode == StrokeMode::PixelSelection)
            {
                if (fileSchemaVersion < 6 || !stroke.pixelSelectionOp
                    || stroke.reframeOp || stroke.imageOp
                    || !stroke.points.isEmpty() || stroke.visibilityClip
                    || !stroke.clipMask.isNull() || !stroke.fillMask.isNull()
                    || stroke.fillCoverage
                    || !isValidPixelSelectionOp(*stroke.pixelSelectionOp)
                    || stroke.pixelSelectionOp->canvasSize != epochSize)
                {
                    setError(error,
                        DocumentSerializer::tr(
                            "A pixel selection operation is invalid."));
                    return false;
                }
                continue;
            }
            if (stroke.mode == StrokeMode::Reframe)
            {
                if (fileSchemaVersion < 6 || !stroke.reframeOp
                    || stroke.pixelSelectionOp || stroke.imageOp
                    || !stroke.points.isEmpty() || stroke.visibilityClip
                    || !stroke.clipMask.isNull() || !stroke.fillMask.isNull()
                    || stroke.fillCoverage
                    || !isValidReframeOp(*stroke.reframeOp)
                    || stroke.reframeOp->sourceSize != epochSize)
                {
                    setError(error,
                        DocumentSerializer::tr(
                            "A reframe operation is invalid."));
                    return false;
                }
                epochSize = stroke.reframeOp->targetSize;
                continue;
            }
            if (stroke.mode == StrokeMode::CompositeBoundary)
            {
                if (fileSchemaVersion < 13 || stroke.pixelSelectionOp
                    || stroke.reframeOp || stroke.imageOp
                    || !stroke.points.isEmpty() || stroke.visibilityClip
                    || !stroke.clipMask.isNull() || !stroke.fillMask.isNull()
                    || stroke.fillCoverage)
                {
                    setError(error,
                        DocumentSerializer::tr(
                            "A composite boundary is invalid."));
                    return false;
                }
                continue;
            }
            if (stroke.mode == StrokeMode::Image)
            {
                if (fileSchemaVersion < 11 || !stroke.imageOp
                    || stroke.pixelSelectionOp || stroke.reframeOp
                    || !stroke.points.isEmpty() || stroke.visibilityClip
                    || !stroke.clipMask.isNull() || !stroke.fillMask.isNull()
                    || stroke.fillCoverage || !isValidImageOp(*stroke.imageOp)
                    || !document.rasterAssets.contains(stroke.imageOp->assetId))
                {
                    setError(error,
                        DocumentSerializer::tr(
                            "An image operation is invalid."));
                    return false;
                }
                continue;
            }

            const QRect canvasRect(QPoint(), epochSize);
            const bool invalidVisibilityClip =
                stroke.visibilityClip
                && (stroke.visibilityClip->isEmpty()
                    || !canvasRect.contains(*stroke.visibilityClip));
            const bool invalidFillMask =
                (!stroke.fillMask.isNull()
                    && (stroke.mode != StrokeMode::Fill
                        || stroke.fillMask.size() != epochSize
                        || stroke.fillMask.format()
                               != QImage::Format_Grayscale8));
            const bool invalidFillCoverage =
                stroke.fillCoverage
                && (fileSchemaVersion < 10 || stroke.mode != StrokeMode::Fill
                    || !stroke.fillMask.isNull()
                    || !isValidPackedMaskRegion(*stroke.fillCoverage)
                    || stroke.fillCoverage->canvasSize != epochSize);
            if ((stroke.mode != StrokeMode::Paint
                    && stroke.mode != StrokeMode::Erase
                    && stroke.mode != StrokeMode::Fill)
                || stroke.pixelSelectionOp || stroke.reframeOp || stroke.imageOp
                || stroke.points.isEmpty()
                || stroke.points.size() > DocumentLimits::maximumPointsPerStroke
                || (!stroke.clipMask.isNull()
                    && (stroke.clipMask.size() != epochSize
                        || stroke.clipMask.format()
                               != QImage::Format_Grayscale8))
                || invalidVisibilityClip || invalidFillMask
                || invalidFillCoverage
                || stroke.points.size()
                       > DocumentLimits::maximumTotalPoints - totalPoints)
            {
                setError(error,
                    DocumentSerializer::tr("A stroke contains invalid data."));
                return false;
            }
            totalPoints += stroke.points.size();
            for (const StrokePoint &point : stroke.points)
            {
                const bool outsideLegacyCanvas =
                    fileSchemaVersion <= 4
                    && (point.position.x() < 0.0 || point.position.y() < 0.0
                        || point.position.x() > epochSize.width()
                        || point.position.y() > epochSize.height());
                if (!std::isfinite(point.position.x())
                    || !std::isfinite(point.position.y())
                    || !std::isfinite(point.pressure)
                    || std::abs(point.position.x())
                           > DocumentLimits::maximumStoredCoordinateMagnitude
                    || std::abs(point.position.y())
                           > DocumentLimits::maximumStoredCoordinateMagnitude
                    || outsideLegacyCanvas || point.pressure < 0.0
                    || point.pressure > 1.0)
                {
                    setError(error,
                        DocumentSerializer::tr(
                            "A stroke contains an invalid point."));
                    return false;
                }
            }
        }
        if (epochSize != document.size)
        {
            setError(error,
                DocumentSerializer::tr(
                    "A layer does not end at the document canvas size."));
            return false;
        }
    }
    if (!analyzeLayerHierarchy(document).isValid())
    {
        setError(
            error, DocumentSerializer::tr("The layer hierarchy is invalid."));
        return false;
    }
    const bool validActiveLayer =
        !std::any_of(document.layers.cbegin(),
            document.layers.cend(),
            [](const Layer &layer)
            {
                return layer.kind == LayerKind::Paint;
            })
            ? document.activeLayerId.isNull()
            : !document.activeLayerId.isNull() && activeLayerFound;
    if (!validActiveLayer)
    {
        setError(
            error, DocumentSerializer::tr("The active layer ID is invalid."));
        return false;
    }
    if (stats)
    {
        stats->totalStrokeCount = totalStrokes;
        stats->totalPointCount = totalPoints;
        stats->distinctMaskBytes = clipMaskBytes;
    }
    return true;
}

void normalizeLayerInitialCanvasSizes(Document &document)
{
    for (Layer &layer : document.layers)
    {
        if (layer.initialCanvasSize.isValid())
        {
            continue;
        }
        layer.initialCanvasSize = document.size;
        for (const Stroke &operation : layer.strokes)
        {
            if (operation.reframeOp)
            {
                layer.initialCanvasSize = operation.reframeOp->sourceSize;
                break;
            }
            if (operation.pixelSelectionOp)
            {
                layer.initialCanvasSize =
                    operation.pixelSelectionOp->canvasSize;
                break;
            }
        }
    }
}
}

}

// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "io/serializer/PreparedPlanBuilder.hpp"

#include "document/DocumentLimits.hpp"
#include "document/DocumentOperations.hpp"
#include "document/LayerHierarchy.hpp"
#include "document/SelectionOperation.hpp"
#include "io/serializer/DocumentValidation.hpp"

#include <QCryptographicHash>
#include <QJsonArray>
#include <QJsonDocument>
#include <QSet>

#include <algorithm>
#include <cmath>
#include <limits>
#include <utility>

namespace ugurugu
{
namespace serializer_detail
{

bool addSerializedBytes(qint64 &total, qint64 amount, qint64 maximumBytes)
{
    if (amount < 0 || amount > maximumBytes || total > maximumBytes - amount)
    {
        return false;
    }
    total += amount;
    return true;
}

bool sameImageIdentity(const QImage &left, const QImage &right)
{
    return left.isNull() == right.isNull()
           && (left.isNull()
               || (left.cacheKey() == right.cacheKey()
                   && left.size() == right.size()
                   && left.format() == right.format()));
}

bool samePackedIdentity(const QByteArray &left, const QByteArray &right)
{
    return left.size() == right.size()
           && (left.isEmpty() || left.constData() == right.constData());
}

bool samePixelSelectionIdentity(const std::optional<PixelSelectionOp> &left,
    const std::optional<PixelSelectionOp> &right)
{
    if (left.has_value() != right.has_value())
    {
        return false;
    }
    if (!left)
    {
        return true;
    }
    return left->canvasSize == right->canvasSize
           && left->sourceBounds == right->sourceBounds
           && samePackedIdentity(left->packedMask, right->packedMask)
           && left->transform == right->transform
           && left->sampling == right->sampling
           && left->clearSource == right->clearSource
           && left->drawDestination == right->drawDestination;
}

bool sameStrokeIdentity(const Stroke &left, const Stroke &right)
{
    const bool samePoints =
        left.points.size() == right.points.size()
        && (left.points.isEmpty()
            || left.points.constData() == right.points.constData());
    return left.id == right.id && left.seed == right.seed
           && left.mode == right.mode && left.color == right.color
           && left.width == right.width && left.brush == right.brush
           && samePoints && left.visibilityClip == right.visibilityClip
           && sameImageIdentity(left.clipMask, right.clipMask)
           && sameImageIdentity(left.fillMask, right.fillMask)
           && ((!left.fillCoverage && !right.fillCoverage)
               || (left.fillCoverage && right.fillCoverage
                   && left.fillCoverage->canvasSize
                          == right.fillCoverage->canvasSize
                   && left.fillCoverage->bounds == right.fillCoverage->bounds
                   && samePackedIdentity(left.fillCoverage->packedMask,
                       right.fillCoverage->packedMask)))
           && samePixelSelectionIdentity(
               left.pixelSelectionOp, right.pixelSelectionOp)
           && left.reframeOp == right.reframeOp
           && left.imageOp == right.imageOp;
}

QString backingIdentity(const void *data, qsizetype size)
{
    if (!data || size <= 0)
    {
        return {};
    }
    return QStringLiteral("%1:%2")
        .arg(reinterpret_cast<quintptr>(data), 0, 16)
        .arg(size);
}

void rememberImmutableImage(ImmutableBackings &backings, const QImage &image)
{
    if (!image.isNull())
    {
        backings.images.insert(image.cacheKey(), image);
    }
}

void rememberImmutableBytes(
    ImmutableBackings &backings, const QByteArray &bytes)
{
    const QString identity = backingIdentity(bytes.constData(), bytes.size());
    if (!identity.isEmpty())
    {
        backings.byteArrays.insert(identity, bytes);
    }
}

void rememberImmutablePoints(
    ImmutableBackings &backings, const QVector<StrokePoint> &points)
{
    const QString identity = backingIdentity(points.constData(), points.size());
    if (!identity.isEmpty())
    {
        backings.pointVectors.insert(identity, points);
    }
}

ImmutableBackings immutableBackingsFromPlan(const PreparedPlan *base)
{
    ImmutableBackings backings;
    if (!base)
    {
        return backings;
    }
    for (const ClipAssetMeta &asset : base->clipAssets)
    {
        rememberImmutableImage(backings, asset.image);
    }
    for (const BinaryAssetMeta &asset : base->binaryAssets)
    {
        rememberImmutableBytes(backings, asset.region.packedMask);
    }
    for (const RasterAsset &asset : base->rasterAssets)
    {
        rememberImmutableBytes(backings, asset.compressedRgba);
    }
    for (const StrokeMeta &stroke : base->strokes)
    {
        rememberImmutablePoints(backings, stroke.snapshot.points);
        rememberImmutableImage(backings, stroke.snapshot.clipMask);
        rememberImmutableImage(backings, stroke.snapshot.fillMask);
        if (stroke.snapshot.fillCoverage)
        {
            rememberImmutableBytes(
                backings, stroke.snapshot.fillCoverage->packedMask);
        }
        if (stroke.snapshot.pixelSelectionOp)
        {
            rememberImmutableBytes(
                backings, stroke.snapshot.pixelSelectionOp->packedMask);
        }
    }
    return backings;
}

bool sameStrokeVectorBacking(
    const QVector<Stroke> &left, const QVector<Stroke> &right)
{
    // A non-const QVector access detaches before exposing writable storage.
    // Equality here therefore proves that the candidate still references the
    // immutable backing owned by baseDocument. Never broaden this to an
    // element-wise comparison: retained writable aliases must take the full
    // freezer/validator path.
    return left.size() == right.size()
           && (left.isEmpty() || left.constData() == right.constData());
}

QString owningStringCopy(const QString &source)
{
    if (source.isNull())
    {
        return {};
    }
    return QString(source.constData(), source.size());
}

MetadataReuseResult reusePreparedContentForMetadataEdit(const Document &source,
    const Document &baseDocument,
    const PreparedPlan &basePlan,
    qint64 maximumBytes,
    QString *error)
{
    MetadataReuseResult result;
    if (source.size != baseDocument.size
        || source.rasterAssets != baseDocument.rasterAssets
        || source.layers.size() != baseDocument.layers.size())
    {
        return result;
    }

    QHash<QUuid, const Layer *> baseLayers;
    baseLayers.reserve(baseDocument.layers.size());
    for (const Layer &layer : baseDocument.layers)
    {
        baseLayers.insert(layer.id, &layer);
    }
    QSet<QUuid> matchedLayers;
    matchedLayers.reserve(source.layers.size());
    for (const Layer &layer : source.layers)
    {
        const auto base = baseLayers.constFind(layer.id);
        if (base == baseLayers.cend() || matchedLayers.contains(layer.id)
            || layer.initialCanvasSize != (*base)->initialCanvasSize
            || !sameStrokeVectorBacking(layer.strokes, (*base)->strokes))
        {
            return result;
        }
        matchedLayers.insert(layer.id);
    }

    if (!source.background.isValid())
    {
        setError(
            error, DocumentSerializer::tr("The canvas background is invalid."));
        result.status = MetadataReuseStatus::Invalid;
        return result;
    }
    if (source.animationFrames < DocumentLimits::minimumAnimationFrames
        || source.animationFrames > DocumentLimits::maximumAnimationFrames
        || !std::isfinite(source.framesPerSecond)
        || source.framesPerSecond < DocumentLimits::minimumFramesPerSecond
        || source.framesPerSecond > DocumentLimits::maximumFramesPerSecond
        || !std::isfinite(source.wobbleAmount)
        || source.wobbleAmount < DocumentLimits::minimumWobbleAmount
        || source.wobbleAmount > DocumentLimits::maximumWobbleAmount
        || !isValidMotionSettings(source.motion, source.animationFrames))
    {
        setError(error,
            DocumentSerializer::tr("The animation settings are invalid."));
        result.status = MetadataReuseStatus::Invalid;
        return result;
    }

    bool activeLayerFound = false;
    for (const Layer &layer : source.layers)
    {
        activeLayerFound = activeLayerFound
                           || (layer.id == source.activeLayerId
                               && layer.kind == LayerKind::Paint);
        if (layer.name.trimmed().isEmpty()
            || layer.name.size() > DocumentLimits::maximumLayerNameLength
            || !std::isfinite(layer.opacity) || layer.opacity < 0.0
            || layer.opacity > 1.0 || !isValidLayerBlendMode(layer.blendMode)
            || !isValidLayerKind(layer.kind)
            || (layer.wobbleAmount
                && (!std::isfinite(*layer.wobbleAmount)
                    || *layer.wobbleAmount < DocumentLimits::minimumWobbleAmount
                    || *layer.wobbleAmount
                           > DocumentLimits::maximumWobbleAmount))
            || (layer.motion
                && !isValidMotionSettings(
                    *layer.motion, source.animationFrames))
            || layer.wobbleAmount.has_value() != layer.motion.has_value()
            || (layer.kind == LayerKind::Group
                && (!layer.strokes.isEmpty() || layer.clipToLayerBelow
                    || layer.reference || layer.wobbleAmount || layer.motion)))
        {
            setError(error,
                DocumentSerializer::tr("A layer contains invalid data."));
            result.status = MetadataReuseStatus::Invalid;
            return result;
        }
    }
    if (!analyzeLayerHierarchy(source).isValid())
    {
        setError(
            error, DocumentSerializer::tr("The layer hierarchy is invalid."));
        result.status = MetadataReuseStatus::Invalid;
        return result;
    }
    const bool validActiveLayer =
        !std::any_of(source.layers.cbegin(),
            source.layers.cend(),
            [](const Layer &layer)
            {
                return layer.kind == LayerKind::Paint;
            })
            ? source.activeLayerId.isNull()
            : !source.activeLayerId.isNull() && activeLayerFound;
    if (!validActiveLayer)
    {
        setError(
            error, DocumentSerializer::tr("The active layer ID is invalid."));
        result.status = MetadataReuseStatus::Invalid;
        return result;
    }

    qint64 previousMetadataBytes = 0;
    qint64 nextMetadataBytes = 0;
    const auto addMetadataSize = [](qint64 &total, qint64 bytes)
    {
        return addSerializedBytes(
            total, bytes, std::numeric_limits<qint64>::max());
    };
    const qint64 previousRootBytes =
        QJsonDocument(rootToJson(baseDocument, {}, {}, {}))
            .toJson(QJsonDocument::Compact)
            .size();
    const qint64 nextRootBytes = QJsonDocument(rootToJson(source, {}, {}, {}))
                                     .toJson(QJsonDocument::Compact)
                                     .size();
    if (!addMetadataSize(previousMetadataBytes, previousRootBytes)
        || !addMetadataSize(nextMetadataBytes, nextRootBytes))
    {
        setError(
            error, DocumentSerializer::tr("The project is too large to save."));
        result.status = MetadataReuseStatus::TooLarge;
        return result;
    }
    for (const Layer &layer : source.layers)
    {
        const Layer &baseLayer = **baseLayers.constFind(layer.id);
        const qint64 previousLayerBytes =
            QJsonDocument(layerSkeletonToJson(baseLayer))
                .toJson(QJsonDocument::Compact)
                .size();
        const qint64 nextLayerBytes = QJsonDocument(layerSkeletonToJson(layer))
                                          .toJson(QJsonDocument::Compact)
                                          .size();
        if (!addMetadataSize(previousMetadataBytes, previousLayerBytes)
            || !addMetadataSize(nextMetadataBytes, nextLayerBytes))
        {
            setError(error,
                DocumentSerializer::tr("The project is too large to save."));
            result.status = MetadataReuseStatus::TooLarge;
            return result;
        }
    }
    qint64 compactSize = basePlan.compactSize;
    // Layer and asset topology is byte-for-byte unchanged. Replacing only
    // the root/layer skeleton contribution keeps the prepared size exact
    // without rebuilding the per-stroke plan.
    if (nextMetadataBytes >= previousMetadataBytes)
    {
        const qint64 increase = nextMetadataBytes - previousMetadataBytes;
        if (increase > maximumBytes || compactSize > maximumBytes - increase)
        {
            setError(error,
                DocumentSerializer::tr("The project is too large to save."));
            result.status = MetadataReuseStatus::TooLarge;
            return result;
        }
        compactSize += increase;
    }
    else
    {
        const qint64 decrease = previousMetadataBytes - nextMetadataBytes;
        if (compactSize < decrease)
        {
            setError(error,
                DocumentSerializer::tr("The project contains invalid "
                                       "operations or too much mask data."));
            result.status = MetadataReuseStatus::Invalid;
            return result;
        }
        compactSize -= decrease;
        if (compactSize > maximumBytes)
        {
            setError(error,
                DocumentSerializer::tr("The project is too large to save."));
            result.status = MetadataReuseStatus::TooLarge;
            return result;
        }
    }

    Document frozen;
    frozen.size = source.size;
    frozen.background = source.background;
    frozen.animationFrames = source.animationFrames;
    frozen.framesPerSecond = source.framesPerSecond;
    frozen.wobbleAmount = source.wobbleAmount;
    frozen.motion = source.motion;
    frozen.rasterAssets = baseDocument.rasterAssets;
    frozen.activeLayerId = source.activeLayerId;
    if (source.layers.constData() == baseDocument.layers.constData())
    {
        frozen.layers = baseDocument.layers;
    }
    else
    {
        frozen.layers.reserve(source.layers.size());
        for (const Layer &layer : source.layers)
        {
            const Layer &baseLayer = **baseLayers.constFind(layer.id);
            Layer frozenLayer;
            frozenLayer.id = layer.id;
            frozenLayer.name = owningStringCopy(layer.name);
            frozenLayer.kind = layer.kind;
            frozenLayer.parentGroupId = layer.parentGroupId;
            frozenLayer.clipToLayerBelow = layer.clipToLayerBelow;
            frozenLayer.visible = layer.visible;
            frozenLayer.reference = layer.reference;
            frozenLayer.opacity = layer.opacity;
            frozenLayer.blendMode = layer.blendMode;
            frozenLayer.wobbleAmount = layer.wobbleAmount;
            frozenLayer.motion = layer.motion;
            frozenLayer.initialCanvasSize = layer.initialCanvasSize;
            frozenLayer.strokes = baseLayer.strokes;
            frozen.layers.append(std::move(frozenLayer));
        }
    }

    result.document = std::move(frozen);
    result.plan = basePlan;
    result.plan.compactSize = compactSize;
    result.status = MetadataReuseStatus::Success;
    return result;
}

bool isValidIncrementalStroke(const Stroke &stroke, const QSize &canvasSize)
{
    const QRect canvasRect(QPoint(), canvasSize);
    if ((stroke.mode != StrokeMode::Paint && stroke.mode != StrokeMode::Erase
            && stroke.mode != StrokeMode::Fill)
        || stroke.id.isNull() || !stroke.color.isValid()
        || !std::isfinite(stroke.width)
        || stroke.width < DocumentLimits::minimumStrokeWidth
        || stroke.width > DocumentLimits::maximumStrokeWidth
        || !isValidBrushSettings(stroke.brush) || stroke.points.isEmpty()
        || stroke.points.size() > DocumentLimits::maximumPointsPerStroke
        || stroke.pixelSelectionOp || stroke.reframeOp
        || (stroke.visibilityClip
            && (stroke.visibilityClip->isEmpty()
                || !canvasRect.contains(*stroke.visibilityClip)))
        || (!stroke.clipMask.isNull()
            && (stroke.clipMask.size() != canvasSize
                || stroke.clipMask.format() != QImage::Format_Grayscale8))
        || (!stroke.fillMask.isNull()
            && (stroke.mode != StrokeMode::Fill
                || stroke.fillMask.size() != canvasSize
                || stroke.fillMask.format() != QImage::Format_Grayscale8))
        || stroke.fillCoverage)
    {
        return false;
    }
    return std::all_of(stroke.points.cbegin(),
        stroke.points.cend(),
        [](const StrokePoint &point)
        {
            return std::isfinite(point.position.x())
                   && std::isfinite(point.position.y())
                   && std::abs(point.position.x())
                          <= DocumentLimits::maximumStoredCoordinateMagnitude
                   && std::abs(point.position.y())
                          <= DocumentLimits::maximumStoredCoordinateMagnitude
                   && std::isfinite(point.pressure) && point.pressure >= 0.0
                   && point.pressure <= 1.0;
        });
}

QVector<StrokePoint> freezeIncrementalPoints(const QVector<StrokePoint> &source)
{
    QVector<StrokePoint> frozen;
    frozen.reserve(source.size());
    for (const StrokePoint &point : source)
    {
        frozen.append(point);
    }
    return frozen;
}

Stroke freezeIncrementalStroke(const Stroke &source)
{
    Stroke frozen;
    frozen.id = source.id;
    frozen.seed = source.seed;
    frozen.mode = source.mode;
    frozen.color = source.color;
    frozen.width = source.width;
    frozen.brush = source.brush;
    frozen.points = freezeIncrementalPoints(source.points);
    frozen.visibilityClip = source.visibilityClip;
    return frozen;
}

QString clipMaskId(const QImage &mask, const ClipMaskTable &table)
{
    if (mask.isNull())
    {
        return {};
    }
    return table.idByCacheKey.value(mask.cacheKey());
}

QString binaryMaskId(const Stroke &stroke, const BinaryMaskTable &table)
{
    const std::optional<PackedMaskRegion> region =
        strokeBinaryMaskRegion(stroke);
    if (!region)
    {
        return {};
    }
    return table.idByIdentity.value(binaryMaskIdentity(*region));
}

}

}

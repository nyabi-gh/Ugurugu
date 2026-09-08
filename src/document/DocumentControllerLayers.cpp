// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "document/DocumentBudget.hpp"
#include "document/DocumentController.hpp"
#include "document/DocumentLimits.hpp"
#include "document/LayerHierarchy.hpp"
#include "document/SelectionOperation.hpp"
#include "document/history/HistoryEffects.hpp"
#include "io/serializer/RasterAssetTable.hpp"
#include "render/RenderEngine.hpp"

#include <QFileInfo>

#include <algorithm>
#include <cmath>
#include <memory>
#include <utility>

namespace ugurugu
{

using DocumentBudget::distinctClipMaskBytes;
using DocumentBudget::totalPointCount;
using DocumentBudget::totalStrokeCount;
using history::layerOpacityMergeId;
using history::layerWobbleMergeId;

namespace
{

qsizetype layerPointCount(const Layer &layer)
{
    qsizetype count = 0;
    for (const Stroke &stroke : layer.strokes)
    {
        if (stroke.points.size() > DocumentLimits::maximumTotalPoints - count)
        {
            return DocumentLimits::maximumTotalPoints + 1;
        }
        count += stroke.points.size();
    }
    return count;
}

QSize finalCanvasSize(const Layer &layer)
{
    QSize size = layer.initialCanvasSize;
    for (const Stroke &stroke : layer.strokes)
    {
        if (stroke.reframeOp)
        {
            size = stroke.reframeOp->targetSize;
        }
    }
    return size;
}

// A merge wraps the upper layer's strokes in a composite-boundary pair, so
// every stroke keeps exactly the reach it had while the layers were separate.
// The one thing sealing changes is a layer's tail — the strokes after its
// last boundary, which erased and moved pixels on the flattened composite.
// If such a stroke already reached artwork in an earlier section, sealing
// would take that reach away and silently change the picture, so that merge
// stays refused. Bounds carry the same wobble margin the renderer uses,
// which keeps the answer conservative for every frame.
bool sealedTailClearOfEarlierSections(
    const Document &document, const Layer &layer)
{
    int lastBoundary = -1;
    for (int index = 0; index < layer.strokes.size(); ++index)
    {
        if (layer.strokes[index].mode == StrokeMode::CompositeBoundary)
        {
            lastBoundary = index;
        }
    }
    if (lastBoundary < 0)
    {
        return true;
    }
    const RenderEngine::StrokeCoveragePlan plan =
        RenderEngine::prepareStrokeCoverage(document, layer);
    if (!plan.valid || plan.primitiveBounds.size() != layer.strokes.size())
    {
        return false;
    }
    QRect painted;
    for (int index = 0; index < lastBoundary; ++index)
    {
        if (layer.strokes[index].mode != StrokeMode::Erase)
        {
            painted = painted.united(plan.primitiveBounds[index]);
        }
    }
    for (int index = lastBoundary + 1; index < layer.strokes.size(); ++index)
    {
        const Stroke &stroke = layer.strokes[index];
        if (stroke.mode == StrokeMode::Erase
            && plan.primitiveBounds[index].intersects(painted))
        {
            return false;
        }
        if (stroke.mode == StrokeMode::PixelSelection
            && (!stroke.pixelSelectionOp
                || stroke.pixelSelectionOp->sourceBounds.intersects(painted)))
        {
            return false;
        }
    }
    return true;
}

bool imageTransformWithinStoredBounds(
    const QTransform &transform, const QSize &sourceSize)
{
    const QRectF bounds = transform.mapRect(QRectF(QPointF(), sourceSize));
    const qreal limit = DocumentLimits::maximumStoredCoordinateMagnitude;
    return std::isfinite(bounds.left()) && std::isfinite(bounds.top())
           && std::isfinite(bounds.right()) && std::isfinite(bounds.bottom())
           && bounds.left() >= -limit && bounds.top() >= -limit
           && bounds.right() <= limit && bounds.bottom() <= limit;
}
}

void DocumentController::addLayer(const QUuid &parentGroupId)
{
    const Document &current = document();
    const Layer *requestedGroup = current.layer(parentGroupId);
    if (current.layers.size() >= DocumentLimits::maximumLayers
        || (!parentGroupId.isNull()
            && (!requestedGroup || requestedGroup->kind != LayerKind::Group)))
    {
        failHistoryMacro();
        return;
    }
    Layer layer;
    layer.name = nextLayerName();
    layer.initialCanvasSize = current.size;
    if (requestedGroup)
    {
        layer.parentGroupId = requestedGroup->id;
    }
    else if (const Layer *active = current.layer(current.activeLayerId))
    {
        layer.parentGroupId = active->parentGroupId;
    }
    const QUuid layerId = layer.id;
    Document candidate = current;
    const int activeIndex = current.layerIndex(current.activeLayerId);
    if (!requestedGroup && activeIndex >= 0)
    {
        candidate.layers.insert(activeIndex + 1, std::move(layer));
    }
    else
    {
        candidate.layers.append(std::move(layer));
    }
    candidate.activeLayerId = layerId;
    auto effects = std::make_shared<HistoryEffects>();
    effects->afterDocumentChanged.append(
        HistoryEffects::LayerThumbnail{layerId});
    effects->afterDocumentChanged.append(HistoryEffects::ActiveLayer{});
    tryCommitCandidate(tr("Add layer"),
        std::move(candidate),
        std::move(effects),
        ActiveLayerPolicy::UsePrepared);
}

void DocumentController::addLayerGroup(const QUuid &childId)
{
    const Document &current = document();
    if (current.layers.size() >= DocumentLimits::maximumLayers)
    {
        failHistoryMacro();
        return;
    }
    const Layer *child = current.layer(childId);
    Layer group;
    group.kind = LayerKind::Group;
    group.initialCanvasSize = current.size;
    group.parentGroupId = child ? child->parentGroupId : QUuid();
    int number = 1;
    do
    {
        group.name = tr("Group %1").arg(number++);
    } while (std::any_of(current.layers.cbegin(),
        current.layers.cend(),
        [&group](const Layer &layer)
        {
            return layer.name == group.name;
        }));

    Document candidate = current;
    const int childIndex = current.layerIndex(childId);
    if (childIndex >= 0)
    {
        candidate.layers.insert(childIndex + 1, group);
        candidate.layer(childId)->parentGroupId = group.id;
    }
    else
    {
        candidate.layers.append(group);
    }
    auto effects = std::make_shared<HistoryEffects>();
    effects->afterDocumentChanged.append(
        HistoryEffects::LayerThumbnailsReset{});
    tryCommitCandidate(
        tr("Add layer group"), std::move(candidate), std::move(effects));
}

DocumentController::InsertImageResult DocumentController::insertImage(
    const QImage &image, const QString &sourceFileName)
{
    const auto reject = [this](InsertImageResult result)
    {
        failHistoryMacro();
        return result;
    };
    const Document &current = document();
    if (current.layers.size() >= DocumentLimits::maximumLayers)
    {
        return reject(InsertImageResult::RejectedLayerLimit);
    }

    serializer_detail::RasterAssetRegistrationStatus registrationStatus;
    std::optional<RasterAsset> asset =
        serializer_detail::rasterAssetFromImage(image, &registrationStatus);
    if (!asset)
    {
        using Status = serializer_detail::RasterAssetRegistrationStatus;
        return reject(registrationStatus == Status::Invalid
                              || registrationStatus == Status::PixelLimit
                          ? InsertImageResult::RejectedInvalidImage
                          : InsertImageResult::RejectedAssetLimit);
    }
    if (!current.rasterAssets.contains(asset->id))
    {
        quint64 decodedBytes = 0;
        qint64 payloadBytes = 0;
        for (const RasterAsset &currentAsset : current.rasterAssets)
        {
            const std::optional<quint64> decoded =
                serializer_detail::rasterDecodedByteCount(currentAsset.size);
            if (!decoded
                || *decoded > DocumentLimits::maximumDistinctRasterDecodedBytes
                                  - decodedBytes
                || currentAsset.compressedRgba.size()
                       > DocumentLimits::maximumDistinctRasterPayloadBytes
                             - payloadBytes)
            {
                return reject(InsertImageResult::RejectedAssetLimit);
            }
            decodedBytes += *decoded;
            payloadBytes += currentAsset.compressedRgba.size();
        }
        const std::optional<quint64> addedDecoded =
            serializer_detail::rasterDecodedByteCount(asset->size);
        if (!addedDecoded
            || *addedDecoded > DocumentLimits::maximumDistinctRasterDecodedBytes
                                   - decodedBytes
            || asset->compressedRgba.size()
                   > DocumentLimits::maximumDistinctRasterPayloadBytes
                         - payloadBytes)
        {
            return reject(InsertImageResult::RejectedAssetLimit);
        }
    }

    const qreal scale = std::min({1.0,
        qreal(current.size.width()) / asset->size.width(),
        qreal(current.size.height()) / asset->size.height()});
    const qreal translatedX =
        (current.size.width() - asset->size.width() * scale) / 2.0;
    const qreal translatedY =
        (current.size.height() - asset->size.height() * scale) / 2.0;
    const QTransform transform(
        scale, 0.0, 0.0, scale, translatedX, translatedY);
    if (!imageTransformWithinStoredBounds(transform, asset->size))
    {
        return reject(InsertImageResult::RejectedInvalidImage);
    }

    QString layerName = QFileInfo(sourceFileName).completeBaseName().trimmed();
    layerName = layerName.left(DocumentLimits::maximumLayerNameLength);
    if (layerName.isEmpty())
    {
        layerName = nextLayerName();
    }

    Stroke operation;
    operation.mode = StrokeMode::Image;
    operation.points.clear();
    operation.imageOp = ImageOp{asset->id, transform, SamplingMode::Smooth};

    Layer layer;
    layer.name = std::move(layerName);
    layer.initialCanvasSize = current.size;
    layer.strokes.append(std::move(operation));
    if (const Layer *active = current.layer(current.activeLayerId))
    {
        layer.parentGroupId = active->parentGroupId;
    }
    const QUuid layerId = layer.id;

    Document candidate = current;
    candidate.rasterAssets.insert(asset->id, *asset);
    const int activeIndex = current.layerIndex(current.activeLayerId);
    if (activeIndex >= 0)
    {
        candidate.layers.insert(activeIndex + 1, std::move(layer));
    }
    else
    {
        candidate.layers.append(std::move(layer));
    }
    candidate.activeLayerId = layerId;

    auto effects = std::make_shared<HistoryEffects>();
    effects->afterDocumentChanged.append(
        HistoryEffects::LayerThumbnail{layerId});
    effects->afterDocumentChanged.append(HistoryEffects::ActiveLayer{});
    if (!tryCommitCandidate(tr("Insert image"),
            std::move(candidate),
            std::move(effects),
            ActiveLayerPolicy::UsePrepared))
    {
        return InsertImageResult::RejectedCommit;
    }
    return InsertImageResult::Inserted;
}

bool DocumentController::setImageTransform(const QUuid &layerId,
    const QUuid &strokeId,
    const QTransform &transform,
    SamplingMode sampling)
{
    const Document &current = document();
    const Layer *layer = current.layer(layerId);
    if (!layer || layer->kind != LayerKind::Paint)
    {
        return rejectHistoryMutation();
    }
    const auto stroke = std::find_if(layer->strokes.cbegin(),
        layer->strokes.cend(),
        [&strokeId](const Stroke &candidate)
        {
            return candidate.id == strokeId;
        });
    if (stroke == layer->strokes.cend() || stroke->mode != StrokeMode::Image
        || !stroke->imageOp)
    {
        return rejectHistoryMutation();
    }
    const auto asset = current.rasterAssets.constFind(stroke->imageOp->assetId);
    const ImageOp replacement{stroke->imageOp->assetId, transform, sampling};
    if (asset == current.rasterAssets.cend() || !isValidImageOp(replacement)
        || !imageTransformWithinStoredBounds(transform, asset->size)
        || replacement == *stroke->imageOp)
    {
        return rejectHistoryMutation();
    }

    Document candidate = current;
    Layer *targetLayer = candidate.layer(layerId);
    auto targetStroke = std::find_if(targetLayer->strokes.begin(),
        targetLayer->strokes.end(),
        [&strokeId](const Stroke &candidateStroke)
        {
            return candidateStroke.id == strokeId;
        });
    targetStroke->imageOp = replacement;
    auto effects = std::make_shared<HistoryEffects>();
    effects->afterDocumentChanged.append(
        HistoryEffects::LayerThumbnail{layerId});
    return tryCommitCandidate(
        tr("Transform image"), std::move(candidate), std::move(effects));
}

void DocumentController::duplicateLayer(const QUuid &id)
{
    const Document &current = document();
    const int sourceIndex = current.layerIndex(id);
    if (sourceIndex < 0 || current.layers[sourceIndex].kind != LayerKind::Paint
        || current.layers.size() >= DocumentLimits::maximumLayers)
    {
        failHistoryMacro();
        return;
    }
    const qsizetype sourcePointCount =
        layerPointCount(current.layers[sourceIndex]);
    const qsizetype existingPointCount = totalPointCount(current);
    const qsizetype sourceStrokeCount =
        current.layers[sourceIndex].strokes.size();
    const qsizetype existingStrokeCount = totalStrokeCount(current);
    if (sourcePointCount > DocumentLimits::maximumTotalPoints
        || existingPointCount > DocumentLimits::maximumTotalPoints
        || sourcePointCount
               > DocumentLimits::maximumTotalPoints - existingPointCount
        || sourceStrokeCount > DocumentLimits::maximumTotalStrokes
        || existingStrokeCount > DocumentLimits::maximumTotalStrokes
        || sourceStrokeCount
               > DocumentLimits::maximumTotalStrokes - existingStrokeCount)
    {
        failHistoryMacro();
        return;
    }
    Layer copy = current.layers[sourceIndex];
    copy.id = QUuid::createUuid();
    copy.name = tr("%1 copy").arg(copy.name);
    if (copy.name.size() > DocumentLimits::maximumLayerNameLength)
    {
        copy.name.truncate(DocumentLimits::maximumLayerNameLength);
    }
    for (Stroke &stroke : copy.strokes)
    {
        stroke.id = QUuid::createUuid();
    }
    Document withCopy = current;
    withCopy.layers.insert(sourceIndex + 1, copy);
    withCopy.activeLayerId = copy.id;
    if (distinctClipMaskBytes(withCopy)
        > DocumentLimits::maximumDistinctClipMaskBytes)
    {
        failHistoryMacro();
        return;
    }
    auto effects = std::make_shared<HistoryEffects>();
    effects->afterDocumentChanged.append(
        HistoryEffects::LayerThumbnailsReset{});
    effects->afterDocumentChanged.append(HistoryEffects::ActiveLayer{});
    tryCommitCandidate(tr("Duplicate layer"),
        std::move(withCopy),
        std::move(effects),
        ActiveLayerPolicy::UsePrepared);
}

DocumentController::MergeLayerDownStatus
DocumentController::mergeLayerDownStatus(const QUuid &id) const
{
    const Document &current = document();
    const int sourceIndex = current.layerIndex(id);
    const Layer *source = current.layer(id);
    if (sourceIndex < 0 || !source || source->kind != LayerKind::Paint)
    {
        return MergeLayerDownStatus::MissingLayer;
    }
    int targetIndex = -1;
    for (int index = sourceIndex - 1; index >= 0; --index)
    {
        if (current.layers[index].parentGroupId == source->parentGroupId)
        {
            targetIndex = index;
            break;
        }
    }
    if (targetIndex < 0 || current.layers[targetIndex].kind != LayerKind::Paint)
    {
        return MergeLayerDownStatus::NoPaintLayerBelow;
    }
    const Layer &target = current.layers[targetIndex];
    const bool supportedProperties =
        source->blendMode == LayerBlendMode::Normal
        && target.blendMode == LayerBlendMode::Normal
        && qFuzzyCompare(source->opacity, 1.0)
        && qFuzzyCompare(target.opacity, 1.0) && !source->clipToLayerBelow
        && !target.clipToLayerBelow && source->reference == target.reference
        && source->visible == target.visible
        && qFuzzyCompare(effectiveWobbleAmount(current, *source),
            effectiveWobbleAmount(current, target))
        && effectiveMotion(current, *source)
               == effectiveMotion(current, target);
    if (!supportedProperties)
    {
        return MergeLayerDownStatus::UnsupportedProperties;
    }
    if (finalCanvasSize(target) != source->initialCanvasSize)
    {
        return MergeLayerDownStatus::IncompatibleCanvasEpoch;
    }
    // A reframe spans the whole layer, so merging would re-frame the lower
    // layer's strokes along with the upper's. Erasers and pixel moves merge
    // freely: the boundary pair keeps them inside their own section.
    const bool sourceReframes = std::any_of(source->strokes.cbegin(),
        source->strokes.cend(),
        [](const Stroke &stroke)
        {
            return stroke.mode == StrokeMode::Reframe;
        });
    if (sourceReframes || !sealedTailClearOfEarlierSections(current, *source)
        || !sealedTailClearOfEarlierSections(current, target))
    {
        return MergeLayerDownStatus::UnsupportedStrokes;
    }
    if (source->strokes.size()
            > DocumentLimits::maximumStrokesPerLayer - target.strokes.size() - 2
        || totalStrokeCount(current) > DocumentLimits::maximumTotalStrokes - 2)
    {
        return MergeLayerDownStatus::StrokeLimit;
    }
    return MergeLayerDownStatus::Available;
}

bool DocumentController::mergeLayerDown(const QUuid &id)
{
    if (mergeLayerDownStatus(id) != MergeLayerDownStatus::Available)
    {
        failHistoryMacro();
        return false;
    }
    const Document &current = document();
    const int sourceIndex = current.layerIndex(id);
    const Layer &source = current.layers[sourceIndex];
    int targetIndex = sourceIndex - 1;
    while (current.layers[targetIndex].parentGroupId != source.parentGroupId)
    {
        --targetIndex;
    }
    const QUuid targetId = current.layers[targetIndex].id;
    Document candidate = current;
    Layer &target = candidate.layers[targetIndex];
    // The pair seals the upper layer's strokes into their own composited
    // section: its erasers keep acting only on what they erased before the
    // merge, while strokes added after the merge land behind the trailing
    // boundary and reach everything already on the layer.
    Stroke enterSourceSection;
    enterSourceSection.mode = StrokeMode::CompositeBoundary;
    Stroke sealSourceSection;
    sealSourceSection.mode = StrokeMode::CompositeBoundary;
    target.strokes.reserve(target.strokes.size() + source.strokes.size() + 2);
    target.strokes.append(std::move(enterSourceSection));
    target.strokes.append(source.strokes);
    target.strokes.append(std::move(sealSourceSection));
    candidate.layers.removeAt(sourceIndex);
    candidate.activeLayerId = targetId;
    auto effects = std::make_shared<HistoryEffects>();
    effects->afterDocumentChanged.append(
        HistoryEffects::LayerThumbnailsReset{});
    effects->afterDocumentChanged.append(HistoryEffects::ActiveLayer{});
    return tryCommitCandidate(tr("Merge layer down"),
        std::move(candidate),
        std::move(effects),
        ActiveLayerPolicy::UsePrepared);
}

DocumentController::PasteLayerResult DocumentController::pasteLayer(Layer layer,
    const QSize &sourceCanvasSize,
    const QPointF &contentDelta,
    const QImage &selectionMask,
    const QMap<QString, RasterAsset> &rasterAssets)
{
    const auto reject = [this](PasteLayerResult result)
    {
        failHistoryMacro();
        return result;
    };
    const Document &current = document();
    if (layer.kind != LayerKind::Paint || layer.strokes.isEmpty()
        || !sourceCanvasSize.isValid())
    {
        return reject(PasteLayerResult::RejectedInvalidLayer);
    }
    const bool followsSelection = !selectionMask.isNull();
    if (followsSelection
        && (selectionMask.size() != current.size
            || selectionMask.format() != QImage::Format_Grayscale8
            || sourceCanvasSize != current.size))
    {
        return reject(PasteLayerResult::RejectedInvalidLayer);
    }
    if (current.layers.size() >= DocumentLimits::maximumLayers)
    {
        return reject(PasteLayerResult::RejectedLayerLimit);
    }
    const bool needsReframe = sourceCanvasSize != current.size;
    const bool shiftsContent =
        followsSelection
        && !(qFuzzyIsNull(contentDelta.x()) && qFuzzyIsNull(contentDelta.y()));
    const qsizetype pastedStrokeCount =
        layer.strokes.size() + (needsReframe ? 1 : 0) + (shiftsContent ? 1 : 0);
    const qsizetype existingStrokeCount = totalStrokeCount(current);
    if (pastedStrokeCount > DocumentLimits::maximumStrokesPerLayer
        || existingStrokeCount > DocumentLimits::maximumTotalStrokes
        || pastedStrokeCount
               > DocumentLimits::maximumTotalStrokes - existingStrokeCount)
    {
        return reject(PasteLayerResult::RejectedStrokeLimit);
    }
    const qsizetype pastedPointCount = layerPointCount(layer);
    const qsizetype existingPointCount = totalPointCount(current);
    if (pastedPointCount > DocumentLimits::maximumTotalPoints
        || existingPointCount > DocumentLimits::maximumTotalPoints
        || pastedPointCount
               > DocumentLimits::maximumTotalPoints - existingPointCount)
    {
        return reject(PasteLayerResult::RejectedPointLimit);
    }
    QMap<QString, RasterAsset> importedAssets;
    serializer_detail::RasterAssetTable importedAssetTable;
    for (const Stroke &stroke : std::as_const(layer.strokes))
    {
        if (!stroke.imageOp || importedAssets.contains(stroke.imageOp->assetId))
        {
            continue;
        }
        const QString &assetId = stroke.imageOp->assetId;
        const auto provided = rasterAssets.constFind(assetId);
        const auto existing = current.rasterAssets.constFind(assetId);
        if (existing != current.rasterAssets.cend())
        {
            if (provided != rasterAssets.cend() && *provided != *existing)
            {
                return reject(PasteLayerResult::RejectedInvalidLayer);
            }
            importedAssets.insert(assetId, *existing);
            continue;
        }
        if (provided == rasterAssets.cend() || provided.key() != provided->id)
        {
            return reject(PasteLayerResult::RejectedInvalidLayer);
        }
        const serializer_detail::RasterAssetRegistrationResult registration =
            importedAssetTable.registerPayload(
                provided->id, provided->size, provided->compressedRgba);
        if (registration.status
            != serializer_detail::RasterAssetRegistrationStatus::Registered)
        {
            return reject(PasteLayerResult::RejectedInvalidLayer);
        }
        importedAssets.insert(assetId, *provided);
    }
    QImage nextSelectionMask;
    if (shiftsContent)
    {
        QTransform shift;
        shift.translate(contentDelta.x(), contentDelta.y());
        const std::optional<PixelSelectionOp> moveOperation =
            makePixelSelectionOp(selectionMask,
                shift,
                /*clearSource=*/true,
                /*drawDestination=*/true);
        if (!moveOperation)
        {
            return reject(PasteLayerResult::RejectedInvalidLayer);
        }
        Stroke move;
        move.mode = StrokeMode::PixelSelection;
        move.points.clear();
        move.pixelSelectionOp = *moveOperation;
        layer.strokes.append(std::move(move));
        nextSelectionMask = transformedSelectionSupport(
            selectionMask, current.size, shift, moveOperation->sampling);
        if (nextSelectionMask.isNull())
        {
            return reject(PasteLayerResult::RejectedInvalidLayer);
        }
    }
    else if (followsSelection)
    {
        nextSelectionMask = selectionMask;
    }
    if (needsReframe)
    {
        // The pasted strokes stay in source-document coordinates; a trailing
        // canvas reframe moves the layer's framebuffer epoch to this
        // document's size, the same way resizeCanvas migrates layers.
        Stroke reframe;
        reframe.mode = StrokeMode::Reframe;
        reframe.reframeOp = ReframeOp{ReframeMode::Canvas,
            SamplingMode::Nearest,
            sourceCanvasSize,
            current.size,
            QPoint()};
        reframe.points.clear();
        layer.strokes.append(std::move(reframe));
    }
    layer.id = QUuid::createUuid();
    for (Stroke &stroke : layer.strokes)
    {
        stroke.id = QUuid::createUuid();
    }
    layer.name = nextLayerName();
    layer.visible = true;
    layer.reference = false;
    layer.clipToLayerBelow = false;
    if (!layer.initialCanvasSize.isValid())
    {
        layer.initialCanvasSize = sourceCanvasSize;
    }
    const Layer *active = current.layer(current.activeLayerId);
    layer.parentGroupId = active ? active->parentGroupId : QUuid();
    const QUuid layerId = layer.id;
    Document candidate = current;
    for (auto asset = importedAssets.cbegin(); asset != importedAssets.cend();
        ++asset)
    {
        candidate.rasterAssets.insert(asset.key(), *asset);
    }
    const int activeIndex = current.layerIndex(current.activeLayerId);
    if (activeIndex >= 0)
    {
        candidate.layers.insert(activeIndex + 1, std::move(layer));
    }
    else
    {
        candidate.layers.append(std::move(layer));
    }
    candidate.activeLayerId = layerId;
    if (distinctClipMaskBytes(candidate)
        > DocumentLimits::maximumDistinctClipMaskBytes)
    {
        return reject(PasteLayerResult::RejectedMaskLimit);
    }
    auto effects = std::make_shared<HistoryEffects>();
    effects->afterDocumentChanged.append(
        HistoryEffects::LayerThumbnail{layerId});
    effects->afterDocumentChanged.append(HistoryEffects::ActiveLayer{});
    if (followsSelection)
    {
        std::optional<PackedMaskRegion> packedBefore =
            packBinaryMask(selectionMask);
        std::optional<PackedMaskRegion> packedAfter =
            packBinaryMask(nextSelectionMask);
        if (!packedBefore || !packedAfter)
        {
            return reject(PasteLayerResult::RejectedMaskLimit);
        }
        effects->selectionState = HistoryEffects::SelectionStateTransition{
            {current.activeLayerId, std::move(packedBefore)},
            {layerId, std::move(packedAfter)}};
    }
    if (!tryCommitCandidate(
            followsSelection ? tr("Copy selection") : tr("Paste"),
            std::move(candidate),
            std::move(effects),
            ActiveLayerPolicy::UsePrepared))
    {
        return PasteLayerResult::RejectedCommit;
    }
    return PasteLayerResult::Pasted;
}

void DocumentController::removeLayer(const QUuid &id)
{
    const Document &current = document();
    const int index = current.layerIndex(id);
    if (index < 0)
    {
        failHistoryMacro();
        return;
    }
    Document candidate = current;
    QSet<QUuid> removedIds{id};
    if (current.layers[index].kind == LayerKind::Group)
    {
        const LayerHierarchyAnalysis hierarchy = analyzeLayerHierarchy(current);
        if (!hierarchy.isValid())
        {
            failHistoryMacro();
            return;
        }
        for (const Layer &layer : current.layers)
        {
            if (hierarchy.isDescendantOf(layer.id, id))
            {
                removedIds.insert(layer.id);
            }
        }
    }
    if (removedIds.contains(candidate.activeLayerId))
    {
        candidate.activeLayerId = QUuid();
        const auto choosePaint = [&](int begin, int end, int step)
        {
            for (int candidateIndex = begin; candidateIndex != end;
                candidateIndex += step)
            {
                const Layer &candidateLayer = current.layers[candidateIndex];
                if (!removedIds.contains(candidateLayer.id)
                    && candidateLayer.kind == LayerKind::Paint)
                {
                    candidate.activeLayerId = candidateLayer.id;
                    return true;
                }
            }
            return false;
        };
        if (!choosePaint(index - 1, -1, -1))
        {
            choosePaint(index + 1, static_cast<int>(current.layers.size()), 1);
        }
    }
    candidate.layers.removeIf(
        [&removedIds](const Layer &layer)
        {
            return removedIds.contains(layer.id);
        });
    ensureActiveLayer(candidate);
    auto effects = std::make_shared<HistoryEffects>();
    effects->afterDocumentChanged.append(
        HistoryEffects::LayerThumbnailsReset{});
    effects->afterDocumentChanged.append(HistoryEffects::ActiveLayer{});
    tryCommitCandidate(tr("Delete layer"),
        std::move(candidate),
        std::move(effects),
        ActiveLayerPolicy::UsePrepared);
}

void DocumentController::clearLayer(const QUuid &id)
{
    const Document &current = document();
    const Layer *layer = current.layer(id);
    if (!layer || layer->kind != LayerKind::Paint
        || (layer->strokes.isEmpty()
            && layer->initialCanvasSize == current.size))
    {
        failHistoryMacro();
        return;
    }
    Document candidate = current;
    Layer *target = candidate.layer(id);
    target->strokes.clear();
    target->initialCanvasSize = current.size;
    auto effects = std::make_shared<HistoryEffects>();
    effects->afterDocumentChanged.append(HistoryEffects::LayerThumbnail{id});
    tryCommitCandidate(
        tr("Clear layer"), std::move(candidate), std::move(effects));
}

DocumentController::RenameLayerResult DocumentController::renameLayer(
    const QUuid &id, const QString &name)
{
    const auto reject = [this](RenameLayerResult result)
    {
        failHistoryMacro();
        return result;
    };
    const Document &current = document();
    const Layer *layer = current.layer(id);
    const QString normalized = name.trimmed();
    if (!layer)
    {
        return reject(RenameLayerResult::RejectedInvalidLayer);
    }
    if (normalized.isEmpty())
    {
        return reject(RenameLayerResult::RejectedEmptyName);
    }
    if (normalized.size() > DocumentLimits::maximumLayerNameLength)
    {
        return reject(RenameLayerResult::RejectedNameTooLong);
    }
    if (layer->name == normalized)
    {
        return reject(RenameLayerResult::Unchanged);
    }
    Document candidate = current;
    candidate.layer(id)->name = normalized;
    return tryCommitCandidate(tr("Rename layer"), std::move(candidate))
               ? RenameLayerResult::Renamed
               : RenameLayerResult::RejectedCommit;
}

void DocumentController::setLayerVisible(const QUuid &id, bool visible)
{
    const Document &current = document();
    const Layer *layer = current.layer(id);
    if (!layer || layer->visible == visible)
    {
        failHistoryMacro();
        return;
    }
    Document candidate = current;
    candidate.layer(id)->visible = visible;
    auto effects = std::make_shared<HistoryEffects>();
    effects->afterDocumentChanged.append(
        HistoryEffects::LayerThumbnailsReset{});
    tryCommitCandidate(tr("Toggle layer visibility"),
        std::move(candidate),
        std::move(effects));
}

void DocumentController::setLayerReference(const QUuid &id, bool reference)
{
    const Document &current = document();
    const Layer *layer = current.layer(id);
    if (!layer || layer->kind != LayerKind::Paint
        || layer->reference == reference)
    {
        failHistoryMacro();
        return;
    }
    Document candidate = current;
    candidate.layer(id)->reference = reference;
    tryCommitCandidate(tr("Set reference layer"), std::move(candidate));
}

void DocumentController::setLayerOpacity(const QUuid &id, qreal opacity)
{
    const Document &current = document();
    const Layer *layer = current.layer(id);
    if (!std::isfinite(opacity))
    {
        failHistoryMacro();
        return;
    }
    const qreal normalized = std::clamp(opacity, 0.0, 1.0);
    if (!layer || qFuzzyCompare(layer->opacity, normalized))
    {
        failHistoryMacro();
        return;
    }
    Document candidate = current;
    candidate.layer(id)->opacity = normalized;
    auto effects = std::make_shared<HistoryEffects>();
    effects->afterDocumentChanged.append(
        HistoryEffects::LayerThumbnailsReset{});
    tryCommitCandidate(tr("Change layer opacity"),
        std::move(candidate),
        std::move(effects),
        ActiveLayerPolicy::PreserveCurrentIfPresent,
        layerOpacityMergeId,
        id);
}

void DocumentController::setLayerWobbleOverride(const QUuid &id,
    const std::optional<qreal> &wobbleAmount,
    const std::optional<MotionSettings> &motion)
{
    const Document &current = document();
    const Layer *layer = current.layer(id);
    if (!layer || layer->kind != LayerKind::Paint)
    {
        failHistoryMacro();
        return;
    }
    // Both halves move together: a layer either follows the document or states
    // its own motion in full. Half an override would render against a mix of
    // the two and be impossible to reason about in the layer list.
    if (wobbleAmount.has_value() != motion.has_value())
    {
        failHistoryMacro();
        return;
    }
    std::optional<qreal> normalizedAmount;
    if (wobbleAmount)
    {
        if (!std::isfinite(*wobbleAmount))
        {
            failHistoryMacro();
            return;
        }
        normalizedAmount = std::clamp(*wobbleAmount,
            DocumentLimits::minimumWobbleAmount,
            DocumentLimits::maximumWobbleAmount);
    }
    std::optional<MotionSettings> normalizedMotion = motion;
    if (normalizedMotion)
    {
        if (!isValidMotionStyle(normalizedMotion->style))
        {
            failHistoryMacro();
            return;
        }
        normalizedMotion->poseCount = std::clamp(normalizedMotion->poseCount,
            DocumentLimits::minimumMotionPoseCount,
            normalizedMotion->style == MotionStyle::Classic
                ? DocumentLimits::maximumMotionPoseCount
                : current.animationFrames);
        normalizedMotion->detail = std::clamp(normalizedMotion->detail,
            DocumentLimits::minimumMotionDetail,
            DocumentLimits::maximumMotionDetail);
        normalizedMotion->linked =
            std::clamp(normalizedMotion->linked, 0.0, 1.0);
        normalizedMotion->randomness =
            std::clamp(normalizedMotion->randomness, 0.0, 1.0);
        normalizedMotion->breakAmount =
            std::clamp(normalizedMotion->breakAmount, 0.0, 1.0);
        normalizedMotion->breakRange = std::clamp(normalizedMotion->breakRange,
            DocumentLimits::minimumBreakRange,
            DocumentLimits::maximumBreakRange);
    }
    if (layer->wobbleAmount == normalizedAmount
        && layer->motion == normalizedMotion)
    {
        failHistoryMacro();
        return;
    }
    Document candidate = current;
    Layer *target = candidate.layer(id);
    target->wobbleAmount = normalizedAmount;
    target->motion = normalizedMotion;
    auto effects = std::make_shared<HistoryEffects>();
    effects->afterDocumentChanged.append(
        HistoryEffects::LayerThumbnailsReset{});
    tryCommitCandidate(normalizedAmount ? tr("Change layer wobble")
                                        : tr("Follow document wobble"),
        std::move(candidate),
        std::move(effects),
        ActiveLayerPolicy::PreserveCurrentIfPresent,
        layerWobbleMergeId,
        id);
}

void DocumentController::setLayerBlendMode(const QUuid &id, LayerBlendMode mode)
{
    const Document &current = document();
    const Layer *layer = current.layer(id);
    if (!layer || !isValidLayerBlendMode(mode) || layer->blendMode == mode)
    {
        failHistoryMacro();
        return;
    }
    Document candidate = current;
    candidate.layer(id)->blendMode = mode;
    auto effects = std::make_shared<HistoryEffects>();
    effects->afterDocumentChanged.append(
        HistoryEffects::LayerThumbnailsReset{});
    tryCommitCandidate(tr("Change layer blend mode"),
        std::move(candidate),
        std::move(effects));
}

void DocumentController::setLayerClipToBelow(const QUuid &id, bool clipped)
{
    const Document &current = document();
    const Layer *layer = current.layer(id);
    if (!layer || layer->kind != LayerKind::Paint
        || layer->clipToLayerBelow == clipped)
    {
        failHistoryMacro();
        return;
    }
    Document candidate = current;
    candidate.layer(id)->clipToLayerBelow = clipped;
    auto effects = std::make_shared<HistoryEffects>();
    effects->afterDocumentChanged.append(
        HistoryEffects::LayerThumbnailsReset{});
    tryCommitCandidate(
        tr("Change layer clipping"), std::move(candidate), std::move(effects));
}

void DocumentController::setLayerParentGroup(
    const QUuid &id, const QUuid &groupId)
{
    const Document &current = document();
    const Layer *layer = current.layer(id);
    const Layer *group = current.layer(groupId);
    const bool validParent =
        groupId.isNull() || (group && group->kind == LayerKind::Group);
    if (!layer || !validParent || layer->parentGroupId == groupId
        || id == groupId)
    {
        failHistoryMacro();
        return;
    }
    Document candidate = current;
    candidate.layer(id)->parentGroupId = groupId;
    auto effects = std::make_shared<HistoryEffects>();
    effects->afterDocumentChanged.append(
        HistoryEffects::LayerThumbnailsReset{});
    tryCommitCandidate(
        tr("Move layer into group"), std::move(candidate), std::move(effects));
}

void DocumentController::moveLayer(const QUuid &id, int offset)
{
    const Document &current = document();
    const int from = current.layerIndex(id);
    const Layer *layer = current.layer(id);
    if (from < 0 || !layer || offset == 0)
    {
        failHistoryMacro();
        return;
    }
    QVector<int> siblingIndexes;
    for (int index = 0; index < current.layers.size(); ++index)
    {
        if (current.layers[index].parentGroupId == layer->parentGroupId)
        {
            siblingIndexes.append(index);
        }
    }
    const int siblingPosition = static_cast<int>(siblingIndexes.indexOf(from));
    const int targetPosition = siblingPosition + offset;
    if (siblingPosition < 0 || targetPosition < 0
        || targetPosition >= siblingIndexes.size())
    {
        failHistoryMacro();
        return;
    }
    const int to = siblingIndexes[targetPosition];
    Document candidate = current;
    candidate.layers.move(from, to);
    auto effects = std::make_shared<HistoryEffects>();
    effects->afterDocumentChanged.append(
        HistoryEffects::LayerThumbnailsReset{});
    tryCommitCandidate(
        tr("Move layer"), std::move(candidate), std::move(effects));
}

}

// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "document/history/DocumentDelta.hpp"

#include <QHash>
#include <QSet>

#include <algorithm>
#include <tuple>
#include <utility>

namespace ugurugu
{
namespace history
{

namespace
{

using IndexedLayer = DocumentDelta::IndexedLayer;
using IndexedStroke = DocumentDelta::IndexedStroke;
using LayerChange = DocumentDelta::LayerChange;
using ReplacedStroke = DocumentDelta::ReplacedStroke;
using StrokeSequenceDelta = DocumentDelta::StrokeSequenceDelta;

constexpr auto documentScalarFields = std::tuple{&DocumentDelta::size,
    &DocumentDelta::background,
    &DocumentDelta::animationFrames,
    &DocumentDelta::framesPerSecond,
    &DocumentDelta::wobbleAmount,
    &DocumentDelta::motionSettings,
    &DocumentDelta::rasterAssets,
    &DocumentDelta::activeLayerId};

constexpr auto layerScalarFields = std::tuple{&LayerChange::name,
    &LayerChange::visible,
    &LayerChange::reference,
    &LayerChange::opacity,
    &LayerChange::blendMode,
    &LayerChange::parentGroupId,
    &LayerChange::clipToLayerBelow,
    &LayerChange::initialCanvasSize,
    &LayerChange::wobbleAmount,
    &LayerChange::motion};

template <typename Owner, typename Fields>
int populatedFieldCount(const Owner &owner, const Fields &fields)
{
    return std::apply(
        [&owner](auto... field)
        {
            return (
                0 + ... + static_cast<int>(static_cast<bool>(owner.*field)));
        },
        fields);
}

bool hasNoDocumentStructure(const DocumentDelta &delta)
{
    return delta.removedLayers.isEmpty() && delta.addedLayers.isEmpty()
           && delta.beforeLayerOrder.isEmpty()
           && delta.afterLayerOrder.isEmpty();
}

template <typename T>
bool mergeValue(std::optional<DocumentDelta::ValueChange<T>> &current,
    const std::optional<DocumentDelta::ValueChange<T>> &later)
{
    if (!current || !later || current->after != later->before)
    {
        return false;
    }
    current->after = later->after;
    return true;
}

template <typename T>
bool mergeDocumentScalar(DocumentDelta &current,
    const DocumentDelta &next,
    std::optional<DocumentDelta::ValueChange<T>> DocumentDelta::*field)
{
    if (!hasNoDocumentStructure(current) || !hasNoDocumentStructure(next)
        || !current.changedLayers.isEmpty() || !next.changedLayers.isEmpty()
        || populatedFieldCount(current, documentScalarFields) != 1
        || populatedFieldCount(next, documentScalarFields) != 1)
    {
        return false;
    }
    return mergeValue(current.*field, next.*field);
}

template <typename T>
bool mergeLayerScalar(DocumentDelta &current,
    const DocumentDelta &next,
    const QUuid &scope,
    std::optional<DocumentDelta::ValueChange<T>> LayerChange::*field)
{
    if (scope.isNull() || !hasNoDocumentStructure(current)
        || !hasNoDocumentStructure(next)
        || populatedFieldCount(current, documentScalarFields) != 0
        || populatedFieldCount(next, documentScalarFields) != 0
        || current.changedLayers.size() != 1 || next.changedLayers.size() != 1)
    {
        return false;
    }
    LayerChange &currentLayer = current.changedLayers[0];
    const LayerChange &nextLayer = next.changedLayers[0];
    if (currentLayer.id != scope || nextLayer.id != scope
        || !currentLayer.strokes.isEmpty() || !nextLayer.strokes.isEmpty()
        || populatedFieldCount(currentLayer, layerScalarFields) != 1
        || populatedFieldCount(nextLayer, layerScalarFields) != 1)
    {
        return false;
    }
    return mergeValue(currentLayer.*field, nextLayer.*field);
}

// A layer's wobble override is two fields that move as one gesture, so it
// needs a merge that spans both rather than the single-field form above. A
// field only one side touched is adopted as is: the other side left it alone,
// so its recorded before value is still the truth.
bool mergeLayerWobble(
    DocumentDelta &current, const DocumentDelta &next, const QUuid &scope)
{
    if (scope.isNull() || !hasNoDocumentStructure(current)
        || !hasNoDocumentStructure(next)
        || populatedFieldCount(current, documentScalarFields) != 0
        || populatedFieldCount(next, documentScalarFields) != 0
        || current.changedLayers.size() != 1 || next.changedLayers.size() != 1)
    {
        return false;
    }
    LayerChange &currentLayer = current.changedLayers[0];
    const LayerChange &nextLayer = next.changedLayers[0];
    const auto touchesOnlyWobble = [](const LayerChange &change)
    {
        return !change.name && !change.visible && !change.reference
               && !change.opacity && !change.blendMode && !change.parentGroupId
               && !change.clipToLayerBelow && !change.initialCanvasSize
               && change.strokes.isEmpty();
    };
    if (currentLayer.id != scope || nextLayer.id != scope
        || !touchesOnlyWobble(currentLayer) || !touchesOnlyWobble(nextLayer))
    {
        return false;
    }
    const auto fold =
        []<typename T>(std::optional<DocumentDelta::ValueChange<T>> &target,
            const std::optional<DocumentDelta::ValueChange<T>> &later)
    {
        if (!later)
        {
            return true;
        }
        if (!target)
        {
            target = later;
            return true;
        }
        if (target->after != later->before)
        {
            return false;
        }
        target->after = later->after;
        return true;
    };
    return fold(currentLayer.wobbleAmount, nextLayer.wobbleAmount)
           && fold(currentLayer.motion, nextLayer.motion);
}

// Payload equality is decided by backing address, not by content, because a
// delta only has to detect whether the two states still share one allocation.
// Two distinct buffers holding identical bytes compare unequal, which costs a
// redundant delta entry but never loses a change.
bool samePointBacking(
    const QVector<StrokePoint> &left, const QVector<StrokePoint> &right)
{
    return left.size() == right.size()
           && (left.isEmpty() || left.constData() == right.constData());
}

bool sameStrokeVectorBacking(
    const QVector<Stroke> &left, const QVector<Stroke> &right)
{
    return left.size() == right.size()
           && (left.isEmpty() || left.constData() == right.constData());
}

bool sameByteBacking(const QByteArray &left, const QByteArray &right)
{
    return left.size() == right.size()
           && (left.isEmpty() || left.constData() == right.constData());
}

bool sameImageBacking(const QImage &left, const QImage &right)
{
    if (left.isNull() || right.isNull())
    {
        return left.isNull() && right.isNull();
    }
    return left.cacheKey() == right.cacheKey() && left.size() == right.size()
           && left.format() == right.format();
}

bool samePixelSelectionOperation(const std::optional<PixelSelectionOp> &left,
    const std::optional<PixelSelectionOp> &right)
{
    if (!left || !right)
    {
        return !left && !right;
    }
    return left->canvasSize == right->canvasSize
           && left->sourceBounds == right->sourceBounds
           && sameByteBacking(left->packedMask, right->packedMask)
           && left->transform == right->transform
           && left->sampling == right->sampling
           && left->clearSource == right->clearSource
           && left->drawDestination == right->drawDestination;
}

bool sameStroke(const Stroke &left, const Stroke &right)
{
    return left.id == right.id && left.seed == right.seed
           && left.mode == right.mode && left.color == right.color
           && left.width == right.width && left.brush == right.brush
           && samePointBacking(left.points, right.points)
           && left.visibilityClip == right.visibilityClip
           && sameImageBacking(left.clipMask, right.clipMask)
           && sameImageBacking(left.fillMask, right.fillMask)
           && left.fillCoverage == right.fillCoverage
           && samePixelSelectionOperation(
               left.pixelSelectionOp, right.pixelSelectionOp)
           && left.reframeOp == right.reframeOp
           && left.imageOp == right.imageOp;
}

template <typename T>
void recordChange(const T &before,
    const T &after,
    std::optional<DocumentDelta::ValueChange<T>> &destination)
{
    if (before != after)
    {
        destination = DocumentDelta::ValueChange<T>{before, after};
    }
}

template <typename T>
void applyChange(T &target,
    const std::optional<DocumentDelta::ValueChange<T>> &change,
    bool forward)
{
    if (change)
    {
        target = forward ? change->after : change->before;
    }
}

QVector<QUuid> strokeIds(const QVector<Stroke> &strokes)
{
    QVector<QUuid> ids;
    ids.reserve(strokes.size());
    for (const Stroke &stroke : strokes)
    {
        ids.append(stroke.id);
    }
    return ids;
}

QVector<QUuid> layerIds(const QVector<Layer> &layers)
{
    QVector<QUuid> ids;
    ids.reserve(layers.size());
    for (const Layer &layer : layers)
    {
        ids.append(layer.id);
    }
    return ids;
}

StrokeSequenceDelta betweenStrokes(
    const QVector<Stroke> &before, const QVector<Stroke> &after)
{
    if (sameStrokeVectorBacking(before, after))
    {
        return {};
    }

    // Appending is the dominant drawing path, while equal-position
    // replacement covers transforms.  Avoid two large UUID hash tables
    // when the common prefix already proves that no sequence matching is
    // needed.
    const int commonPrefixSize =
        static_cast<int>(std::min(before.size(), after.size()));
    StrokeSequenceDelta aligned;
    bool commonPrefixMatches = true;
    for (int index = 0; index < commonPrefixSize; ++index)
    {
        if (before[index].id != after[index].id)
        {
            commonPrefixMatches = false;
            break;
        }
        if (!sameStroke(before[index], after[index]))
        {
            aligned.replaced.append(
                {before[index].id, before[index], after[index]});
        }
    }
    if (commonPrefixMatches)
    {
        for (int index = commonPrefixSize; index < before.size(); ++index)
        {
            aligned.removed.append({index, before[index]});
        }
        for (int index = commonPrefixSize; index < after.size(); ++index)
        {
            aligned.added.append({index, after[index]});
        }
        return aligned;
    }

    StrokeSequenceDelta delta;
    QHash<QUuid, int> beforeIndexes;
    QHash<QUuid, int> afterIndexes;
    beforeIndexes.reserve(before.size());
    afterIndexes.reserve(after.size());
    for (int index = 0; index < before.size(); ++index)
    {
        beforeIndexes.insert(before[index].id, index);
    }
    for (int index = 0; index < after.size(); ++index)
    {
        afterIndexes.insert(after[index].id, index);
    }

    QVector<QUuid> commonBefore;
    QVector<QUuid> commonAfter;
    commonBefore.reserve(std::min(before.size(), after.size()));
    commonAfter.reserve(std::min(before.size(), after.size()));
    for (int index = 0; index < before.size(); ++index)
    {
        const Stroke &stroke = before[index];
        const auto afterIndex = afterIndexes.constFind(stroke.id);
        if (afterIndex == afterIndexes.cend())
        {
            delta.removed.append({index, stroke});
            continue;
        }
        commonBefore.append(stroke.id);
        const Stroke &afterStroke = after[*afterIndex];
        if (!sameStroke(stroke, afterStroke))
        {
            delta.replaced.append({stroke.id, stroke, afterStroke});
        }
    }
    for (int index = 0; index < after.size(); ++index)
    {
        const Stroke &stroke = after[index];
        if (!beforeIndexes.contains(stroke.id))
        {
            delta.added.append({index, stroke});
        }
        else
        {
            commonAfter.append(stroke.id);
        }
    }
    if (commonBefore != commonAfter)
    {
        delta.beforeOrder = strokeIds(before);
        delta.afterOrder = strokeIds(after);
    }
    return delta;
}

bool reorderStrokes(QVector<Stroke> &strokes, const QVector<QUuid> &order)
{
    if (order.isEmpty())
    {
        return true;
    }
    if (strokes.size() != order.size())
    {
        return false;
    }
    QHash<QUuid, int> indexes;
    indexes.reserve(strokes.size());
    for (int index = 0; index < strokes.size(); ++index)
    {
        if (indexes.contains(strokes[index].id))
        {
            return false;
        }
        indexes.insert(strokes[index].id, index);
    }
    QVector<Stroke> reordered;
    reordered.reserve(strokes.size());
    for (const QUuid &id : order)
    {
        const auto found = indexes.constFind(id);
        if (found == indexes.cend())
        {
            return false;
        }
        reordered.append(strokes[*found]);
    }
    strokes = std::move(reordered);
    return true;
}

bool reorderLayers(QVector<Layer> &layers, const QVector<QUuid> &order)
{
    if (order.isEmpty())
    {
        return true;
    }
    if (layers.size() != order.size())
    {
        return false;
    }
    QHash<QUuid, int> indexes;
    indexes.reserve(layers.size());
    for (int index = 0; index < layers.size(); ++index)
    {
        if (indexes.contains(layers[index].id))
        {
            return false;
        }
        indexes.insert(layers[index].id, index);
    }
    QVector<Layer> reordered;
    reordered.reserve(layers.size());
    for (const QUuid &id : order)
    {
        const auto found = indexes.constFind(id);
        if (found == indexes.cend())
        {
            return false;
        }
        reordered.append(layers[*found]);
    }
    layers = std::move(reordered);
    return true;
}

bool applyStrokeDelta(
    QVector<Stroke> &strokes, const StrokeSequenceDelta &delta, bool forward)
{
    if (delta.isEmpty())
    {
        return true;
    }
    QHash<QUuid, int> currentIndexes;
    currentIndexes.reserve(strokes.size());
    for (int index = 0; index < strokes.size(); ++index)
    {
        if (currentIndexes.contains(strokes[index].id))
        {
            return false;
        }
        currentIndexes.insert(strokes[index].id, index);
    }
    for (const ReplacedStroke &replacement : delta.replaced)
    {
        const auto found = currentIndexes.constFind(replacement.id);
        if (found == currentIndexes.cend())
        {
            return false;
        }
        strokes[*found] = forward ? replacement.after : replacement.before;
    }

    const QVector<IndexedStroke> &removals =
        forward ? delta.removed : delta.added;
    QSet<QUuid> removalIds;
    removalIds.reserve(removals.size());
    for (const IndexedStroke &entry : removals)
    {
        if (entry.index < 0 || entry.index >= strokes.size()
            || strokes[entry.index].id != entry.stroke.id
            || removalIds.contains(entry.stroke.id))
        {
            return false;
        }
        removalIds.insert(entry.stroke.id);
    }
    QVector<Stroke> retained;
    retained.reserve(strokes.size() - removals.size());
    for (const Stroke &stroke : std::as_const(strokes))
    {
        if (!removalIds.contains(stroke.id))
        {
            retained.append(stroke);
        }
    }
    if (retained.size() != strokes.size() - removals.size())
    {
        return false;
    }

    const QVector<IndexedStroke> &additions =
        forward ? delta.added : delta.removed;
    const int targetSize = static_cast<int>(retained.size() + additions.size());
    QSet<QUuid> targetIds;
    targetIds.reserve(targetSize);
    for (const Stroke &stroke : std::as_const(retained))
    {
        targetIds.insert(stroke.id);
    }
    int previousAdditionIndex = -1;
    for (const IndexedStroke &entry : additions)
    {
        if (entry.index <= previousAdditionIndex || entry.index < 0
            || entry.index >= targetSize || targetIds.contains(entry.stroke.id))
        {
            return false;
        }
        previousAdditionIndex = entry.index;
        targetIds.insert(entry.stroke.id);
    }

    QVector<Stroke> rebuilt;
    rebuilt.reserve(targetSize);
    int retainedIndex = 0;
    int additionIndex = 0;
    for (int targetIndex = 0; targetIndex < targetSize; ++targetIndex)
    {
        if (additionIndex < additions.size()
            && additions[additionIndex].index == targetIndex)
        {
            rebuilt.append(additions[additionIndex].stroke);
            ++additionIndex;
        }
        else
        {
            if (retainedIndex >= retained.size())
            {
                return false;
            }
            rebuilt.append(retained[retainedIndex]);
            ++retainedIndex;
        }
    }
    if (retainedIndex != retained.size() || additionIndex != additions.size())
    {
        return false;
    }
    strokes = std::move(rebuilt);
    return reorderStrokes(
        strokes, forward ? delta.afterOrder : delta.beforeOrder);
}

void accountStroke(MemoryFootprint &footprint, const Stroke &stroke)
{
    accountVectorStorage(
        footprint, stroke.points, QStringLiteral("stroke-points"));
    accountImageStorage(footprint, stroke.clipMask);
    accountImageStorage(footprint, stroke.fillMask);
    accountPackedMask(footprint, stroke.fillCoverage);
    if (stroke.pixelSelectionOp)
    {
        accountByteStorage(footprint, stroke.pixelSelectionOp->packedMask);
    }
}

void accountLayer(MemoryFootprint &footprint, const Layer &layer)
{
    accountVectorStorage(
        footprint, layer.strokes, QStringLiteral("layer-strokes"));
    for (const Stroke &stroke : layer.strokes)
    {
        accountStroke(footprint, stroke);
    }
    footprint.addOwned(static_cast<qint64>(layer.name.capacity())
                       * static_cast<qint64>(sizeof(QChar)));
}

void accountRasterAssets(
    MemoryFootprint &footprint, const QMap<QString, RasterAsset> &assets)
{
    for (const RasterAsset &asset : assets)
    {
        accountByteStorage(footprint, asset.compressedRgba);
        footprint.addOwned(static_cast<qint64>(asset.id.capacity())
                           * static_cast<qint64>(sizeof(QChar)));
    }
}

}

DocumentDelta DocumentDelta::between(
    const Document &before, const Document &after)
{
    DocumentDelta delta;
    recordChange(before.size, after.size, delta.size);
    recordChange(before.background, after.background, delta.background);
    recordChange(
        before.animationFrames, after.animationFrames, delta.animationFrames);
    recordChange(
        before.framesPerSecond, after.framesPerSecond, delta.framesPerSecond);
    recordChange(before.wobbleAmount, after.wobbleAmount, delta.wobbleAmount);
    recordChange(before.motion, after.motion, delta.motionSettings);
    recordChange(before.rasterAssets, after.rasterAssets, delta.rasterAssets);
    recordChange(
        before.activeLayerId, after.activeLayerId, delta.activeLayerId);

    QHash<QUuid, int> beforeIndexes;
    QHash<QUuid, int> afterIndexes;
    beforeIndexes.reserve(before.layers.size());
    afterIndexes.reserve(after.layers.size());
    for (int index = 0; index < before.layers.size(); ++index)
    {
        beforeIndexes.insert(before.layers[index].id, index);
    }
    for (int index = 0; index < after.layers.size(); ++index)
    {
        afterIndexes.insert(after.layers[index].id, index);
    }

    QVector<QUuid> commonBefore;
    QVector<QUuid> commonAfter;
    commonBefore.reserve(std::min(before.layers.size(), after.layers.size()));
    commonAfter.reserve(std::min(before.layers.size(), after.layers.size()));
    for (int index = 0; index < before.layers.size(); ++index)
    {
        const Layer &layer = before.layers[index];
        const auto afterIndex = afterIndexes.constFind(layer.id);
        if (afterIndex == afterIndexes.cend())
        {
            delta.removedLayers.append({index, layer});
            continue;
        }
        commonBefore.append(layer.id);
        const Layer &afterLayer = after.layers[*afterIndex];
        LayerChange change;
        change.id = layer.id;
        recordChange(layer.name, afterLayer.name, change.name);
        recordChange(layer.visible, afterLayer.visible, change.visible);
        recordChange(layer.reference, afterLayer.reference, change.reference);
        recordChange(layer.opacity, afterLayer.opacity, change.opacity);
        recordChange(layer.blendMode, afterLayer.blendMode, change.blendMode);
        recordChange(layer.parentGroupId,
            afterLayer.parentGroupId,
            change.parentGroupId);
        recordChange(layer.clipToLayerBelow,
            afterLayer.clipToLayerBelow,
            change.clipToLayerBelow);
        recordChange(layer.initialCanvasSize,
            afterLayer.initialCanvasSize,
            change.initialCanvasSize);
        recordChange(
            layer.wobbleAmount, afterLayer.wobbleAmount, change.wobbleAmount);
        recordChange(layer.motion, afterLayer.motion, change.motion);
        if (!sameStrokeVectorBacking(layer.strokes, afterLayer.strokes))
        {
            change.strokes = betweenStrokes(layer.strokes, afterLayer.strokes);
        }
        if (!change.isEmpty())
        {
            delta.changedLayers.append(std::move(change));
        }
    }
    for (int index = 0; index < after.layers.size(); ++index)
    {
        const Layer &layer = after.layers[index];
        if (!beforeIndexes.contains(layer.id))
        {
            delta.addedLayers.append({index, layer});
        }
        else
        {
            commonAfter.append(layer.id);
        }
    }
    if (commonBefore != commonAfter)
    {
        delta.beforeLayerOrder = layerIds(before.layers);
        delta.afterLayerOrder = layerIds(after.layers);
    }
    return delta;
}

DocumentDelta DocumentDelta::appendedStroke(
    const Document &before, const Document &after, const QUuid &layerId)
{
    DocumentDelta delta;
    if (before.size != after.size || before.background != after.background
        || before.animationFrames != after.animationFrames
        || before.framesPerSecond != after.framesPerSecond
        || before.wobbleAmount != after.wobbleAmount
        || before.motion != after.motion
        || before.rasterAssets != after.rasterAssets
        || before.activeLayerId != after.activeLayerId
        || before.layers.size() != after.layers.size())
    {
        return delta;
    }
    for (int index = 0; index < before.layers.size(); ++index)
    {
        const Layer &beforeLayer = before.layers[index];
        const Layer &afterLayer = after.layers[index];
        if (beforeLayer.id != afterLayer.id
            || beforeLayer.name != afterLayer.name
            || beforeLayer.kind != afterLayer.kind
            || beforeLayer.parentGroupId != afterLayer.parentGroupId
            || beforeLayer.clipToLayerBelow != afterLayer.clipToLayerBelow
            || beforeLayer.visible != afterLayer.visible
            || beforeLayer.reference != afterLayer.reference
            || beforeLayer.opacity != afterLayer.opacity
            || beforeLayer.blendMode != afterLayer.blendMode
            || beforeLayer.wobbleAmount != afterLayer.wobbleAmount
            || beforeLayer.motion != afterLayer.motion
            || beforeLayer.initialCanvasSize != afterLayer.initialCanvasSize)
        {
            return {};
        }
        if (beforeLayer.id == layerId)
        {
            if (afterLayer.strokes.size() != beforeLayer.strokes.size() + 1)
            {
                return {};
            }
            LayerChange change;
            change.id = layerId;
            change.strokes.added.append(
                {static_cast<int>(beforeLayer.strokes.size()),
                    afterLayer.strokes.constLast()});
            delta.changedLayers.append(std::move(change));
        }
        else if (!sameStrokeVectorBacking(
                     beforeLayer.strokes, afterLayer.strokes))
        {
            return {};
        }
    }
    return delta;
}

bool DocumentDelta::isEmpty() const
{
    return !size && !background && !animationFrames && !framesPerSecond
           && !wobbleAmount && !motionSettings && !rasterAssets
           && !activeLayerId && removedLayers.isEmpty() && addedLayers.isEmpty()
           && changedLayers.isEmpty() && beforeLayerOrder.isEmpty();
}

bool DocumentDelta::apply(Document &document, bool forward) const
{
    applyChange(document.size, size, forward);
    applyChange(document.background, background, forward);
    applyChange(document.animationFrames, animationFrames, forward);
    applyChange(document.framesPerSecond, framesPerSecond, forward);
    applyChange(document.wobbleAmount, wobbleAmount, forward);
    applyChange(document.motion, motionSettings, forward);
    applyChange(document.rasterAssets, rasterAssets, forward);

    for (const LayerChange &change : changedLayers)
    {
        Layer *layer = document.layer(change.id);
        if (!layer)
        {
            return false;
        }
        applyChange(layer->name, change.name, forward);
        applyChange(layer->visible, change.visible, forward);
        applyChange(layer->reference, change.reference, forward);
        applyChange(layer->opacity, change.opacity, forward);
        applyChange(layer->blendMode, change.blendMode, forward);
        applyChange(layer->parentGroupId, change.parentGroupId, forward);
        applyChange(layer->clipToLayerBelow, change.clipToLayerBelow, forward);
        applyChange(
            layer->initialCanvasSize, change.initialCanvasSize, forward);
        applyChange(layer->wobbleAmount, change.wobbleAmount, forward);
        applyChange(layer->motion, change.motion, forward);
        if (!applyStrokeDelta(layer->strokes, change.strokes, forward))
        {
            return false;
        }
    }

    const QVector<IndexedLayer> &removals =
        forward ? removedLayers : addedLayers;
    for (auto entry = removals.crbegin(); entry != removals.crend(); ++entry)
    {
        const int index = document.layerIndex(entry->layer.id);
        if (index < 0)
        {
            return false;
        }
        document.layers.removeAt(index);
    }

    const QVector<IndexedLayer> &additions =
        forward ? addedLayers : removedLayers;
    for (const IndexedLayer &entry : additions)
    {
        if (entry.index < 0 || entry.index > document.layers.size()
            || document.layer(entry.layer.id))
        {
            return false;
        }
        document.layers.insert(entry.index, entry.layer);
    }
    if (!reorderLayers(
            document.layers, forward ? afterLayerOrder : beforeLayerOrder))
    {
        return false;
    }
    applyChange(document.activeLayerId, activeLayerId, forward);
    return true;
}

bool DocumentDelta::mergeScalar(
    const DocumentDelta &next, int mergeId, const QUuid &scope)
{
    if (mergeId == wobbleAmountMergeId)
    {
        return mergeDocumentScalar(*this, next, &DocumentDelta::wobbleAmount);
    }
    if (mergeId == animationFramesMergeId)
    {
        return mergeDocumentScalar(
            *this, next, &DocumentDelta::animationFrames);
    }
    if (mergeId == framesPerSecondMergeId)
    {
        return mergeDocumentScalar(
            *this, next, &DocumentDelta::framesPerSecond);
    }
    if (mergeId == motionStyleMergeId || mergeId == motionPoseCountMergeId
        || mergeId == motionDetailMergeId || mergeId == motionLinkedMergeId
        || mergeId == motionRandomnessMergeId || mergeId == brokenLineMergeId
        || mergeId == breakAmountMergeId || mergeId == breakRangeMergeId)
    {
        return mergeDocumentScalar(*this, next, &DocumentDelta::motionSettings);
    }
    if (mergeId == layerOpacityMergeId)
    {
        return mergeLayerScalar(*this, next, scope, &LayerChange::opacity);
    }
    if (mergeId == layerWobbleMergeId)
    {
        return mergeLayerWobble(*this, next, scope);
    }
    return false;
}

void DocumentDelta::normalizeMergedChanges()
{
    const auto normalize = []<typename T>(std::optional<ValueChange<T>> &change)
    {
        if (change && change->before == change->after)
        {
            change.reset();
        }
    };
    normalize(size);
    normalize(background);
    normalize(animationFrames);
    normalize(framesPerSecond);
    normalize(wobbleAmount);
    normalize(motionSettings);
    normalize(activeLayerId);
    for (LayerChange &change : changedLayers)
    {
        normalize(change.name);
        normalize(change.visible);
        normalize(change.reference);
        normalize(change.opacity);
        normalize(change.blendMode);
        normalize(change.parentGroupId);
        normalize(change.clipToLayerBelow);
        normalize(change.initialCanvasSize);
        normalize(change.wobbleAmount);
        normalize(change.motion);
    }
    changedLayers.removeIf(
        [](const LayerChange &change)
        {
            return change.isEmpty();
        });
}

QVector<Stroke> DocumentDelta::payloadStrokes(bool targetAfter) const
{
    QVector<Stroke> strokes;
    const QVector<IndexedLayer> &layers =
        targetAfter ? addedLayers : removedLayers;
    qsizetype count = 0;
    for (const IndexedLayer &entry : layers)
    {
        count += entry.layer.strokes.size();
    }
    for (const LayerChange &change : changedLayers)
    {
        count += targetAfter ? change.strokes.added.size()
                                   + change.strokes.replaced.size()
                             : change.strokes.removed.size()
                                   + change.strokes.replaced.size();
    }
    strokes.reserve(count);
    for (const IndexedLayer &entry : layers)
    {
        strokes += entry.layer.strokes;
    }
    for (const LayerChange &change : changedLayers)
    {
        const QVector<IndexedStroke> &indexed =
            targetAfter ? change.strokes.added : change.strokes.removed;
        for (const IndexedStroke &entry : indexed)
        {
            strokes.append(entry.stroke);
        }
        for (const ReplacedStroke &entry : change.strokes.replaced)
        {
            strokes.append(targetAfter ? entry.after : entry.before);
        }
    }
    return strokes;
}

void DocumentDelta::accountStorage(MemoryFootprint &footprint) const
{
    footprint.addOwned(sizeof(DocumentDelta));
    if (rasterAssets)
    {
        accountRasterAssets(footprint, rasterAssets->before);
        accountRasterAssets(footprint, rasterAssets->after);
    }
    footprint.addOwned(static_cast<qint64>(removedLayers.capacity())
                           * static_cast<qint64>(sizeof(IndexedLayer))
                       + static_cast<qint64>(addedLayers.capacity())
                             * static_cast<qint64>(sizeof(IndexedLayer))
                       + static_cast<qint64>(changedLayers.capacity())
                             * static_cast<qint64>(sizeof(LayerChange))
                       + static_cast<qint64>(beforeLayerOrder.capacity())
                             * static_cast<qint64>(sizeof(QUuid))
                       + static_cast<qint64>(afterLayerOrder.capacity())
                             * static_cast<qint64>(sizeof(QUuid)));
    for (const IndexedLayer &entry : removedLayers)
    {
        accountLayer(footprint, entry.layer);
    }
    for (const IndexedLayer &entry : addedLayers)
    {
        accountLayer(footprint, entry.layer);
    }
    for (const LayerChange &change : changedLayers)
    {
        if (change.name)
        {
            footprint.addOwned(
                static_cast<qint64>(change.name->before.capacity()
                                    + change.name->after.capacity())
                * static_cast<qint64>(sizeof(QChar)));
        }
        const StrokeSequenceDelta &strokes = change.strokes;
        footprint.addOwned(
            static_cast<qint64>(strokes.removed.capacity())
                * static_cast<qint64>(sizeof(IndexedStroke))
            + static_cast<qint64>(strokes.added.capacity())
                  * static_cast<qint64>(sizeof(IndexedStroke))
            + static_cast<qint64>(strokes.replaced.capacity())
                  * static_cast<qint64>(sizeof(ReplacedStroke))
            + static_cast<qint64>(strokes.beforeOrder.capacity()
                                  + strokes.afterOrder.capacity())
                  * static_cast<qint64>(sizeof(QUuid)));
        for (const IndexedStroke &entry : strokes.removed)
        {
            accountStroke(footprint, entry.stroke);
        }
        for (const IndexedStroke &entry : strokes.added)
        {
            accountStroke(footprint, entry.stroke);
        }
        for (const ReplacedStroke &entry : strokes.replaced)
        {
            accountStroke(footprint, entry.before);
            accountStroke(footprint, entry.after);
        }
    }
}

StorageStats DocumentDelta::storageStats() const
{
    StorageStats stats;
    stats.retainedLayers = removedLayers.size() + addedLayers.size();
    for (const IndexedLayer &entry : removedLayers)
    {
        stats.retainedStrokes += entry.layer.strokes.size();
    }
    for (const IndexedLayer &entry : addedLayers)
    {
        stats.retainedStrokes += entry.layer.strokes.size();
    }
    for (const LayerChange &change : changedLayers)
    {
        stats.retainedStrokes += change.strokes.retainedStrokeCount();
    }
    return stats;
}

}

}

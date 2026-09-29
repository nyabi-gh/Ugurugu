// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#pragma once

#include "document/Document.hpp"
#include "document/history/HistoryMemory.hpp"
#include "document/history/HistoryTypes.hpp"

#include <QUuid>
#include <QVector>

#include <optional>

namespace ugurugu
{
namespace history
{

// Structural difference between two document states, stored instead of two
// full documents so an undo entry retains only what actually changed.
//
// Strokes and layers are captured by value, but their payload backings
// (points, masks) are shared with the documents they came from, so a delta
// stays cheap only while those backings remain implicitly shared. `apply` is
// all-or-nothing at the document level: it returns false without a usable
// document when the target does not match what the delta was built against,
// and the caller must discard that document rather than present it.
struct DocumentDelta
{
    template <typename T> struct ValueChange
    {
        T before;
        T after;
    };

    struct IndexedStroke
    {
        int index = -1;
        Stroke stroke;
    };

    struct ReplacedStroke
    {
        QUuid id;
        Stroke before;
        Stroke after;
    };

    struct StrokeSequenceDelta
    {
        QVector<IndexedStroke> removed;
        QVector<IndexedStroke> added;
        QVector<ReplacedStroke> replaced;
        QVector<QUuid> beforeOrder;
        QVector<QUuid> afterOrder;

        bool isEmpty() const
        {
            return removed.isEmpty() && added.isEmpty() && replaced.isEmpty()
                   && beforeOrder.isEmpty();
        }

        qsizetype retainedStrokeCount() const
        {
            return removed.size() + added.size() + replaced.size() * 2;
        }
    };

    struct LayerChange
    {
        QUuid id;
        std::optional<ValueChange<QString>> name;
        std::optional<ValueChange<bool>> visible;
        std::optional<ValueChange<bool>> reference;
        std::optional<ValueChange<qreal>> opacity;
        std::optional<ValueChange<LayerBlendMode>> blendMode;
        std::optional<ValueChange<QUuid>> parentGroupId;
        std::optional<ValueChange<bool>> clipToLayerBelow;
        std::optional<ValueChange<QSize>> initialCanvasSize;
        std::optional<ValueChange<std::optional<qreal>>> wobbleAmount;
        std::optional<ValueChange<std::optional<MotionSettings>>> motion;
        StrokeSequenceDelta strokes;

        bool isEmpty() const
        {
            return !name && !visible && !reference && !opacity && !blendMode
                   && !parentGroupId && !clipToLayerBelow && !initialCanvasSize
                   && !wobbleAmount && !motion && strokes.isEmpty();
        }
    };

    struct IndexedLayer
    {
        int index = -1;
        Layer layer;
    };

    std::optional<ValueChange<QSize>> size;
    std::optional<ValueChange<QColor>> background;
    std::optional<ValueChange<int>> animationFrames;
    std::optional<ValueChange<qreal>> framesPerSecond;
    std::optional<ValueChange<qreal>> wobbleAmount;
    std::optional<ValueChange<MotionSettings>> motionSettings;
    std::optional<ValueChange<QMap<QString, RasterAsset>>> rasterAssets;
    std::optional<ValueChange<QUuid>> activeLayerId;
    QVector<IndexedLayer> removedLayers;
    QVector<IndexedLayer> addedLayers;
    QVector<LayerChange> changedLayers;
    QVector<QUuid> beforeLayerOrder;
    QVector<QUuid> afterLayerOrder;

    static DocumentDelta between(const Document &before, const Document &after);

    // Fast path for the dominant drawing gesture. Returns an empty delta when
    // the two states differ by anything other than one stroke appended to
    // `layerId`, so the caller must fall back to `between` on an empty result.
    static DocumentDelta appendedStroke(
        const Document &before, const Document &after, const QUuid &layerId);

    bool isEmpty() const;

    // Returns false when the delta does not match `document`, leaving it
    // partially modified; the caller must discard it rather than use it.
    bool apply(Document &document, bool forward) const;

    bool mergeScalar(
        const DocumentDelta &next, int mergeId, const QUuid &scope);
    void normalizeMergedChanges();
    QVector<Stroke> payloadStrokes(bool targetAfter) const;
    void accountStorage(MemoryFootprint &footprint) const;
    StorageStats storageStats() const;
};

}

}

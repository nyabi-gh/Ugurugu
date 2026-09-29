// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#pragma once

#include "document/Document.hpp"
#include "document/history/HistoryMemory.hpp"

#include <QPointF>
#include <QSize>
#include <QTransform>
#include <QUuid>
#include <QVector>

#include <optional>
#include <variant>

namespace ugurugu
{
namespace history
{

// UI side effects a history entry must replay alongside the document change,
// recorded once at commit so undo and redo emit the exact same transition with
// the two sides swapped rather than re-deriving it from the new state.
//
// The target document is installed before any of these signals: slots may
// query or modify the controller synchronously. Before-events fire between
// that installation and documentChanged, so consumers translating cached
// geometry (selection masks, live lasso points) run against the new document
// ahead of the general refresh; after-events fire once documentChanged has.
// DocumentHistoryTests::installsTargetStateBeforeHistoryEffectSignals pins
// this ordering.
struct HistoryEffects
{
    struct CanvasResize
    {
        QSize beforeSize;
        QSize afterSize;
        QTransform forwardTransform;
        QTransform reverseTransform;
    };

    struct StrokeTransform
    {
        QUuid layerId;
        QVector<QUuid> strokeIds;
        QTransform forwardTransform;
        QTransform reverseTransform;
    };

    struct StrokeDuplicate
    {
        QUuid layerId;
        QVector<QUuid> sourceIds;
        QVector<QUuid> duplicateIds;
        QPointF delta;
    };

    struct SelectionOverlay
    {
        QUuid layerId;
        QVector<QUuid> beforeIds;
        QVector<QUuid> afterIds;
        std::optional<PackedMaskRegion> beforeMask;
        std::optional<PackedMaskRegion> afterMask;
    };

    struct StrokePresence
    {
        QUuid layerId;
        QUuid strokeId;
        std::optional<PackedMaskRegion> clipMask;
    };

    struct LayerThumbnail
    {
        QUuid layerId;
    };

    struct LayerThumbnailsReset
    {
    };

    struct ActiveLayer
    {
    };

    struct SelectionState
    {
        QUuid layerId;
        std::optional<PackedMaskRegion> mask;
    };

    struct SelectionStateTransition
    {
        SelectionState before;
        SelectionState after;
    };

    using BeforeEvent = std::variant<CanvasResize,
        StrokeTransform,
        StrokeDuplicate,
        SelectionOverlay,
        StrokePresence>;
    using AfterEvent =
        std::variant<LayerThumbnail, LayerThumbnailsReset, ActiveLayer>;

    QVector<BeforeEvent> beforeDocumentChanged;
    QVector<AfterEvent> afterDocumentChanged;
    std::optional<SelectionStateTransition> selectionState;

    static bool sameSelectionState(
        const SelectionState &left, const SelectionState &right);

    bool isEmpty() const;
    bool hasDocumentEffects() const;
    bool hasSelectionTransition() const;

    // Deep-copies every payload so the result shares no backing with the
    // buffers it was built from. Only a frozen copy may be stored in history:
    // an implicitly shared one would keep a caller allocation alive and would
    // report another entry's bytes under the same backing address, which the
    // address-keyed memory accounting counts only once.
    HistoryEffects frozenCopy() const;

    void append(const HistoryEffects &later);
    void discardDocumentEffects();
    void accountStorage(MemoryFootprint &footprint) const;
};

}

}

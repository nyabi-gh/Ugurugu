// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "document/history/HistoryEffects.hpp"

#include <type_traits>

namespace ugurugu
{
namespace history
{

namespace
{

QVector<QUuid> owningIds(const QVector<QUuid> &source)
{
    QVector<QUuid> copy;
    copy.reserve(source.size());
    for (const QUuid &id : source)
    {
        copy.append(id);
    }
    return copy;
}

std::optional<PackedMaskRegion> owningMask(
    const std::optional<PackedMaskRegion> &source)
{
    if (!source)
    {
        return std::nullopt;
    }
    PackedMaskRegion copy = *source;
    copy.packedMask =
        QByteArray(source->packedMask.constData(), source->packedMask.size());
    return copy;
}

}

bool HistoryEffects::sameSelectionState(
    const SelectionState &left, const SelectionState &right)
{
    return left.layerId == right.layerId && left.mask == right.mask;
}

bool HistoryEffects::isEmpty() const
{
    return beforeDocumentChanged.isEmpty() && afterDocumentChanged.isEmpty()
           && !selectionState;
}

bool HistoryEffects::hasDocumentEffects() const
{
    return !beforeDocumentChanged.isEmpty() || !afterDocumentChanged.isEmpty();
}

bool HistoryEffects::hasSelectionTransition() const
{
    return selectionState
           && !sameSelectionState(
               selectionState->before, selectionState->after);
}

HistoryEffects HistoryEffects::frozenCopy() const
{
    HistoryEffects frozen = *this;
    for (BeforeEvent &event : frozen.beforeDocumentChanged)
    {
        std::visit(
            [](auto &value)
            {
                using T = std::decay_t<decltype(value)>;
                if constexpr (std::is_same_v<T, StrokeTransform>)
                {
                    value.strokeIds = owningIds(value.strokeIds);
                }
                else if constexpr (std::is_same_v<T, StrokeDuplicate>)
                {
                    value.sourceIds = owningIds(value.sourceIds);
                    value.duplicateIds = owningIds(value.duplicateIds);
                }
                else if constexpr (std::is_same_v<T, SelectionOverlay>)
                {
                    value.beforeIds = owningIds(value.beforeIds);
                    value.afterIds = owningIds(value.afterIds);
                    value.beforeMask = owningMask(value.beforeMask);
                    value.afterMask = owningMask(value.afterMask);
                }
                else if constexpr (std::is_same_v<T, StrokePresence>)
                {
                    value.clipMask = owningMask(value.clipMask);
                }
            },
            event);
    }
    if (frozen.selectionState)
    {
        frozen.selectionState->before.mask =
            owningMask(frozen.selectionState->before.mask);
        frozen.selectionState->after.mask =
            owningMask(frozen.selectionState->after.mask);
    }
    return frozen;
}

void HistoryEffects::append(const HistoryEffects &later)
{
    beforeDocumentChanged += later.beforeDocumentChanged;
    afterDocumentChanged += later.afterDocumentChanged;
    if (later.selectionState)
    {
        if (!selectionState)
        {
            selectionState = later.selectionState;
        }
        else
        {
            selectionState->after = later.selectionState->after;
        }
        if (!hasSelectionTransition())
        {
            selectionState.reset();
        }
    }
}

void HistoryEffects::discardDocumentEffects()
{
    beforeDocumentChanged.clear();
    afterDocumentChanged.clear();
}

void HistoryEffects::accountStorage(MemoryFootprint &footprint) const
{
    footprint.addOwned(static_cast<qint64>(beforeDocumentChanged.capacity())
                           * static_cast<qint64>(sizeof(BeforeEvent))
                       + static_cast<qint64>(afterDocumentChanged.capacity())
                             * static_cast<qint64>(sizeof(AfterEvent))
                       + static_cast<qint64>(sizeof(HistoryEffects)));
    for (const BeforeEvent &event : beforeDocumentChanged)
    {
        std::visit(
            [&footprint](const auto &value)
            {
                using T = std::decay_t<decltype(value)>;
                if constexpr (std::is_same_v<T, StrokeTransform>)
                {
                    accountVectorStorage(footprint,
                        value.strokeIds,
                        QStringLiteral("effect-transform-ids"));
                }
                else if constexpr (std::is_same_v<T, StrokeDuplicate>)
                {
                    accountVectorStorage(footprint,
                        value.sourceIds,
                        QStringLiteral("effect-source-ids"));
                    accountVectorStorage(footprint,
                        value.duplicateIds,
                        QStringLiteral("effect-duplicate-ids"));
                }
                else if constexpr (std::is_same_v<T, SelectionOverlay>)
                {
                    accountVectorStorage(footprint,
                        value.beforeIds,
                        QStringLiteral("effect-before-ids"));
                    accountVectorStorage(footprint,
                        value.afterIds,
                        QStringLiteral("effect-after-ids"));
                    accountPackedMask(footprint, value.beforeMask);
                    accountPackedMask(footprint, value.afterMask);
                }
                else if constexpr (std::is_same_v<T, StrokePresence>)
                {
                    accountPackedMask(footprint, value.clipMask);
                }
            },
            event);
    }
    if (selectionState)
    {
        accountPackedMask(footprint, selectionState->before.mask);
        accountPackedMask(footprint, selectionState->after.mask);
    }
}

}

}

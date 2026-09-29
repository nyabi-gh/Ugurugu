// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "document/DocumentController.hpp"
#include "document/history/HistoryMemory.hpp"
#include "document/history/LogicalHistoryCommand.hpp"

#include <QScopedValueRollback>

#include <algorithm>
#include <memory>
#include <vector>

namespace ugurugu
{

using history::LogicalHistoryCommand;
using history::MemoryFootprint;

struct DocumentUndoStack::Impl
{
    std::vector<std::unique_ptr<LogicalHistoryCommand>> entries;
    int index = 0;
    int cleanIndex = 0;
    int undoLimit = 64;
    qsizetype peakTransientPreparedDocuments = 0;
    // Last values handed to observers, so notifyHistoryState only emits on an
    // actual change instead of on every mutation.
    bool notifiedCanUndo = false;
    bool notifiedCanRedo = false;
    QString notifiedUndoText;
    QString notifiedRedoText;
};

DocumentUndoStack::DocumentUndoStack(DocumentController *owner)
    : m_owner(owner)
    , m_impl(std::make_unique<Impl>())
{
}

DocumentUndoStack::~DocumentUndoStack() = default;

bool DocumentUndoStack::canUndo() const
{
    return m_impl && m_impl->index > 0;
}

bool DocumentUndoStack::canRedo() const
{
    return m_impl && m_impl->index < static_cast<int>(m_impl->entries.size());
}

bool DocumentUndoStack::isClean() const
{
    return m_impl && m_impl->cleanIndex >= 0
           && m_impl->index == m_impl->cleanIndex;
}

int DocumentUndoStack::count() const
{
    return m_impl ? static_cast<int>(m_impl->entries.size()) : 0;
}

int DocumentUndoStack::index() const
{
    return m_impl ? m_impl->index : 0;
}

int DocumentUndoStack::undoLimit() const
{
    return m_impl ? m_impl->undoLimit : 0;
}

void DocumentUndoStack::setUndoLimit(int limit)
{
    if (!m_impl || m_moving || hasOpenMacro() || limit < 0)
    {
        return;
    }
    m_impl->undoLimit = limit;
    enforceLimits();
    notifyHistoryState();
}

void DocumentUndoStack::setClean()
{
    if (!m_impl || m_moving || hasOpenMacro())
    {
        return;
    }
    m_impl->cleanIndex = m_impl->index;
}

void DocumentUndoStack::clear()
{
    if (!m_impl || m_moving)
    {
        return;
    }
    if (hasOpenMacro())
    {
        failOpenMacro();
        return;
    }
    m_impl->entries.clear();
    m_impl->index = 0;
    m_impl->cleanIndex = 0;
    notifyHistoryState();
}

void DocumentUndoStack::push(LogicalHistoryCommand *rawCommand)
{
    std::unique_ptr<LogicalHistoryCommand> command(rawCommand);
    if (!m_impl || !command || m_moving || hasOpenMacro())
    {
        return;
    }

    while (static_cast<int>(m_impl->entries.size()) > m_impl->index)
    {
        const int oldCount = static_cast<int>(m_impl->entries.size());
        if (m_impl->cleanIndex == oldCount)
        {
            m_impl->cleanIndex = -1;
        }
        m_impl->entries.pop_back();
    }

    const bool mayMergeAcrossCurrentBoundary =
        m_impl->cleanIndex != m_impl->index;
    QScopedValueRollback<bool> movement(m_moving, true);
    command->redo();

    bool merged = false;
    if (mayMergeAcrossCurrentBoundary && m_impl->index > 0
        && command->id() >= 0)
    {
        LogicalHistoryCommand *previous =
            m_impl->entries[static_cast<size_t>(m_impl->index - 1)].get();
        if (previous && previous->id() == command->id())
        {
            merged = previous->mergeWith(command.get());
            if (merged && previous->isObsolete())
            {
                m_impl->entries.erase(
                    m_impl->entries.begin() + (m_impl->index - 1));
                --m_impl->index;
            }
        }
    }
    if (!merged)
    {
        m_impl->entries.push_back(std::move(command));
        ++m_impl->index;
    }
    m_moving = false;
    enforceLimits();
    notifyHistoryState();
}

void DocumentUndoStack::undo()
{
    if (!m_impl || !m_owner || m_moving || hasOpenMacro() || !canUndo())
    {
        return;
    }
    QScopedValueRollback<bool> movement(m_moving, true);
    LogicalHistoryCommand *command =
        m_impl->entries[static_cast<size_t>(m_impl->index - 1)].get();
    if (!command || !m_owner->preflightHistoryMovement(command, false))
    {
        m_owner->clearHistoryPreflight(command);
        return;
    }
    m_impl->peakTransientPreparedDocuments =
        std::max(m_impl->peakTransientPreparedDocuments,
            m_owner->historyStorageStats(command).stagedPreparedDocuments);
    m_owner->applyHistoryMovement(command, false);
    --m_impl->index;
    m_owner->clearHistoryPreflight(command);
    m_moving = false;
    notifyHistoryState();
}

void DocumentUndoStack::redo()
{
    if (!m_impl || !m_owner || m_moving || hasOpenMacro() || !canRedo())
    {
        return;
    }
    QScopedValueRollback<bool> movement(m_moving, true);
    LogicalHistoryCommand *command =
        m_impl->entries[static_cast<size_t>(m_impl->index)].get();
    if (!command || !m_owner->preflightHistoryMovement(command, true))
    {
        m_owner->clearHistoryPreflight(command);
        return;
    }
    m_impl->peakTransientPreparedDocuments =
        std::max(m_impl->peakTransientPreparedDocuments,
            m_owner->historyStorageStats(command).stagedPreparedDocuments);
    m_owner->applyHistoryMovement(command, true);
    ++m_impl->index;
    m_owner->clearHistoryPreflight(command);
    m_moving = false;
    notifyHistoryState();
}

void DocumentUndoStack::beginMacro(const QString &text)
{
    if (!m_owner || m_moving)
    {
        return;
    }
    m_owner->beginHistoryMacro(text);
}

void DocumentUndoStack::endMacro()
{
    if (!m_owner || m_moving || !hasOpenMacro())
    {
        return;
    }
    m_owner->endHistoryMacro();
}

void DocumentUndoStack::failOpenMacro()
{
    if (m_owner)
    {
        m_owner->failHistoryMacro();
    }
}

bool DocumentUndoStack::hasOpenMacro() const
{
    return m_owner && m_owner->hasOpenHistoryMacro();
}

QString DocumentUndoStack::undoText() const
{
    if (!m_impl || !canUndo())
    {
        return tr("Undo");
    }
    return tr("Undo %1").arg(
        m_impl->entries[static_cast<size_t>(m_impl->index - 1)]->text());
}

QString DocumentUndoStack::redoText() const
{
    if (!m_impl || !canRedo())
    {
        return tr("Redo");
    }
    return tr("Redo %1").arg(
        m_impl->entries[static_cast<size_t>(m_impl->index)]->text());
}

void DocumentUndoStack::notifyHistoryState()
{
    if (!m_impl)
    {
        return;
    }
    // A slot may move history again from inside any of these signals, which
    // runs this notification to completion before the outer one resumes. Each
    // value is therefore read immediately before it is published: a snapshot
    // taken up front would resume as the newest state and publish - and record
    // as published - the state history has already left.
    if (const bool undoAvailable = canUndo();
        m_impl->notifiedCanUndo != undoAvailable)
    {
        m_impl->notifiedCanUndo = undoAvailable;
        emit canUndoChanged(undoAvailable);
    }
    if (const bool redoAvailable = canRedo();
        m_impl->notifiedCanRedo != redoAvailable)
    {
        m_impl->notifiedCanRedo = redoAvailable;
        emit canRedoChanged(redoAvailable);
    }
    if (const QString nextUndoText = undoText();
        m_impl->notifiedUndoText != nextUndoText)
    {
        m_impl->notifiedUndoText = nextUndoText;
        emit undoTextChanged(nextUndoText);
    }
    if (const QString nextRedoText = redoText();
        m_impl->notifiedRedoText != nextRedoText)
    {
        m_impl->notifiedRedoText = nextRedoText;
        emit redoTextChanged(nextRedoText);
    }
}

DocumentUndoStack::StorageStats DocumentUndoStack::storageStats() const
{
    StorageStats total;
    if (!m_impl)
    {
        return total;
    }
    MemoryFootprint footprint;
    total.entryCount = static_cast<qsizetype>(m_impl->entries.size());
    total.peakTransientPreparedDocuments =
        m_impl->peakTransientPreparedDocuments;
    if (m_owner)
    {
        total.macroPreparedDocuments = m_owner->macroPreparedDocumentCount();
    }
    for (const std::unique_ptr<LogicalHistoryCommand> &entry : m_impl->entries)
    {
        const StorageStats command = entry->storageStats();
        total.retainedLayers += command.retainedLayers;
        total.retainedStrokes += command.retainedStrokes;
        total.retainedPreparedDocuments += command.retainedPreparedDocuments;
        total.stagedPreparedDocuments += command.stagedPreparedDocuments;
        entry->accountStorage(footprint);
    }
    total.retainedBytes = footprint.totalBytes();
    total.residentBudgetSoftExceeded =
        total.entryCount == 1 && total.retainedBytes > m_maximumResidentBytes;
    return total;
}

void DocumentUndoStack::enforceLimits()
{
    if (!m_impl)
    {
        return;
    }
    while (m_impl->entries.size() > 1)
    {
        const StorageStats stats = storageStats();
        const bool overCount =
            m_impl->undoLimit > 0
            && static_cast<int>(m_impl->entries.size()) > m_impl->undoLimit;
        const bool overBytes = stats.retainedBytes > m_maximumResidentBytes;
        if (!overCount && !overBytes)
        {
            break;
        }

        const int count = static_cast<int>(m_impl->entries.size());
        const int undoDistance = m_impl->index;
        const int redoDistance = count - m_impl->index;
        const bool removePrefix =
            undoDistance > 0
            && (redoDistance == 0 || undoDistance >= redoDistance);
        if (removePrefix)
        {
            m_impl->entries.erase(m_impl->entries.begin());
            --m_impl->index;
            if (m_impl->cleanIndex == 0)
            {
                m_impl->cleanIndex = -1;
            }
            else if (m_impl->cleanIndex > 0)
            {
                --m_impl->cleanIndex;
            }
        }
        else
        {
            const int oldCount = count;
            m_impl->entries.pop_back();
            if (m_impl->cleanIndex == oldCount)
            {
                m_impl->cleanIndex = -1;
            }
        }
    }
}
}

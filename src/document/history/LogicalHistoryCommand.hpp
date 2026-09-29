// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#pragma once

#include "document/history/HistoryMemory.hpp"
#include "document/history/HistoryTypes.hpp"

#include <QString>

#include <utility>

namespace ugurugu
{
namespace history
{

// Base for every entry DocumentUndoStack owns.
//
// Deliberately not a QUndoCommand. The document core links only QtCore and
// QtGui so it can be reused where QtWidgets is unavailable, and the stack
// already implements its own cursor, merging and limits rather than deferring
// to QUndoStack.
//
// `preflight` must build the complete target state and return false if it
// cannot, before `redo`/`undo` touch anything; the stack only moves its cursor
// after a successful preflight, so a rejected movement leaves the document and
// its UI side effects untouched. `clearPreflight` releases whatever preflight
// staged, and must be safe to call whether or not preflight succeeded.
class LogicalHistoryCommand
{
public:
    explicit LogicalHistoryCommand(QString text = {})
        : m_text(std::move(text))
    {
    }

    virtual ~LogicalHistoryCommand() = default;

    LogicalHistoryCommand(const LogicalHistoryCommand &) = delete;
    LogicalHistoryCommand &operator=(const LogicalHistoryCommand &) = delete;

    const QString &text() const
    {
        return m_text;
    }

    // Two adjacent entries may merge only when both report the same
    // non-negative id. The default never merges.
    virtual int id() const
    {
        return -1;
    }

    virtual bool mergeWith(const LogicalHistoryCommand *)
    {
        return false;
    }

    // Set by a merge that leaves nothing to apply. The stack drops an obsolete
    // entry instead of keeping an undo step that would do nothing.
    bool isObsolete() const
    {
        return m_obsolete;
    }

    void setObsolete(bool obsolete)
    {
        m_obsolete = obsolete;
    }

    virtual void redo() = 0;
    virtual void undo() = 0;
    virtual bool preflight(bool forward) = 0;
    virtual void clearPreflight() = 0;
    virtual StorageStats storageStats() const = 0;
    virtual void accountStorage(MemoryFootprint &footprint) const = 0;

private:
    QString m_text;
    bool m_obsolete = false;
};

}

}

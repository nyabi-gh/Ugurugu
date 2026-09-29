// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#pragma once

#include "document/DocumentController.hpp"
#include "io/DocumentSerializer.hpp"

#include <QtGlobal>

namespace ugurugu
{

class DocumentControllerTestAccess final
{
public:
    static void failNextDocumentReplacementPreparation(
        DocumentController &controller)
    {
        controller.m_failNextDocumentReplacementPreparationForTesting = true;
    }

    static void failHistoryPrepareAfter(
        DocumentController &controller, int successfulStages)
    {
        controller.m_historyPrepareFailureCountdownForTesting =
            successfulStages;
    }

    static quint64 contentRevision(const DocumentController &controller)
    {
        return controller.m_currentContentRevision;
    }

    static quint64 savedContentRevision(const DocumentController &controller)
    {
        return controller.m_savedContentRevision;
    }

    static quint64 nextContentRevision(const DocumentController &controller)
    {
        return controller.m_nextContentRevision;
    }

    static void resetSerializationStats(DocumentController &controller)
    {
        controller.m_serializationCache.resetStats();
    }

    static DocumentSerializer::SerializationCache::Stats serializationStats(
        const DocumentController &controller)
    {
        return controller.m_serializationCache.stats();
    }

    static quint64 historyNode(const DocumentController &controller)
    {
        return controller.m_currentHistoryNode;
    }

    static quint64 nextHistoryNode(const DocumentController &controller)
    {
        return controller.m_nextHistoryNode;
    }

    static const void *stateIdentity(const DocumentController &controller)
    {
        return controller.m_currentState.get();
    }

    static void setHistoryResidentLimit(
        DocumentController &controller, qint64 bytes)
    {
        controller.m_undoStack.m_maximumResidentBytes = bytes;
        controller.m_undoStack.enforceLimits();
    }
};

}

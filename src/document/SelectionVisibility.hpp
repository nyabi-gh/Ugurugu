// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#pragma once

#include "document/Document.hpp"

#include <atomic>

namespace ugurugu
{

class SelectionVisibility final
{
public:
    struct Result
    {
        bool hasVisiblePixels = false;
        bool renderSucceeded = false;
        int renderedFrames = 0;
        quint64 renderedPixels = 0;
        quint64 maximumExplicitImageBytes = 0;
    };

    struct EditableStrokeStats
    {
        quint64 fullCanvasFallbacks = 0;
        quint64 regionalRenders = 0;
        quint64 pixelSelectionOperationsReplayed = 0;
        quint64 reframeOperationsReplayed = 0;
        quint64 eraseOperationsReplayed = 0;
        quint64 effectCandidatesExamined = 0;
        quint64 maximumExplicitImageBytes = 0;
    };

    // An animated layer is inspected frame by frame over the whole animation,
    // so callers that can be superseded pass a cancellation flag, which must
    // outlive the call. It is read before each frame; a cancelled run returns
    // with renderSucceeded false so no caller caches an unfinished answer.
    static Result evaluate(const Document &document,
        const Layer &layer,
        const QImage &selectionMask,
        int preferredFrame = 0,
        const std::atomic_bool *cancelled = nullptr);
    static QVector<QUuid> editableStrokeIds(const Document &document,
        const Layer &layer,
        const QImage &selectionMask,
        int frameIndex,
        EditableStrokeStats *stats = nullptr);
};

}

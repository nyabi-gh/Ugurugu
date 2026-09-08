// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "io/SelectionClipboardCodec.hpp"
#include "wasm/BridgeDocument.hpp"

#include <QVector>

using namespace ugurugu::wasm;

namespace
{

// Outside BridgeDocument on purpose: the desktop's clipboard belongs to the
// process, not to the document, so a copy has to survive opening another file.
QByteArray &clipboardPayload()
{
    static QByteArray payload;
    return payload;
}

// The desktop hands removeSelectedContent every stroke on the layer and lets
// it decide which the mask covers. Same call, same definition of "selected".
QVector<QUuid> strokeIdsOf(const ugurugu::Layer &layer)
{
    QVector<QUuid> ids;
    ids.reserve(layer.strokes.size());
    for (const ugurugu::Stroke &stroke : layer.strokes)
    {
        ids.append(stroke.id);
    }
    return ids;
}

bool captureSelection(BridgeDocument *handle,
    int frame,
    ugurugu::SelectionClipboardCodec::Copy *out)
{
    if (handle->transformActive)
    {
        setError(StatusStrokeRejected,
            QByteArrayLiteral("apply or cancel the selection transform first"));
        return false;
    }
    const ugurugu::Document &document = handle->controller->document();
    const QUuid layerId = paintTargetLayer(document);
    if (!selectionAppliesTo(handle, layerId))
    {
        setError(StatusNoSelection,
            QByteArrayLiteral("select an area on this layer first"));
        return false;
    }
    QString error;
    std::optional<ugurugu::SelectionClipboardCodec::Copy> copy =
        ugurugu::SelectionClipboardCodec::makeCopy(
            document, layerId, handle->selectionMask, frame, &error);
    if (!copy)
    {
        setError(StatusEmptyRegion,
            error.isEmpty() ? QByteArrayLiteral("the selection is empty")
                            : error.toUtf8());
        return false;
    }
    *out = std::move(*copy);
    return true;
}

int reportPaste(ugurugu::DocumentController::PasteLayerResult result)
{
    using Result = ugurugu::DocumentController::PasteLayerResult;
    if (result == Result::Pasted)
    {
        clearError();
        return 1;
    }
    switch (result)
    {
    case Result::RejectedLayerLimit:
        setError(StatusDocumentInvalid,
            QByteArrayLiteral("the document already has the maximum "
                              "number of layers"));
        break;
    case Result::RejectedStrokeLimit:
        setError(StatusDocumentInvalid,
            QByteArrayLiteral("pasting would exceed the stroke limit"));
        break;
    case Result::RejectedPointLimit:
        setError(StatusDocumentInvalid,
            QByteArrayLiteral("pasting would exceed the point limit"));
        break;
    case Result::RejectedMaskLimit:
        setError(StatusDocumentInvalid,
            QByteArrayLiteral("pasting would exceed the mask budget"));
        break;
    default:
        setError(StatusDocumentInvalid,
            QByteArrayLiteral("the clipboard content could not be pasted"));
        break;
    }
    return 0;
}

}

extern "C"
{
    EMSCRIPTEN_KEEPALIVE int ugu_clipboard_has()
    {
        return clipboardPayload().isEmpty() ? 0 : 1;
    }

    // Copies the selection and, like CanvasWidget::copySelection, drops the
    // copy on a new layer offset from the original so it can be dragged away
    // immediately. The shell arms its move mode on success.
    EMSCRIPTEN_KEEPALIVE int ugu_selection_copy(
        BridgeDocument *handle, int frame)
    {
        ugurugu::SelectionClipboardCodec::Copy copy;
        if (!captureSelection(handle, frame, &copy))
        {
            return 0;
        }
        clipboardPayload() = copy.payload;
        const int pasted =
            reportPaste(handle->controller->pasteLayer(std::move(copy.layer),
                copy.canvasSize,
                QPointF(12.0, 12.0),
                handle->selectionMask,
                copy.rasterAssets));
        if (pasted == 1)
        {
            invalidateSplit(handle);
            renderCommittedFrame(handle, frame);
        }
        return pasted;
    }

    EMSCRIPTEN_KEEPALIVE int ugu_selection_cut(
        BridgeDocument *handle, int frame)
    {
        ugurugu::SelectionClipboardCodec::Copy copy;
        if (!captureSelection(handle, frame, &copy))
        {
            return 0;
        }
        const ugurugu::Document &document = handle->controller->document();
        const QUuid layerId = paintTargetLayer(document);
        const ugurugu::Layer *layer = document.layer(layerId);
        if (layer == nullptr)
        {
            setError(StatusNoPaintLayer,
                QByteArrayLiteral("document has no paint layer"));
            return 0;
        }
        clipboardPayload() = copy.payload;
        ugurugu::DocumentUndoStack *undoStack = handle->controller->undoStack();
        undoStack->beginMacro(QStringLiteral("Cut selection"));
        const bool removed = handle->controller->removeSelectedContent(
            layerId, strokeIdsOf(*layer), handle->selectionMask);
        undoStack->endMacro();
        if (!removed)
        {
            setError(StatusEmptyRegion,
                QByteArrayLiteral("there is no content in the selected area"));
            return 0;
        }
        // The selection stays where it was, as it does on the desktop: the
        // hole is still the region the artist is working on.
        invalidateSplit(handle);
        renderCommittedFrame(handle, frame);
        clearError();
        return 1;
    }

    EMSCRIPTEN_KEEPALIVE int ugu_clipboard_paste(
        BridgeDocument *handle, int frame)
    {
        if (clipboardPayload().isEmpty())
        {
            setError(StatusEmptyRegion,
                QByteArrayLiteral("there is nothing to paste"));
            return 0;
        }
        QString error;
        const std::optional<ugurugu::SelectionClipboardCodec::Pasted> pasted =
            ugurugu::SelectionClipboardCodec::decode(
                clipboardPayload(), &error);
        if (!pasted)
        {
            setError(StatusDocumentInvalid,
                error.isEmpty()
                    ? QByteArrayLiteral(
                          "the clipboard content could not be pasted")
                    : error.toUtf8());
            return 0;
        }
        const int result = reportPaste(handle->controller->pasteLayer(
            pasted->layer, pasted->canvasSize, {}, {}, pasted->rasterAssets));
        if (result == 1)
        {
            invalidateSplit(handle);
            renderCommittedFrame(handle, frame);
        }
        return result;
    }
}

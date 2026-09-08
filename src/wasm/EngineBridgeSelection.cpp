// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "document/DocumentOperations.hpp"
#include "document/StrokeMask.hpp"
#include "wasm/BridgeDocument.hpp"

#include <QPointF>
#include <QRectF>
#include <QSizeF>

#include <algorithm>
#include <utility>

using namespace ugurugu::wasm;

extern "C"
{
    // points is a flat x, y array of `count` document-space points: the path
    // for a freehand lasso, or the two drag corners for a rectangle or
    // ellipse. paint 1 fills the area instead of selecting it, which is the
    // desktop lasso's Paint mode.
    EMSCRIPTEN_KEEPALIVE int ugu_selection_shape(BridgeDocument *handle,
        int frame,
        int shape,
        const double *points,
        int count,
        int combine,
        int paint)
    {
        const ugurugu::Document &document = handle->controller->document();
        const QUuid layerId = paintTargetLayer(document);
        if (layerId.isNull())
        {
            setError(StatusNoPaintLayer,
                QByteArrayLiteral("document has no paint layer"));
            return 0;
        }
        const QImage mask = maskFromShape(document.size, shape, points, count);
        if (mask.isNull())
        {
            setError(StatusEmptyRegion,
                QByteArrayLiteral("the drawn area is too small to use"));
            return 0;
        }
        if (paint != 0)
        {
            return commitFrozenFill(handle, mask, frame);
        }
        applySelectionCombine(handle, mask, layerId, combine);
        clearError();
        return 1;
    }

    EMSCRIPTEN_KEEPALIVE int ugu_selection_flood(
        BridgeDocument *handle, int frame, double x, double y, int combine)
    {
        const ugurugu::Document &document = handle->controller->document();
        const QSize size = document.size;
        if (!QRectF(QPointF(0.0, 0.0), QSizeF(size)).contains(QPointF(x, y)))
        {
            setError(StatusInvalidArgument,
                QByteArrayLiteral("the point is outside the canvas"));
            return 0;
        }
        const QUuid layerId = paintTargetLayer(document);
        if (layerId.isNull())
        {
            setError(StatusNoPaintLayer,
                QByteArrayLiteral("document has no paint layer"));
            return 0;
        }
        const QImage reference = referenceImage(handle, frame, layerId);
        if (reference.isNull())
        {
            setError(StatusEmptyRegion,
                handle->fillReference == ReferenceMarkedLayers
                    ? QByteArrayLiteral("mark a visible paint layer as a "
                                        "reference first")
                    : QByteArrayLiteral("the reference image could not be "
                                        "rendered"));
            return 0;
        }
        const QPoint seed(std::clamp(static_cast<int>(x), 0, size.width() - 1),
            std::clamp(static_cast<int>(y), 0, size.height() - 1));
        const QImage mask = ugurugu::FloodFillMask::fromImage(
            reference, seed, handle->fillComparison, handle->fillTolerance);
        if (mask.isNull())
        {
            setError(StatusEmptyRegion,
                QByteArrayLiteral("click an area enclosed by lines to select "
                                  "it"));
            return 0;
        }
        applySelectionCombine(handle, mask, layerId, combine);
        clearError();
        return 1;
    }

    EMSCRIPTEN_KEEPALIVE int ugu_selection_all(BridgeDocument *handle)
    {
        const ugurugu::Document &document = handle->controller->document();
        const QUuid layerId = paintTargetLayer(document);
        if (layerId.isNull())
        {
            setError(StatusNoPaintLayer,
                QByteArrayLiteral("document has no paint layer"));
            return 0;
        }
        QImage mask(document.size, QImage::Format_Grayscale8);
        if (mask.isNull())
        {
            setError(StatusOutOfMemory,
                QByteArrayLiteral("the selection mask could not be allocated"));
            return 0;
        }
        mask.fill(255);
        pushSelectionChange(handle, mask, layerId, "Select all");
        clearError();
        return 1;
    }

    EMSCRIPTEN_KEEPALIVE int ugu_selection_invert(BridgeDocument *handle)
    {
        if (handle->selectionMask.isNull())
        {
            setError(StatusNoSelection,
                QByteArrayLiteral("there is no selection to invert"));
            return 0;
        }
        QImage inverted = handle->selectionMask;
        inverted.invertPixels();
        pushSelectionChange(
            handle, inverted, handle->selectionLayerId, "Invert selection");
        clearError();
        return 1;
    }

    EMSCRIPTEN_KEEPALIVE void ugu_selection_clear(BridgeDocument *handle)
    {
        if (handle->selectionMask.isNull())
        {
            return;
        }
        pushSelectionChange(handle, QImage(), QUuid(), "Deselect");
    }

    EMSCRIPTEN_KEEPALIVE int ugu_selection_fill(
        BridgeDocument *handle, int frame)
    {
        const QUuid layerId = paintTargetLayer(handle->controller->document());
        if (!selectionAppliesTo(handle, layerId))
        {
            setError(StatusNoSelection,
                QByteArrayLiteral("select an area on this layer to fill"));
            return 0;
        }
        // Held by value: committing the fill may replace the selection, and
        // the fill clips to the same mask so it lands exactly inside the
        // marching ants instead of bleeding a pixel past them.
        const QImage mask = handle->selectionMask;
        return commitFrozenFill(handle, mask, frame);
    }

    EMSCRIPTEN_KEEPALIVE int ugu_selection_delete(
        BridgeDocument *handle, int frame)
    {
        const ugurugu::Document &document = handle->controller->document();
        const QUuid layerId = paintTargetLayer(document);
        if (!selectionAppliesTo(handle, layerId))
        {
            setError(StatusNoSelection,
                QByteArrayLiteral("select an area on this layer to delete"));
            return 0;
        }
        const ugurugu::Layer *layer = document.layer(layerId);
        if (layer == nullptr)
        {
            setError(StatusNoPaintLayer,
                QByteArrayLiteral("document has no paint layer"));
            return 0;
        }
        // The desktop offers every stroke on the layer to the controller and
        // lets removeSelectedContent decide which ones the mask actually
        // covers; matching that keeps one definition of "selected content".
        QVector<QUuid> strokeIds;
        strokeIds.reserve(layer->strokes.size());
        for (const ugurugu::Stroke &stroke : layer->strokes)
        {
            strokeIds.append(stroke.id);
        }
        const bool removed = handle->controller->removeSelectedContent(
            layerId, strokeIds, handle->selectionMask);
        if (!removed)
        {
            setError(StatusEmptyRegion,
                QByteArrayLiteral("there is no content in the selected area"));
            return 0;
        }
        installSelection(handle, QImage(), QUuid());
        invalidateSplit(handle);
        renderCommittedFrame(handle, frame);
        clearError();
        return 1;
    }

    // Opens a floating transform on the current selection. Until it is applied
    // or cancelled the frame shows the selected pixels lifted off the layer and
    // drawn back through the pending matrix; the document itself is untouched,
    // so the whole gesture costs one undo entry.
    EMSCRIPTEN_KEEPALIVE int ugu_selection_transform_begin(
        BridgeDocument *handle, int frame)
    {
        if (handle->transformActive)
        {
            clearError();
            return 1;
        }
        const ugurugu::Document &document = handle->controller->document();
        const QUuid layerId = paintTargetLayer(document);
        if (!selectionAppliesTo(handle, layerId))
        {
            setError(StatusNoSelection,
                QByteArrayLiteral("select an area on this layer to move it"));
            return 0;
        }
        const ugurugu::Layer *layer = document.layer(layerId);
        if (layer == nullptr)
        {
            setError(StatusNoPaintLayer,
                QByteArrayLiteral("document has no paint layer"));
            return 0;
        }
        // Same guard the stroke tools carry: moving pixels on a layer nobody
        // can see is work the artist would never find again.
        if (!layer->visible || layer->opacity <= 0.0
            || !ugurugu::DocumentOperations::isLayerRenderable(
                document, *layer))
        {
            setError(StatusLayerNotDrawable,
                QByteArrayLiteral("the selected layer is hidden; make it "
                                  "visible to move the selection"));
            return 0;
        }
        const std::optional<ugurugu::PixelSelectionOp> operation =
            ugurugu::makePixelSelectionOp(
                handle->selectionMask, QTransform(), true, true);
        if (!operation)
        {
            setError(StatusEmptyRegion,
                QByteArrayLiteral("the selection could not be lifted"));
            return 0;
        }
        if (!handle->renderedFrameCommitted
            || handle->renderedFrameIndex != frame
            || handle->renderedFrame.size() != document.size)
        {
            renderCommittedFrame(handle, frame);
        }
        handle->transformActive = true;
        handle->transformLayerId = layerId;
        handle->transformSourceMask = handle->selectionMask;
        handle->transformOperation = *operation;
        handle->transformMatrix = QTransform();
        handle->transformFrame = frame;
        handle->transformHistoryIndex =
            handle->controller->undoStack()->index();
        // Shares with renderedFrame until the first preview paints and detaches
        // it, so opening a session copies nothing.
        handle->transformBaseFrame = handle->renderedFrame;
        handle->transformPreviewRegion = QRect();
        handle->dirty = QRect();
        clearError();
        return 1;
    }

    // Replaces the pending matrix and redraws the floating pixels. Rejecting an
    // unusable matrix leaves the previous preview standing, so a gesture that
    // wanders out of range simply stops following instead of tearing.
    EMSCRIPTEN_KEEPALIVE int ugu_selection_transform_update(
        BridgeDocument *handle,
        double m11,
        double m12,
        double m21,
        double m22,
        double dx,
        double dy)
    {
        if (!handle->transformActive)
        {
            setError(StatusInvalidArgument,
                QByteArrayLiteral("no selection transform is in progress"));
            return 0;
        }
        if (!std::isfinite(m11) || !std::isfinite(m12) || !std::isfinite(m21)
            || !std::isfinite(m22) || !std::isfinite(dx) || !std::isfinite(dy))
        {
            setError(StatusInvalidArgument,
                QByteArrayLiteral("the selection transform is not finite"));
            return 0;
        }
        const QTransform transform(m11, m12, m21, m22, dx, dy);
        ugurugu::PixelSelectionOp candidate = handle->transformOperation;
        candidate.transform = transform;
        candidate.sampling = ugurugu::samplingForSelectionTransform(transform);
        if (!ugurugu::isValidPixelSelectionOp(candidate))
        {
            setError(StatusInvalidArgument,
                QByteArrayLiteral("the selection transform is out of range"));
            return 0;
        }
        handle->transformOperation = candidate;
        handle->transformMatrix = transform;
        renderSelectionTransformPreview(handle);
        clearError();
        return 1;
    }

    // Commits the pending matrix as one undoable operation and moves the ants
    // with the pixels.
    EMSCRIPTEN_KEEPALIVE int ugu_selection_transform_apply(
        BridgeDocument *handle)
    {
        if (!handle->transformActive)
        {
            setError(StatusInvalidArgument,
                QByteArrayLiteral("no selection transform is in progress"));
            return 0;
        }
        const int frame = handle->transformFrame;
        // Undo, a layer edit or another tool ran while the session was open, so
        // the captured pixels no longer describe this document. Drop the
        // session and show what the document actually holds.
        if (handle->controller->undoStack()->index()
            != handle->transformHistoryIndex)
        {
            clearSelectionTransform(handle);
            invalidateSplit(handle);
            renderCommittedFrame(handle, frame);
            setError(StatusInvalidArgument,
                QByteArrayLiteral("the document changed while the selection "
                                  "was being moved"));
            return 0;
        }
        // A gesture that ends where it started is not a document change; the
        // desktop drops the session rather than pushing an empty entry.
        if (handle->transformMatrix.isIdentity())
        {
            restoreSelectionTransformBase(handle);
            clearError();
            return 1;
        }
        const ugurugu::Document &document = handle->controller->document();
        const QUuid layerId = handle->transformLayerId;
        const ugurugu::Layer *layer = document.layer(layerId);
        if (layer == nullptr)
        {
            restoreSelectionTransformBase(handle);
            setError(StatusNoPaintLayer,
                QByteArrayLiteral("the layer the selection came from is gone"));
            return 0;
        }
        // The desktop offers every stroke on the layer and lets the controller
        // decide which ones the mask covers; matching that keeps one definition
        // of "selected content".
        QVector<QUuid> strokeIds;
        strokeIds.reserve(layer->strokes.size());
        for (const ugurugu::Stroke &stroke : layer->strokes)
        {
            strokeIds.append(stroke.id);
        }
        const QTransform transform = handle->transformMatrix;
        const QImage sourceMask = handle->transformSourceMask;
        const ugurugu::SamplingMode sampling =
            handle->transformOperation.sampling;
        if (!handle->controller->transformSelection(
                layerId, strokeIds, transform, sourceMask, sampling))
        {
            restoreSelectionTransformBase(handle);
            setError(StatusEmptyRegion,
                QByteArrayLiteral("there is no content in the selected area"));
            return 0;
        }
        // The ants follow the pixels, the same support mask
        // CanvasWidget::transformSelectionOverlay installs after the commit. An
        // empty result means the content left the canvas, which drops the
        // selection instead of leaving ants around nothing.
        installSelection(handle,
            ugurugu::transformedSelectionSupport(
                sourceMask, sourceMask.size(), transform, sampling),
            layerId);
        clearSelectionTransform(handle);
        invalidateSplit(handle);
        renderCommittedFrame(handle, frame);
        clearError();
        return 1;
    }

    EMSCRIPTEN_KEEPALIVE void ugu_selection_transform_cancel(
        BridgeDocument *handle)
    {
        if (!handle->transformActive)
        {
            return;
        }
        restoreSelectionTransformBase(handle);
    }

    EMSCRIPTEN_KEEPALIVE int ugu_selection_transform_active(
        const BridgeDocument *handle)
    {
        return handle->transformActive ? 1 : 0;
    }

    EMSCRIPTEN_KEEPALIVE int ugu_selection_active(const BridgeDocument *handle)
    {
        return handle->selectionMask.isNull() ? 0 : 1;
    }

    EMSCRIPTEN_KEEPALIVE int ugu_selection_revision(
        const BridgeDocument *handle)
    {
        return handle->selectionRevision;
    }

    // Flat float buffer of closed contours in document coordinates: a vertex
    // count, then that many x, y pairs, repeated. Valid until the next
    // selection change or close on the same handle.
    EMSCRIPTEN_KEEPALIVE const float *ugu_selection_outline(
        const BridgeDocument *handle)
    {
        return handle->outlinePoints.constData();
    }

    EMSCRIPTEN_KEEPALIVE int ugu_selection_outline_size(
        const BridgeDocument *handle)
    {
        return static_cast<int>(handle->outlinePoints.size());
    }
}

// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "brush/BrushPreset.hpp"
#include "brush/EraserPreset.hpp"
#include "document/DocumentLimits.hpp"
#include "document/DocumentOperations.hpp"
#include "wasm/BridgeDocument.hpp"

#include <QColor>
#include <QRandomGenerator>
#include <QRectF>

#include <algorithm>
#include <utility>

using namespace ugurugu::wasm;

extern "C"
{
    EMSCRIPTEN_KEEPALIVE int ugu_brush_preset_count()
    {
        return static_cast<int>(ugurugu::BrushPresetCatalog::builtIns().size());
    }

    EMSCRIPTEN_KEEPALIVE const char *ugu_brush_preset_name(
        BridgeDocument *handle, int index)
    {
        const auto &presets = ugurugu::BrushPresetCatalog::builtIns();
        if (index < 0 || index >= presets.size())
        {
            return "";
        }
        handle->scratchText =
            ugurugu::BrushPresetCatalog::displayName(presets[index]).toUtf8();
        return handle->scratchText.constData();
    }

    EMSCRIPTEN_KEEPALIVE double ugu_brush_preset_default_size(int index)
    {
        const auto &presets = ugurugu::BrushPresetCatalog::builtIns();
        if (index < 0 || index >= presets.size())
        {
            return 6.0;
        }
        return presets[index].defaultSize;
    }

    EMSCRIPTEN_KEEPALIVE void ugu_set_brush_preset(
        BridgeDocument *handle, int index)
    {
        const auto &presets = ugurugu::BrushPresetCatalog::builtIns();
        if (index < 0 || index >= presets.size())
        {
            return;
        }
        handle->brushTemplate.brush = presets[index].settings;
    }

    EMSCRIPTEN_KEEPALIVE int ugu_eraser_preset_count()
    {
        return static_cast<int>(
            ugurugu::EraserPresetCatalog::builtIns().size());
    }

    EMSCRIPTEN_KEEPALIVE const char *ugu_eraser_preset_name(
        BridgeDocument *handle, int index)
    {
        const auto &presets = ugurugu::EraserPresetCatalog::builtIns();
        if (index < 0 || index >= presets.size())
        {
            return "";
        }
        handle->scratchText =
            ugurugu::EraserPresetCatalog::displayName(presets[index]).toUtf8();
        return handle->scratchText.constData();
    }

    EMSCRIPTEN_KEEPALIVE double ugu_eraser_preset_default_size(int index)
    {
        const auto &presets = ugurugu::EraserPresetCatalog::builtIns();
        if (index < 0 || index >= presets.size())
        {
            return 6.0;
        }
        return presets[index].defaultSize;
    }

    EMSCRIPTEN_KEEPALIVE void ugu_set_eraser_preset(
        BridgeDocument *handle, int index)
    {
        const auto &presets = ugurugu::EraserPresetCatalog::builtIns();
        if (index < 0 || index >= presets.size())
        {
            return;
        }
        handle->brushTemplate.brush = presets[index].settings;
    }

    EMSCRIPTEN_KEEPALIVE void ugu_set_brush(BridgeDocument *handle,
        int red,
        int green,
        int blue,
        int alpha,
        double width,
        int erase)
    {
        handle->brushTemplate.color = QColor(red, green, blue, alpha);
        handle->brushTemplate.width = width;
        handle->brushTemplate.mode = erase != 0 ? ugurugu::StrokeMode::Erase
                                                : ugurugu::StrokeMode::Paint;
    }

    // The desktop carries this per stroke from a brush-panel toggle
    // (CanvasWidgetTools.cpp:83); BrushSettings defaults it off and no preset
    // sets it, so without this the web could only ever draw aliased strokes.
    // Stored beside the template rather than on it because preset selection
    // replaces the template's brush wholesale, which used to silently drop
    // the toggle after switching tools.
    EMSCRIPTEN_KEEPALIVE void ugu_set_brush_antialiasing(
        BridgeDocument *handle, int antialiasing)
    {
        handle->brushAntialiasing = antialiasing != 0;
    }

    EMSCRIPTEN_KEEPALIVE void ugu_set_stabilization(
        BridgeDocument *handle, double strength)
    {
        handle->stabilizer.setStrength(strength);
    }

    EMSCRIPTEN_KEEPALIVE int ugu_stroke_begin(BridgeDocument *handle,
        int frame,
        double x,
        double y,
        double pressure,
        double timestamp)
    {
        const ugurugu::Document &document = handle->controller->document();
        const QUuid layerId = paintTargetLayer(document);
        const ugurugu::Layer *target = document.layer(layerId);
        if (layerId.isNull() || target == nullptr)
        {
            setError(StatusNoPaintLayer,
                QByteArrayLiteral("document has no paint layer"));
            return 0;
        }
        // CanvasWidget::beginStroke refuses a stroke the artist could not see.
        // Without the same guard the web committed invisible strokes to a
        // hidden layer and said nothing, so the work only reappeared when the
        // layer was switched back on.
        if (!target->visible)
        {
            setError(StatusLayerNotDrawable,
                QByteArrayLiteral("the active layer is hidden; make it "
                                  "visible to draw"));
            return 0;
        }
        if (target->opacity <= 0.0)
        {
            setError(StatusLayerNotDrawable,
                QByteArrayLiteral("the active layer opacity is 0%; raise it "
                                  "to draw"));
            return 0;
        }
        // A layer inside a hidden group is just as invisible, and its own flags
        // do not say so.
        if (!ugurugu::DocumentOperations::isLayerRenderable(document, *target))
        {
            setError(StatusLayerNotDrawable,
                QByteArrayLiteral("the group holding the active layer is "
                                  "hidden; make it visible to draw"));
            return 0;
        }
        handle->strokeFrame = frame;
        handle->strokeLayerId = layerId;
        handle->activeStroke = handle->brushTemplate;
        handle->activeStroke.id = QUuid::createUuid();
        handle->activeStroke.seed = QRandomGenerator::global()->generate64();
        // Erasers keep their preset's antialiasing, matching the desktop's
        // beginStroke which applies the toggle to paint strokes only.
        if (handle->activeStroke.mode == ugurugu::StrokeMode::Paint)
        {
            handle->activeStroke.brush.antialiasing = handle->brushAntialiasing;
        }
        // A selection made on this layer confines the stroke to it, exactly as
        // CanvasWidget::beginStroke does. Both the incremental preview and the
        // committed render honour clipMask, so what is drawn is what lands.
        if (selectionAppliesTo(handle, layerId))
        {
            handle->activeStroke.clipMask = handle->selectionMask;
        }
        const QPointF raw(x, y);
        const auto time = static_cast<quint64>(timestamp);
        handle->activeStroke.points = {
            canvasPoint(handle, handle->stabilizer.begin(raw, time), pressure)};
        handle->lastRawPosition = raw;
        handle->lastTimestamp = time;
        handle->strokeInProgress = true;

        // A split promoted by the previous ugu_stroke_end already holds this
        // layer's committed pixels, so the common case of drawing stroke after
        // stroke never pays for renderLayerSplit again.
        handle->incrementalActive = ensureLayerSplit(handle, layerId, frame);
        if (!handle->incrementalActive)
        {
            renderCommittedFrame(handle, frame);
            clearError();
            return 1;
        }

        handle->incremental.clear();
        if (!handle->renderedFrameCommitted
            || handle->renderedFrameIndex != frame
            || handle->renderedFrame.size() != document.size)
        {
            // Composing the split with its own base layer reproduces the
            // committed frame exactly — LayerSplitPreviewTests pins that — and
            // costs three image composites instead of a second full render.
            QImage composed = ugurugu::RenderEngine::composeLayerSplit(
                handle->split, handle->split.layerBase);
            if (composed.isNull())
            {
                renderCommittedFrame(handle, frame);
            }
            else
            {
                handle->renderedFrame = std::move(composed);
                handle->renderedFrameIndex = frame;
                handle->renderedFrameCommitted = true;
                handle->dirty = handle->renderedFrame.rect();
            }
        }
        else
        {
            handle->dirty = QRect();
        }
        clearError();
        return 1;
    }

    EMSCRIPTEN_KEEPALIVE void ugu_stroke_append(BridgeDocument *handle,
        double x,
        double y,
        double pressure,
        double timestamp)
    {
        if (!handle->strokeInProgress)
        {
            return;
        }
        const QPointF raw(x, y);
        const auto time = static_cast<quint64>(timestamp);
        handle->activeStroke.points.append(canvasPoint(
            handle, handle->stabilizer.update(raw, time), pressure));
        handle->lastRawPosition = raw;
        handle->lastTimestamp = time;
    }

    // Renders the active stroke's pending points into the frame buffer and
    // narrows the dirty rectangle to the touched tiles when the incremental
    // path holds; otherwise the whole frame is re-rendered and dirty.
    EMSCRIPTEN_KEEPALIVE int ugu_stroke_render(BridgeDocument *handle)
    {
        if (!handle->strokeInProgress)
        {
            return 0;
        }
        if (!handle->incrementalActive)
        {
            renderFullStrokePreview(handle);
            return 1;
        }
        QRect dirty;
        if (!updateIncrementalStroke(handle, &dirty))
        {
            handle->incrementalActive = false;
            invalidateSplit(handle);
            renderFullStrokePreview(handle);
            return 1;
        }
        handle->dirty = dirty.intersected(handle->renderedFrame.rect());
        return 1;
    }

    EMSCRIPTEN_KEEPALIVE int ugu_stroke_end(BridgeDocument *handle)
    {
        if (!handle->strokeInProgress)
        {
            return -1;
        }
        const QPointF finished = canvasPoint(handle,
            handle->stabilizer.finish(
                handle->lastRawPosition, handle->lastTimestamp),
            1.0)
                                     .position;
        if (auto &points = handle->activeStroke.points; !points.isEmpty())
        {
            const QPointF delta = finished - points.last().position;
            // Matches CanvasWidget::endStroke: an endpoint the resampler would
            // merge away is moved into the last point instead of appended, so
            // the committed geometry matches what the preview drew.
            if (points.size() >= ugurugu::DocumentLimits::maximumPointsPerStroke
                || QPointF::dotProduct(delta, delta) < 0.75 * 0.75)
            {
                points.last().position = finished;
            }
            else
            {
                points.append(
                    ugurugu::StrokePoint{finished, points.last().pressure});
            }
        }
        handle->strokeInProgress = false;

        // Bring the preview up to the final point and bake it into the split's
        // base layer. If the commit then lands unchanged, that promoted split
        // is the committed document's split and renderedFrame already shows
        // it, so no full render is needed here or at the next stroke.
        QRect tailDirty;
        ugurugu::RenderEngine::LayerSplitFrame promoted;
        bool promotable = false;
        if (handle->incrementalActive
            && updateIncrementalStroke(handle, &tailDirty))
        {
            promoted = handle->split;
            promotable = handle->incremental.applyTo(promoted.layerBase);
        }
        handle->incrementalActive = false;

        const QUuid layerId = handle->strokeLayerId;
        const auto result = handle->controller->addStroke(
            layerId, std::move(handle->activeStroke));
        handle->activeStroke = {};
        using AddStrokeResult = ugurugu::DocumentController::AddStrokeResult;
        if (promotable && result == AddStrokeResult::Added)
        {
            handle->split = std::move(promoted);
            handle->splitLayerId = layerId;
            handle->splitFrame = handle->strokeFrame;
            handle->splitUsable = true;
            handle->renderedFrameIndex = handle->strokeFrame;
            handle->renderedFrameCommitted = true;
            handle->dirty = tailDirty.intersected(handle->renderedFrame.rect());
        }
        else
        {
            invalidateSplit(handle);
            renderCommittedFrame(handle, handle->strokeFrame);
        }
        if (result != AddStrokeResult::Added
            && result != AddStrokeResult::AddedWithResampledPoints)
        {
            setError(
                StatusStrokeRejected, QByteArrayLiteral("stroke rejected"));
        }
        return static_cast<int>(result);
    }

    EMSCRIPTEN_KEEPALIVE void ugu_stroke_cancel(BridgeDocument *handle)
    {
        handle->strokeInProgress = false;
        handle->incrementalActive = false;
        invalidateSplit(handle);
        handle->activeStroke = {};
        renderCommittedFrame(handle, handle->strokeFrame);
    }

    // reference: 0 active layer, 1 layers marked as references, 2 all visible
    // layers. comparison: 0 alpha boundary, 1 color tolerance. tolerance is
    // 0-255 and only applies to the color comparison.
    EMSCRIPTEN_KEEPALIVE void ugu_set_fill_options(BridgeDocument *handle,
        int reference,
        int comparison,
        int tolerance,
        int antialiasing)
    {
        handle->fillReference = std::clamp(
            reference, 0, static_cast<int>(ReferenceAllVisibleLayers));
        handle->fillComparison =
            comparison == 1 ? ugurugu::FloodFillMask::Comparison::Color
                            : ugurugu::FloodFillMask::Comparison::AlphaBoundary;
        handle->fillTolerance = std::clamp(tolerance, 0, 255);
        handle->bucketAntialiasing = antialiasing != 0;
    }

    // Floods from the seed on the configured reference image and commits the
    // result as one Fill stroke. Returns 1 on success; on 0 the status code
    // says whether the point was outside the selection, the area was not
    // fillable, or the layer refused the stroke.
    EMSCRIPTEN_KEEPALIVE int ugu_bucket_fill(
        BridgeDocument *handle, int frame, double x, double y)
    {
        const ugurugu::Document &document = handle->controller->document();
        const QSize size = document.size;
        if (!QRectF(QPointF(0.0, 0.0), QSizeF(size)).contains(QPointF(x, y)))
        {
            setError(StatusInvalidArgument,
                QByteArrayLiteral("the fill point is outside the canvas"));
            return 0;
        }
        const QUuid layerId = paintTargetLayer(document);
        if (layerId.isNull())
        {
            setError(StatusNoPaintLayer,
                QByteArrayLiteral("document has no paint layer"));
            return 0;
        }
        const QPoint seed(std::clamp(static_cast<int>(x), 0, size.width() - 1),
            std::clamp(static_cast<int>(y), 0, size.height() - 1));
        // A selection is a wall: the desktop refuses a fill seeded outside it
        // rather than quietly filling the whole layer and clipping it away.
        if (selectionAppliesTo(handle, layerId)
            && handle->selectionMask.constScanLine(seed.y())[seed.x()] < 128)
        {
            setError(StatusNoSelection,
                QByteArrayLiteral("click inside the selected area to fill it"));
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
        const QImage coverage = ugurugu::FloodFillMask::fromImage(
            reference, seed, handle->fillComparison, handle->fillTolerance);
        if (coverage.isNull())
        {
            setError(StatusEmptyRegion,
                QByteArrayLiteral("no fillable area was found"));
            return 0;
        }
        return commitFrozenFill(handle, coverage, frame);
    }
}

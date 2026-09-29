// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "brush/BrushPreset.hpp"
#include "brush/EraserPreset.hpp"
#include "document/DocumentLimits.hpp"
#include "document/SelectionOperation.hpp"
#include "render/PreviewRenderPolicy.hpp"
#include "render/RenderEngine.hpp"
#include "ui/CanvasViewport.hpp"
#include "ui/CanvasWidget.hpp"
#include "ui/Theme.hpp"

#include <QKeyEvent>
#include <QPainter>
#include <QRandomGenerator>

#include <algorithm>
#include <cmath>
#include <numbers>

namespace ugurugu
{

using namespace canvas_detail;

namespace
{

qreal inputPressure(qreal pressure, bool enabled)
{
    return enabled ? std::clamp(pressure, 0.05, 1.0) : 1.0;
}

}

void CanvasWidget::beginStroke(const QPointF &widgetPosition,
    qreal pressure,
    bool tabletEraser,
    quint64 timestamp)
{
    bool inside = false;
    const QPointF documentPosition = mapToDocument(widgetPosition, &inside);
    const Document &document = m_controller->document();
    const Layer *layer = document.layer(document.activeLayerId);
    if (!inside)
    {
        return;
    }
    if (!layer || layer->kind != LayerKind::Paint)
    {
        emit interactionMessage(tr("Add a layer before using this tool."));
        return;
    }
    if (m_groupSelectionActive)
    {
        emit interactionMessage(
            tr("Groups can't be painted on. Select a paint layer to draw."));
        return;
    }
    cancelSelectionTransformForBoundary(
        tr("The pending selection transform was canceled before drawing."));
    if (!reportLayerAcceptsPaint(*layer))
    {
        return;
    }

    // Input latency takes priority over speculative animation frames. A live
    // wobbling stroke uses its single-worker interaction-frame lookahead
    // instead of competing with the multi-worker full-frame warmup.
    cancelFrameCacheWarmup();
    m_activeStroke = Stroke();
    m_activeStroke.seed = QRandomGenerator::global()->generate64();
    m_activeStrokeUsesTabletPressure = m_tabletPressureEnabled;
    const bool erasing = tabletEraser || m_tool == Tool::Eraser;
    m_activeStroke.mode = erasing ? StrokeMode::Erase : StrokeMode::Paint;
    m_activeStroke.color = m_brushColor;
    m_activeStroke.width = erasing ? m_eraserWidth : m_brushWidth;
    m_activeStroke.brush = erasing ? m_eraserSettings : m_brushSettings;
    if (!erasing)
    {
        m_activeStroke.brush.antialiasing = m_brushAntialiasing;
    }
    if (!m_selectionMask.isNull() && m_selectionLayer == document.activeLayerId)
    {
        m_activeStroke.clipMask = m_selectionMask;
    }
    m_strokeStabilizer.setStrength(
        erasing ? eraserStabilization() : brushStabilization());
    const QPointF position = m_strokeStabilizer.begin(
        clampedDocumentPosition(documentPosition), timestamp);
    m_activeStroke.points.append(
        {position, inputPressure(pressure, m_activeStrokeUsesTabletPressure)});
    m_activeStrokeLayer = document.activeLayerId;
    m_drawing = true;
    invalidateActiveStrokePreview();
    m_incrementalStrokeRenderer.clear();
    m_composedPreviewFrame = {};
    m_composedSelectionPreviewRegion = {};
    m_composedPreviewBaseKey = 0;
    if (usesPreparedInteractionFrames())
    {
        const QSize renderSize = previewRenderSize();
        if (hasInteractionFrame(
                m_currentFrame, renderSize, m_activeStrokeLayer))
        {
            const int frameCount = std::max(1, document.animationFrames);
            requestInteractionFrameWarmup((m_currentFrame + 1) % frameCount);
        }
        else
        {
            requestInteractionFrameWarmup(m_currentFrame);
        }
    }
    requestDisplayUpdate();
}

void CanvasWidget::continueStroke(
    const QPointF &widgetPosition, qreal pressure, quint64 timestamp)
{
    if (!m_drawing)
    {
        return;
    }
    if (m_activeStroke.points.size() >= DocumentLimits::maximumPointsPerStroke)
    {
        return;
    }
    const QPointF position = m_strokeStabilizer.update(
        clampedDocumentPosition(mapToDocument(widgetPosition)), timestamp);
    if (pointDistance(position, m_activeStroke.points.constLast().position)
        < 0.75)
    {
        return;
    }
    m_activeStroke.points.append(
        {position, inputPressure(pressure, m_activeStrokeUsesTabletPressure)});
    invalidateActiveStrokePreview();
    // A pointer reports several times per display refresh, and a resolve
    // re-renders every tile the stroke tail covers — a cost that grows with
    // the stroke, so the reports piled up faster than they could be served
    // and drawing fell behind the hand. Only the first report after a paint
    // resolves; the rest just widen the pending repaint and let that paint
    // resolve once for all of them. The repaint is the whole viewport rather
    // than the tail, because the exact changed region is a product of the
    // resolve we are skipping; the GPU display redraws the frame view whole
    // either way, and the software path trades one full blit per frame for
    // the resolves it no longer runs per report.
    if (m_strokePreviewResolvedSincePaint)
    {
        requestDisplayUpdate();
        return;
    }
    m_strokePreviewResolvedSincePaint = true;
    const QSize renderSize = previewRenderSize();
    bool previewResolved = false;
    if (!renderSize.isEmpty())
    {
        activeStrokePreview(displayDocument(), renderSize, previewResolved);
    }
    if (previewResolved && m_activeStrokePreviewPatchBoundsValid)
    {
        if (m_activeStrokePreviewPatchBounds.isEmpty())
        {
            return;
        }
        const Document &document = m_controller->document();
        const QRectF documentRect(m_activeStrokePreviewPatchBounds.x()
                                      * document.size.width()
                                      / static_cast<qreal>(renderSize.width()),
            m_activeStrokePreviewPatchBounds.y() * document.size.height()
                / static_cast<qreal>(renderSize.height()),
            m_activeStrokePreviewPatchBounds.width() * document.size.width()
                / static_cast<qreal>(renderSize.width()),
            m_activeStrokePreviewPatchBounds.height() * document.size.height()
                / static_cast<qreal>(renderSize.height()));
        requestDisplayUpdate(documentTransform()
                .mapRect(documentRect)
                .toAlignedRect()
                .adjusted(-2, -2, 2, 2));
        return;
    }
    requestDisplayUpdate();
}

void CanvasWidget::endStroke(const QPointF &widgetPosition, quint64 timestamp)
{
    if (!m_drawing)
    {
        return;
    }
    // A TabletRelease reports pressure 0 because the pen has already left the
    // surface, and the clamp in continueStroke would turn that into the
    // minimum width. Carrying the last sampled pressure forward keeps the
    // taper that the preceding TabletMove events already produced.
    const qreal endpointPressure = m_activeStroke.points.constLast().pressure;
    continueStroke(widgetPosition, endpointPressure, timestamp);
    const QPointF finalPosition = m_strokeStabilizer.finish(
        clampedDocumentPosition(mapToDocument(widgetPosition)), timestamp);
    const StrokePoint finalPoint{finalPosition, endpointPressure};
    if (m_activeStroke.points.size() >= DocumentLimits::maximumPointsPerStroke
        || pointDistance(
               finalPosition, m_activeStroke.points.constLast().position)
               < 0.75)
    {
        m_activeStroke.points.last() = finalPoint;
    }
    else
    {
        m_activeStroke.points.append(finalPoint);
    }
    invalidateActiveStrokePreview();
    const QSize promotedRenderSize = previewRenderSize();
    bool promotedPreviewResolved = false;
    const QImage promotedFrame = activeStrokePreview(
        m_controller->document(), promotedRenderSize, promotedPreviewResolved);
    RenderEngine::LayerSplitFrame promotedSplit = m_previewSplit;
    if (!promotedSplit.valid || m_previewSplitLayer != m_activeStrokeLayer
        || m_previewSplitFrame != m_currentFrame
        || promotedSplit.below.size() != promotedRenderSize
        || !m_incrementalStrokeRenderer.applyTo(promotedSplit.layerBase))
    {
        promotedSplit = {};
    }
    RenderEngine::LayerRasterFrame promotedRasters = m_previewLayerRasters;
    auto promotedLayer = promotedRasters.paintLayers.find(m_activeStrokeLayer);
    if (!promotedRasters.valid || m_previewLayerRasterFrame != m_currentFrame
        || promotedRasters.outputSize != promotedRenderSize
        || promotedLayer == promotedRasters.paintLayers.end()
        || !m_incrementalStrokeRenderer.applyTo(promotedLayer.value()))
    {
        promotedRasters = {};
    }
    Stroke completed = m_activeStroke;
    const QUuid layerId = m_activeStrokeLayer;
    const int promotedFrameIndex = m_currentFrame;
    m_drawing = false;
    cancelInteractionFrameWarmup();
    m_activeStroke = Stroke();
    m_activeStrokeLayer = QUuid();
    invalidateActiveStrokePreview();
    // Only written to while a stroke is live, and its pixels survive as the
    // promoted frame below. Releasing it here is what frees the budget the
    // promotion needs.
    m_composedPreviewFrame = {};
    m_composedSelectionPreviewRegion = {};
    m_composedPreviewBaseKey = 0;
    m_strokeStabilizer.reset();
    m_strokeCommitDefersInteractionWarmup = true;
    const DocumentController::AddStrokeResult result =
        commitStroke(layerId, std::move(completed));
    m_strokeCommitDefersInteractionWarmup = false;
    bool promotedReusableBase = false;
    if (result == DocumentController::AddStrokeResult::Added
        && promotedPreviewResolved && !promotedFrame.isNull())
    {
        promotedReusableBase = promotedSplit.valid || promotedRasters.valid;
        m_cachedRenderSize = promotedRenderSize;
        const int cost =
            PreviewRenderPolicy::cacheCostKiB(promotedFrame.sizeInBytes());
        m_frameCache.insert(
            promotedFrameIndex, new QImage(promotedFrame), cost);
        // The promotion is a fresh composite of the committed document, so a
        // regional invalidation that just marked the frame stale is satisfied.
        m_frameCacheStaleFrames.remove(promotedFrameIndex);
        clearCompletedFrameCacheRefresh();
        if (promotedSplit.valid)
        {
            m_previewSplit = std::move(promotedSplit);
            m_previewSplitLayer = layerId;
            m_previewSplitFrame = promotedFrameIndex;
        }
        if (promotedRasters.valid)
        {
            m_previewLayerRasters = std::move(promotedRasters);
            m_previewLayerRasterFrame = promotedFrameIndex;
        }
        updateFrameCacheBudget();
    }
    if (result != DocumentController::AddStrokeResult::Added
        && result
               != DocumentController::AddStrokeResult::AddedWithResampledPoints)
    {
        scheduleFrameCacheWarmup();
    }
    // A pen-up that promoted both the exact current frame and a reusable
    // split or raster base has nothing left for the interaction worker to
    // prepare. Every other outcome — resampled points, a rejected stroke, a
    // promotion without a reusable base — still warms up.
    const bool fullyPromoted =
        result == DocumentController::AddStrokeResult::Added
        && promotedPreviewResolved && !promotedFrame.isNull()
        && promotedReusableBase;
    if (!m_animating && !fullyPromoted)
    {
        requestInteractionFrameWarmup(m_currentFrame);
    }
    requestDisplayUpdate();
}

DocumentController::AddStrokeResult CanvasWidget::commitStroke(
    const QUuid &layerId, Stroke stroke)
{
    const bool recordsColor =
        stroke.mode == StrokeMode::Paint || stroke.mode == StrokeMode::Fill;
    const QColor usedColor = stroke.color;
    m_pendingStrokeRefreshHint = {true, layerId, stroke.id};
    const DocumentController::AddStrokeResult result =
        m_controller->addStroke(layerId, std::move(stroke));
    m_pendingStrokeRefreshHint = {};
    switch (result)
    {
    case DocumentController::AddStrokeResult::Added:
        if (recordsColor)
        {
            emit colorUsed(usedColor);
        }
        return result;
    case DocumentController::AddStrokeResult::AddedWithResampledPoints:
        if (recordsColor)
        {
            emit colorUsed(usedColor);
        }
        emit interactionMessage(tr("The stroke was simplified because the "
                                   "project point limit was reached."));
        return result;
    case DocumentController::AddStrokeResult::RejectedInvalidLayer:
        emit interactionMessage(tr("The stroke could not be added because its "
                                   "layer is no longer available."));
        return result;
    case DocumentController::AddStrokeResult::RejectedStrokeLimit:
        emit interactionMessage(tr("The stroke could not be added because the "
                                   "project stroke limit was reached."));
        return result;
    case DocumentController::AddStrokeResult::RejectedPointLimit:
        emit interactionMessage(tr("The stroke could not be added because the "
                                   "project point limit was reached."));
        return result;
    case DocumentController::AddStrokeResult::RejectedInvalidStroke:
    case DocumentController::AddStrokeResult::RejectedMaskLimit:
    case DocumentController::AddStrokeResult::RejectedCommit:
        emit interactionMessage(tr("The stroke could not be added."));
        return result;
    }
    return result;
}

void CanvasWidget::cancelStroke()
{
    if (!m_drawing)
    {
        return;
    }
    m_drawing = false;
    cancelInteractionFrameWarmup();
    m_activeStroke = Stroke();
    m_activeStrokeLayer = QUuid();
    invalidateActiveStrokePreview();
    m_incrementalStrokeRenderer.clear();
    m_composedPreviewFrame = {};
    m_composedSelectionPreviewRegion = {};
    m_composedPreviewBaseKey = 0;
    m_strokeStabilizer.reset();
    scheduleFrameCacheWarmup();
    if (!m_animating)
    {
        requestInteractionFrameWarmup(m_currentFrame);
    }
    requestDisplayUpdate();
}

void CanvasWidget::beginPan(const QPointF &widgetPosition)
{
    cancelStroke();
    m_panning = true;
    m_lastPanPosition = widgetPosition;
    updateCursor();
}

void CanvasWidget::continuePan(const QPointF &widgetPosition)
{
    const QPointF delta = widgetPosition - m_lastPanPosition;
    m_pan += delta;
    m_lastPanPosition = widgetPosition;
    const QPoint pixelDelta(qRound(delta.x()), qRound(delta.y()));
    const bool integralDelta = qFuzzyIsNull(delta.x() - pixelDelta.x())
                               && qFuzzyIsNull(delta.y() - pixelDelta.y());
    // scroll() shifts the software backing store, which the GPU display does
    // not paint into; there the pan is a transform change on the next draw.
    if (!usingGpuDisplay() && integralDelta && !pixelDelta.isNull()
        && std::abs(pixelDelta.x()) < width()
        && std::abs(pixelDelta.y()) < height())
    {
        scroll(pixelDelta.x(), pixelDelta.y(), rect());
    }
    else
    {
        requestDisplayUpdate();
    }
}

void CanvasWidget::endPan()
{
    if (!m_panning)
    {
        return;
    }
    m_panning = false;
    updateCursor();
    const QRect pointerRect = pointerUpdateRect();
    if (!pointerRect.isEmpty())
    {
        requestDisplayUpdate(pointerRect);
    }
}

QPointF CanvasWidget::zoomAnchorPosition() const
{
    return m_pointerOverWidget ? m_pointerWidgetPosition
                               : QPointF(width() * 0.5, height() * 0.5);
}

void CanvasWidget::zoomToward(qreal targetZoom, const QPointF &widgetPosition)
{
    const qreal nextZoom = std::clamp(targetZoom, minimumZoom, maximumZoom);
    if (qFuzzyCompare(m_zoom, nextZoom))
    {
        return;
    }
    cancelTouchSequence();
    bool inside = false;
    const QPointF anchor = mapToDocument(widgetPosition, &inside);
    m_zoom = nextZoom;
    if (inside)
    {
        m_pan += widgetPosition - documentTransform().map(anchor);
    }
    m_zoomRenderTimer.start();
    notifyZoomChanged();
    requestDisplayUpdate();
}

void CanvasWidget::beginZoomDrag(const QPointF &widgetPosition)
{
    cancelStroke();
    endPan();
    m_zoomDragging = true;
    m_zoomDragStart = widgetPosition;
    m_zoomDragStartZoom = m_zoom;
    m_zoomDragAnchor = mapToDocument(widgetPosition, &m_zoomDragAnchorInside);
    updateCursor();
}

void CanvasWidget::continueZoomDrag(const QPointF &widgetPosition)
{
    if (!m_zoomDragging)
    {
        return;
    }
    const qreal distance = widgetPosition.x() - m_zoomDragStart.x();
    const qreal nextZoom =
        std::clamp(m_zoomDragStartZoom
                       * std::pow(2.0, distance / dragZoomDoublingDistance),
            minimumZoom,
            maximumZoom);
    if (qFuzzyCompare(m_zoom, nextZoom))
    {
        return;
    }
    m_zoom = nextZoom;
    if (m_zoomDragAnchorInside)
    {
        m_pan += m_zoomDragStart - documentTransform().map(m_zoomDragAnchor);
    }
    m_zoomRenderTimer.start();
    notifyZoomChanged();
    requestDisplayUpdate();
}

void CanvasWidget::endZoomDrag()
{
    if (!m_zoomDragging)
    {
        return;
    }
    m_zoomDragging = false;
    m_zoomRenderTimer.stop();
    updateCursor();
    const QRect pointerRect = pointerUpdateRect();
    if (!pointerRect.isEmpty())
    {
        requestDisplayUpdate(pointerRect);
    }
    requestDisplayUpdate();
}

void CanvasWidget::applyCanvasRotation(qreal degrees)
{
    if (!std::isfinite(degrees))
    {
        return;
    }
    const qreal normalized = normalizedRotation(degrees);
    if (qFuzzyIsNull(m_canvasRotation - normalized))
    {
        return;
    }
    m_canvasRotation = normalized;
    emit canvasRotationChanged(normalized);
    if (m_pointerOverWidget)
    {
        updatePointerPosition(m_pointerWidgetPosition);
    }
    else
    {
        updateSelectionActionBar();
    }
    requestDisplayUpdate();
}

void CanvasWidget::rotateCanvasAround(
    qreal degrees, const QPointF &widgetPosition)
{
    if (!std::isfinite(degrees))
    {
        return;
    }
    const qreal normalized = normalizedRotation(degrees);
    if (qFuzzyIsNull(m_canvasRotation - normalized))
    {
        return;
    }
    bool inside = false;
    const QPointF anchor = mapToDocument(widgetPosition, &inside);
    m_canvasRotation = normalized;
    if (inside)
    {
        m_pan += widgetPosition - documentTransform().map(anchor);
    }
    emit canvasRotationChanged(normalized);
    if (m_pointerOverWidget)
    {
        updatePointerPosition(m_pointerWidgetPosition);
    }
    else
    {
        updateSelectionActionBar();
    }
    requestDisplayUpdate();
}

void CanvasWidget::beginCanvasRotation(const QPointF &widgetPosition)
{
    cancelStroke();
    endPan();
    endZoomDrag();
    endColorPick();
    m_rotatingCanvas = true;
    m_rotationDragStart = widgetPosition;
    m_rotationDragStartAngle = m_canvasRotation;
    updateCursor();
}

void CanvasWidget::continueCanvasRotation(const QPointF &widgetPosition)
{
    if (!m_rotatingCanvas)
    {
        return;
    }
    const qreal delta = (widgetPosition.x() - m_rotationDragStart.x())
                        * dragRotationDegreesPerPixel;
    applyCanvasRotation(m_rotationDragStartAngle + delta);
}

void CanvasWidget::endCanvasRotation()
{
    if (!m_rotatingCanvas)
    {
        return;
    }
    m_rotatingCanvas = false;
    updateCursor();
    const QRect pointerRect = pointerUpdateRect();
    if (!pointerRect.isEmpty())
    {
        requestDisplayUpdate(pointerRect);
    }
    requestDisplayUpdate();
}

bool CanvasWidget::touchGestureConflictsWithActiveInteraction() const
{
    return m_drawing || m_tabletSequence || m_panning || m_zoomDragging
           || m_rotatingCanvas || m_pickingColor || m_movingSelection
           || m_areaSelectionActive || m_textDragging;
}

void CanvasWidget::beginTouchGesture(int firstPointId,
    const QPointF &firstPosition,
    int secondPointId,
    const QPointF &secondPosition)
{
    endTouchGesture();
    const QPointF vector = secondPosition - firstPosition;
    const qreal distance = std::hypot(vector.x(), vector.y());
    if (firstPointId == secondPointId || !std::isfinite(firstPosition.x())
        || !std::isfinite(firstPosition.y())
        || !std::isfinite(secondPosition.x())
        || !std::isfinite(secondPosition.y()) || distance <= 0.01)
    {
        return;
    }

    m_touchFirstPointId = firstPointId;
    m_touchSecondPointId = secondPointId;
    m_touchGestureLastCenter = (firstPosition + secondPosition) * 0.5;
    m_touchGestureLastDistance = distance;
    m_touchGestureLastAngle =
        std::atan2(vector.y(), vector.x()) * 180.0 / std::numbers::pi_v<qreal>;
    m_touchGestureActive = true;
    updateCursor();
    requestDisplayUpdate();
}

void CanvasWidget::continueTouchGesture(
    const QPointF &firstPosition, const QPointF &secondPosition)
{
    if (!m_touchGestureActive || !std::isfinite(firstPosition.x())
        || !std::isfinite(firstPosition.y())
        || !std::isfinite(secondPosition.x())
        || !std::isfinite(secondPosition.y()))
    {
        return;
    }

    const QPointF vector = secondPosition - firstPosition;
    const qreal distance = std::hypot(vector.x(), vector.y());
    if (distance <= 0.01)
    {
        endTouchGesture();
        return;
    }
    const QPointF center = (firstPosition + secondPosition) * 0.5;
    const qreal angle =
        std::atan2(vector.y(), vector.x()) * 180.0 / std::numbers::pi_v<qreal>;
    const qreal angleDelta =
        std::remainder(angle - m_touchGestureLastAngle, 360.0);
    const qreal nextZoom =
        std::clamp(m_zoom * distance / m_touchGestureLastDistance,
            minimumZoom,
            maximumZoom);
    const qreal nextRotation =
        normalizedRotation(m_canvasRotation + angleDelta);
    const bool zoomChanged = !qFuzzyCompare(m_zoom, nextZoom);
    const bool rotationChanged = !qFuzzyIsNull(m_canvasRotation - nextRotation);
    const QPointF anchor = mapToDocument(m_touchGestureLastCenter);

    m_zoom = nextZoom;
    m_canvasRotation = nextRotation;
    const QPointF panDelta = center - documentTransform().map(anchor);
    m_pan += panDelta;
    m_touchGestureLastCenter = center;
    m_touchGestureLastDistance = distance;
    m_touchGestureLastAngle = angle;

    if (zoomChanged)
    {
        m_zoomRenderTimer.start();
        notifyZoomChanged();
    }
    if (rotationChanged)
    {
        emit canvasRotationChanged(nextRotation);
    }
    if (!zoomChanged && !rotationChanged && panDelta.isNull())
    {
        return;
    }
    if (m_pointerOverWidget)
    {
        updatePointerPosition(m_pointerWidgetPosition);
    }
    updateSelectionActionBar();
    requestDisplayUpdate();
}

void CanvasWidget::endTouchGesture()
{
    if (!m_touchGestureActive)
    {
        m_touchFirstPointId = -1;
        m_touchSecondPointId = -1;
        return;
    }
    m_touchGestureActive = false;
    m_touchFirstPointId = -1;
    m_touchSecondPointId = -1;
    m_zoomRenderTimer.stop();
    updateCursor();
    requestDisplayUpdate();
}

void CanvasWidget::suppressTouchSequence()
{
    if (!m_touchSequence)
    {
        return;
    }
    endTouchGesture();
    m_touchGestureSuppressed = true;
    updateCursor();
    requestDisplayUpdate();
}

void CanvasWidget::cancelTouchSequence()
{
    const bool hadSequence = m_touchSequence;
    endTouchGesture();
    m_touchSequence = false;
    m_touchGestureSuppressed = false;
    m_touchDevice = nullptr;
    if (hadSequence)
    {
        updateCursor();
        requestDisplayUpdate();
    }
}

bool CanvasWidget::isColorPickableTool() const
{
    return m_tool == Tool::Brush || m_tool == Tool::Eraser
           || m_tool == Tool::Bucket || m_tool == Tool::Eyedropper;
}

void CanvasWidget::beginColorPick(const QPointF &widgetPosition)
{
    cancelStroke();
    m_pickingColor = true;
    updateCursor();
    pickColorAt(widgetPosition);
    requestDisplayUpdate();
}

void CanvasWidget::endColorPick()
{
    if (!m_pickingColor)
    {
        return;
    }
    m_pickingColor = false;
    m_colorPickFrame = {};
    m_colorPickFrameIndex = -1;
    updateFrameCacheBudget();
    updateCursor();
    requestDisplayUpdate();
}

void CanvasWidget::pickColorAt(const QPointF &widgetPosition)
{
    bool inside = false;
    const QPointF documentPosition = mapToDocument(widgetPosition, &inside);
    const QSize documentSize = m_controller->document().size;
    if (!inside || !documentSize.isValid())
    {
        return;
    }
    if (m_colorPickFrame.isNull() || m_colorPickFrameIndex != m_currentFrame)
    {
        Document document = hasPendingSelectionTransform()
                                ? displayDocumentWithPendingSelectionTransform()
                                : displayDocument();
        m_colorPickFrame = {};
        m_colorPickFrame = RenderEngine::render(document, m_currentFrame);
        m_colorPickFrameIndex = m_currentFrame;
        updateFrameCacheBudget();
    }
    if (m_colorPickFrame.isNull())
    {
        return;
    }
    const int x = std::clamp(static_cast<int>(documentPosition.x()),
        0,
        m_colorPickFrame.width() - 1);
    const int y = std::clamp(static_cast<int>(documentPosition.y()),
        0,
        m_colorPickFrame.height() - 1);
    QColor color = m_colorPickFrame.pixelColor(x, y);
    if (color.alpha() == 0)
    {
        return;
    }
    setBrushColor(color);
}

void CanvasWidget::updatePointerPosition(const QPointF &widgetPosition)
{
    QRect dirtyRect = pointerUpdateRect();
    bool inside = false;
    const QPointF position = mapToDocument(widgetPosition, &inside);
    m_pointerWidgetPosition = widgetPosition;
    m_pointerInside = inside;
    m_pointerOverWidget = true;
    emit pointerPositionChanged(position, inside);
    if (m_selectionMoveMode)
    {
        updateCursor();
    }
    dirtyRect = dirtyRect.united(pointerUpdateRect());
    if (!dirtyRect.isEmpty())
    {
        requestDisplayUpdate(dirtyRect);
    }
}

QRect CanvasWidget::pointerUpdateRect() const
{
    if (!m_pointerOverWidget || m_panning || m_rotatingCanvas || m_touchSequence
        || m_spacePressed || m_pickingColor
        || (m_tool != Tool::Brush && m_tool != Tool::Eraser
            && !m_tabletPointerEraser))
    {
        return {};
    }
    const qreal toolWidth = m_tabletPointerEraser || m_tool == Tool::Eraser
                                ? m_eraserWidth
                                : m_brushWidth;
    const qreal radius =
        std::max(1.0, toolWidth * uniformScale(documentTransform()) * 0.5);
    return QRectF(m_pointerWidgetPosition.x() - radius,
        m_pointerWidgetPosition.y() - radius,
        radius * 2.0,
        radius * 2.0)
        .adjusted(-4.0, -4.0, 4.0, 4.0)
        .toAlignedRect()
        .intersected(rect());
}

void CanvasWidget::updateCursor()
{
    if (m_rotatingCanvas || m_touchGestureActive)
    {
        setCursor(Qt::ClosedHandCursor);
        return;
    }
    if (m_touchSequence && !m_touchGestureSuppressed)
    {
        setCursor(Qt::OpenHandCursor);
        return;
    }
    if (m_zoomDragging)
    {
        setCursor(Qt::SizeHorCursor);
        return;
    }
    if (m_pickingColor)
    {
        setCursor(Qt::CrossCursor);
        return;
    }
    if (m_panning)
    {
        setCursor(Qt::ClosedHandCursor);
        return;
    }
    if (m_spacePressed)
    {
        setCursor(m_shiftPressed ? Qt::CrossCursor : Qt::OpenHandCursor);
        return;
    }
    if (m_selectionMoveMode
        && (m_movingSelection
            || (m_pointerOverWidget
                && selectionContains(mapToDocument(m_pointerWidgetPosition)))))
    {
        setCursor(Qt::SizeAllCursor);
        return;
    }
    if (m_groupSelectionActive
        && (m_tabletPointerEraser || m_tool == Tool::Brush
            || m_tool == Tool::Eraser || m_tool == Tool::Bucket))
    {
        setCursor(Qt::ForbiddenCursor);
        return;
    }
    if (m_tool == Tool::Text && !m_tabletPointerEraser)
    {
        setCursor(Qt::IBeamCursor);
        return;
    }
    const bool drawsWithRing = m_tabletPointerEraser || m_tool == Tool::Brush
                               || m_tool == Tool::Eraser;
    setCursor(drawsWithRing ? Qt::BlankCursor : Qt::CrossCursor);
}

void CanvasWidget::notifyZoomChanged()
{
    emit zoomChanged(std::max(1, qRound(std::abs(zoom()) * 100.0)));
}

}

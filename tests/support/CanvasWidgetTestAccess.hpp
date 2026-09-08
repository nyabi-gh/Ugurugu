// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#pragma once

#include "ui/CanvasWidget.hpp"

#include <QSize>

#include <atomic>
#include <memory>

namespace ugurugu
{

class CanvasWidgetTestAccess final
{
public:
    static QSize previewRenderSize(const CanvasWidget &canvas)
    {
        return canvas.previewRenderSize();
    }

    static CanvasWidget::DisplayedFrame resolveDisplayedFrame(
        CanvasWidget &canvas)
    {
        return canvas.resolveDisplayedFrame();
    }

    static bool usingGpuDisplay(const CanvasWidget &canvas)
    {
        return canvas.usingGpuDisplay();
    }

    // Identifies the baked canvas shadow. A repaint that reuses it keeps the
    // key; one that redraws the fourteen passes gets a new one.
    static qint64 shadowCacheKey(const CanvasWidget &canvas)
    {
        return canvas.m_shadowCache.isNull() ? 0
                                             : canvas.m_shadowCache.cacheKey();
    }

    static QSize cachedRenderSize(const CanvasWidget &canvas)
    {
        return canvas.m_cachedRenderSize;
    }

    static Document displayDocument(const CanvasWidget &canvas)
    {
        return canvas.displayDocument();
    }

    static bool zoomRenderPending(const CanvasWidget &canvas)
    {
        return canvas.m_zoomRenderTimer.isActive();
    }

    static bool hasCachedFrame(const CanvasWidget &canvas, int frame)
    {
        return canvas.m_frameCache.object(frame) != nullptr;
    }

    static bool frameCacheWarmupActive(const CanvasWidget &canvas)
    {
        return canvas.m_frameCacheWarmupActive;
    }

    static int frameCacheWarmupWorkerCount(const CanvasWidget &canvas)
    {
        return canvas.m_frameCacheWarmupWorkersRunning;
    }

    static qsizetype staleFrameCount(const CanvasWidget &canvas)
    {
        return canvas.m_frameCacheStaleFrames.size();
    }

    static qsizetype cachedFrameCount(const CanvasWidget &canvas)
    {
        return canvas.m_frameCache.size();
    }

    static PreviewSurfaceUsage previewSurfaceUsage(const CanvasWidget &canvas)
    {
        return canvas.previewSurfaceUsage();
    }

    static QImage selectionMask(const CanvasWidget &canvas)
    {
        return canvas.m_selectionMask;
    }

    static std::shared_ptr<const std::atomic_bool>
    selectionVisibilityCancellation(const CanvasWidget &canvas)
    {
        return canvas.m_selectionVisibilityCancellation;
    }

    static bool drawing(const CanvasWidget &canvas)
    {
        return canvas.m_drawing;
    }

    static Stroke activeStroke(const CanvasWidget &canvas)
    {
        return canvas.m_activeStroke;
    }

    static bool activeStrokePreviewIncludesStroke(const CanvasWidget &canvas)
    {
        return canvas.m_activeStrokePreviewIncludesStroke;
    }

    static QImage cachedFrame(const CanvasWidget &canvas, int frame)
    {
        const QImage *cached = canvas.m_frameCache.object(frame);
        return cached ? *cached : QImage();
    }

    static void beginStroke(
        CanvasWidget &canvas, const QPointF &widgetPosition, quint64 timestamp)
    {
        canvas.beginStroke(widgetPosition, 1.0, false, timestamp);
    }

    static void continueStroke(
        CanvasWidget &canvas, const QPointF &widgetPosition, quint64 timestamp)
    {
        canvas.continueStroke(widgetPosition, 1.0, timestamp);
    }

    static void endStroke(
        CanvasWidget &canvas, const QPointF &widgetPosition, quint64 timestamp)
    {
        canvas.endStroke(widgetPosition, timestamp);
    }

    static void advanceFrame(CanvasWidget &canvas)
    {
        canvas.advanceFrame();
    }

    static void stopAnimationTimer(CanvasWidget &canvas)
    {
        canvas.m_animationTimer.stop();
    }

    static quint64 synchronousPreviewRenderCount(const CanvasWidget &canvas)
    {
        return canvas.m_synchronousPreviewRenderCount;
    }

    static bool interactionFrameWarmupActive(const CanvasWidget &canvas)
    {
        return canvas.m_interactionFrameWarmupActive;
    }

    static bool hasCurrentInteractionBase(const CanvasWidget &canvas, int frame)
    {
        return (canvas.m_previewSplit.valid
                   && canvas.m_previewSplitFrame == frame)
               || (canvas.m_previewLayerRasters.valid
                   && canvas.m_previewLayerRasterFrame == frame);
    }

    static bool hasPreparedInteractionFrame(
        const CanvasWidget &canvas, int frame)
    {
        const QUuid layerId =
            canvas.m_drawing ? canvas.m_activeStrokeLayer
                             : canvas.m_controller->document().activeLayerId;
        return canvas.m_preparedInteractionFrame.matches(
            frame, canvas.previewRenderSize(), layerId);
    }

    static bool preparedInteractionUsesLayerRasters(const CanvasWidget &canvas)
    {
        return !canvas.m_preparedInteractionFrame.split.valid
               && canvas.m_preparedInteractionFrame.rasters.valid;
    }

    static void discardPreparedInteractionFrame(CanvasWidget &canvas)
    {
        canvas.clearPreparedInteractionFrame();
    }

    static void discardCurrentInteractionFrame(CanvasWidget &canvas)
    {
        canvas.m_previewSplit = {};
        canvas.m_previewSplitLayer = QUuid();
        canvas.m_previewSplitFrame = -1;
        canvas.m_previewLayerRasters = {};
        canvas.m_previewLayerRasterFrame = -1;
        canvas.clearPreparedInteractionFrame();
        canvas.invalidateActiveStrokePreview();
        canvas.m_incrementalStrokeRenderer.clear();
        canvas.m_composedPreviewFrame = {};
        canvas.m_composedSelectionPreviewRegion = {};
        canvas.m_composedPreviewBaseKey = 0;
    }

    static bool areaSelectionActive(const CanvasWidget &canvas)
    {
        return canvas.m_areaSelectionActive;
    }

    static QPointF mapToDocument(
        const CanvasWidget &canvas, const QPointF &widgetPosition)
    {
        return canvas.mapToDocument(widgetPosition);
    }

    static QPointF mapFromDocument(
        const CanvasWidget &canvas, const QPointF &documentPosition)
    {
        return canvas.documentTransform().map(documentPosition);
    }
};

}

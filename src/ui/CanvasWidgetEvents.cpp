// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "document/SelectionOperation.hpp"
#include "render/PreviewRenderPolicy.hpp"
#include "render/RenderEngine.hpp"
#include "ui/CanvasViewport.hpp"
#include "ui/CanvasWidget.hpp"
#include "ui/SelectionActionBar.hpp"
#include "ui/Theme.hpp"

#include <QEnterEvent>
#include <QKeyEvent>
#include <QMouseEvent>
#include <QNativeGestureEvent>
#include <QPaintEvent>
#include <QPainter>
#include <QPainterPath>
#include <QPolygonF>
#include <QResizeEvent>
#include <QTabletEvent>
#include <QTouchEvent>
#include <QWheelEvent>

#include <algorithm>
#include <cmath>

namespace ugurugu
{

using namespace canvas_detail;

namespace
{

// Alt stays the eyedropper for paint tools; only the selection tools read
// it as subtract, which resolves the modifier conflict without changing
// color picking anywhere else.
CanvasSelectionCombine selectionCombineForModifiers(
    Qt::KeyboardModifiers modifiers)
{
    const bool add = modifiers.testFlag(Qt::ShiftModifier);
    const bool subtract = modifiers.testFlag(Qt::AltModifier);
    if (add == subtract)
    {
        return CanvasSelectionCombine::Replace;
    }
    return add ? CanvasSelectionCombine::Add : CanvasSelectionCombine::Subtract;
}

bool acceptsRawCanvasTouch(const QTouchEvent *event)
{
    const QPointingDevice *device = event->pointingDevice();
    if (!device)
    {
        return false;
    }
    if (device->type() == QInputDevice::DeviceType::TouchScreen)
    {
        return true;
    }
#if defined(Q_OS_WIN)
    // Windows reports some external touch displays as TouchPad devices, while
    // desktop touchpads on other platforms stay on their native gesture path.
    return device->type() == QInputDevice::DeviceType::TouchPad;
#else
    return false;
#endif
}

}

bool CanvasWidget::handleTouchEvent(QTouchEvent *event)
{
    event->accept();
    if (m_touchSequence && event->pointingDevice() != m_touchDevice)
    {
        return true;
    }
    if (event->type() == QEvent::TouchCancel)
    {
        cancelTouchSequence();
        return true;
    }

    QVector<const QEventPoint *> activePoints;
    activePoints.reserve(event->points().size());
    for (const QEventPoint &point : event->points())
    {
        if (point.state() != QEventPoint::State::Released)
        {
            activePoints.append(&point);
        }
    }

    if (event->type() == QEvent::TouchBegin)
    {
        cancelTouchSequence();
        m_touchSequence = true;
        m_touchDevice = event->pointingDevice();
        m_touchGestureSuppressed = touchGestureConflictsWithActiveInteraction();
        updateCursor();
        requestDisplayUpdate();
    }
    else if (!m_touchSequence)
    {
        return true;
    }

    if (event->type() == QEvent::TouchEnd || activePoints.isEmpty())
    {
        cancelTouchSequence();
        return true;
    }
    if (activePoints.size() != 2)
    {
        endTouchGesture();
        return true;
    }
    if (m_touchGestureSuppressed)
    {
        return true;
    }
    if (touchGestureConflictsWithActiveInteraction())
    {
        suppressTouchSequence();
        return true;
    }

    const QEventPoint *firstPoint = activePoints.at(0);
    const QEventPoint *secondPoint = activePoints.at(1);
    if (firstPoint->id() > secondPoint->id())
    {
        std::swap(firstPoint, secondPoint);
    }
    if (!m_touchGestureActive || firstPoint->id() != m_touchFirstPointId
        || secondPoint->id() != m_touchSecondPointId)
    {
        beginTouchGesture(firstPoint->id(),
            firstPoint->position(),
            secondPoint->id(),
            secondPoint->position());
        return true;
    }

    continueTouchGesture(firstPoint->position(), secondPoint->position());
    return true;
}

bool CanvasWidget::event(QEvent *event)
{
    if (event->type() == QEvent::TouchBegin
        || event->type() == QEvent::TouchUpdate
        || event->type() == QEvent::TouchEnd
        || event->type() == QEvent::TouchCancel)
    {
        auto *touchEvent = static_cast<QTouchEvent *>(event);
        if (!m_touchSequence && !acceptsRawCanvasTouch(touchEvent))
        {
            return QWidget::event(event);
        }
        if (!m_touchSequence && m_selectionActionBar
            && m_selectionActionBar->isVisible())
        {
            const auto isActionBarTarget = [this](const QWidget *widget)
            {
                return widget
                       && (widget == m_selectionActionBar
                           || m_selectionActionBar->isAncestorOf(widget));
            };
            QWidget *target = qobject_cast<QWidget *>(touchEvent->target());
            if (!isActionBarTarget(target) && !touchEvent->points().isEmpty())
            {
                target =
                    childAt(touchEvent->points().first().position().toPoint());
            }
            if (isActionBarTarget(target))
            {
                return QWidget::event(event);
            }
        }
        return handleTouchEvent(touchEvent);
    }
    if (event->type() == QEvent::NativeGesture)
    {
        auto *gesture = static_cast<QNativeGestureEvent *>(event);
        if (gesture->gestureType() == Qt::ZoomNativeGesture)
        {
            if (!m_touchSequence && !m_drawing && !m_tabletSequence
                && !m_movingSelection && !m_areaSelectionActive
                && !m_textDragging)
            {
                zoomToward(m_zoom * std::pow(2.0, gesture->value()),
                    gesture->position());
            }
            event->accept();
            return true;
        }
        if (gesture->gestureType() == Qt::RotateNativeGesture)
        {
            if (!m_touchSequence && !m_drawing && !m_tabletSequence
                && !m_movingSelection && !m_areaSelectionActive
                && !m_textDragging)
            {
                rotateCanvasAround(
                    m_canvasRotation + gesture->value(), gesture->position());
            }
            event->accept();
            return true;
        }
    }
    switch (event->type())
    {
    case QEvent::FocusOut:
    case QEvent::UngrabMouse:
    case QEvent::TabletLeaveProximity:
    case QEvent::WindowDeactivate:
        cancelActiveInteraction();
        break;
    default:
        break;
    }
    return QWidget::event(event);
}

void CanvasWidget::paintEvent(QPaintEvent *event)
{
    QPainter painter(this);
    painter.fillRect(rect(), Theme::canvasBackground());
    if (usingGpuDisplay())
    {
        // The frame and overlay views cover the whole widget; the fill above
        // only keeps the opaque-paint-event contract for exposed edges.
        return;
    }
    painter.setRenderHint(QPainter::Antialiasing, true);
    painter.setRenderHint(QPainter::SmoothPixmapTransform, false);

    const Document document = displayDocument();
    const QTransform transform = documentTransform();
    const QRectF documentRect(QPointF(0.0, 0.0), QSizeF(document.size));
    const QPolygonF canvasPolygon = transform.map(QPolygonF(documentRect));
    QPainterPath canvasPath;
    canvasPath.addPolygon(canvasPolygon);
    canvasPath.closeSubpath();
    const QRectF canvasBounds = canvasPath.boundingRect();

    const QRectF visibleCanvasRect = canvasBounds.intersected(QRectF(rect()));
    const QRectF checkerRect =
        visibleCanvasRect.intersected(QRectF(event->rect()));
    if (!checkerRect.isEmpty())
    {
        painter.save();
        painter.setClipPath(canvasPath);
        painter.setBrushOrigin(canvasBounds.topLeft());
        painter.fillRect(checkerRect, checkerBrush());
        painter.restore();
    }

    painter.save();
    painter.setTransform(transform);
    painter.drawImage(documentRect, resolveDisplayedFrame().image);
    painter.restore();

    paintOverlay(painter, event->region());
}

// Everything painted around and above the frame pixels: the drop shadow
// (clipped to outside the canvas, so drawing it after the frame is
// equivalent), border, empty-document hint, selection outlines and the tool
// cursor. Kept free of background and frame drawing so a texture-backed
// display can run it over a transparent overlay unchanged.
// Redraws the shadow pixmap when the canvas outline or the device pixel ratio
// changed, and leaves it alone otherwise.
void CanvasWidget::refreshCanvasShadow(const QPolygonF &canvasPolygon)
{
    const qreal ratio = devicePixelRatioF();
    const QSize pixelSize = (QSizeF(size()) * ratio).toSize();
    if (pixelSize.isEmpty())
    {
        m_shadowCache = QPixmap();
        m_shadowCacheOutline = QPolygonF();
        return;
    }
    if (!m_shadowCache.isNull() && m_shadowCache.size() == pixelSize
        && qFuzzyCompare(m_shadowCacheRatio, ratio)
        && m_shadowCacheOutline == canvasPolygon)
    {
        return;
    }

    m_shadowCache = QPixmap(pixelSize);
    m_shadowCache.setDevicePixelRatio(ratio);
    m_shadowCache.fill(Qt::transparent);
    m_shadowCacheOutline = canvasPolygon;
    m_shadowCacheRatio = ratio;

    QPainterPath shadowPath;
    shadowPath.addPolygon(canvasPolygon);
    shadowPath.closeSubpath();
    shadowPath.translate(0.0, 2.0);

    QPainter shadowPainter(&m_shadowCache);
    shadowPainter.setRenderHint(QPainter::Antialiasing, true);
    shadowPainter.setBrush(Qt::NoBrush);
    for (int step = 14; step > 0; --step)
    {
        QColor shadow(Qt::black);
        shadow.setAlphaF(static_cast<float>(0.020 * (1.0 - step / 14.0)));
        shadowPainter.setPen(QPen(
            shadow, step * 2.0, Qt::SolidLine, Qt::RoundCap, Qt::RoundJoin));
        shadowPainter.drawPath(shadowPath);
    }
}

void CanvasWidget::paintOverlay(QPainter &painter, const QRegion &exposedRegion)
{
    painter.setRenderHint(QPainter::Antialiasing, true);
    painter.setRenderHint(QPainter::SmoothPixmapTransform, false);

    const Document document = displayDocument();
    const QTransform transform = documentTransform();
    const QRectF documentRect(QPointF(0.0, 0.0), QSizeF(document.size));
    const QPolygonF canvasPolygon = transform.map(QPolygonF(documentRect));
    QPainterPath canvasPath;
    canvasPath.addPolygon(canvasPolygon);
    canvasPath.closeSubpath();
    const QRectF canvasBounds = canvasPath.boundingRect();

    const QRegion shadowRegion = exposedRegion.subtracted(
        QRegion(canvasPolygon.toPolygon()).intersected(rect()));
    if (!shadowRegion.isEmpty())
    {
        // Baked once per outline instead of stroked on every repaint. The
        // fourteen antialiased passes are cheap while the canvas is square to
        // the screen and expensive the moment it is turned, so a rotated
        // canvas paid four times an ordinary repaint for a picture that only
        // changes when the geometry does.
        refreshCanvasShadow(canvasPolygon);
        if (!m_shadowCache.isNull())
        {
            painter.save();
            painter.setClipRegion(shadowRegion);
            painter.drawPixmap(rect(), m_shadowCache);
            painter.restore();
        }
    }

    painter.setPen(QPen(Theme::canvasBorder(), 1.0));
    painter.setBrush(Qt::NoBrush);
    painter.drawPath(canvasPath);

    if (!m_drawing && !documentHasStrokes(document))
    {
        painter.setPen(QColor(0x9A, 0x9E, 0xA6));
        QFont hintFont = painter.font();
        hintFont.setPointSizeF(hintFont.pointSizeF() * 0.95);
        painter.setFont(hintFont);
        painter.save();
        painter.setClipPath(canvasPath);
        painter.drawText(canvasBounds.adjusted(0.0, 0.0, 0.0, -14.0),
            Qt::AlignHCenter | Qt::AlignBottom,
            tr("B Brush · E Eraser · Space Pan · Scroll or Ctrl+Space "
               "Zoom · Shift+Space Rotate · P Play"));
        painter.restore();
    }

    drawSelectionOverlay(painter, transform);
    drawTextPlacementOverlay(painter, transform);

    const bool pointerUsesEraser =
        m_tabletPointerEraser || m_tool == Tool::Eraser;
    if (m_pointerOverWidget && !m_panning && !m_rotatingCanvas
        && !m_touchSequence && !m_spacePressed && !m_pickingColor
        && !m_groupSelectionActive
        && (m_tabletPointerEraser || m_tool == Tool::Brush
            || m_tool == Tool::Eraser))
    {
        const qreal toolWidth =
            pointerUsesEraser ? m_eraserWidth : m_brushWidth;
        const qreal radius =
            std::max(1.0, toolWidth * uniformScale(transform) * 0.5);
        const QRectF footprint(m_pointerWidgetPosition.x() - radius,
            m_pointerWidgetPosition.y() - radius,
            radius * 2.0,
            radius * 2.0);
        painter.setBrush(Qt::NoBrush);
        painter.setPen(QPen(QColor(20, 20, 20, 220), 3.0));
        painter.drawEllipse(footprint);
        QPen innerPen(QColor(250, 250, 250, 235), 1.0);
        if (pointerUsesEraser)
        {
            innerPen.setStyle(Qt::DashLine);
        }
        painter.setPen(innerPen);
        painter.drawEllipse(footprint);
    }
    updateSelectionActionBar();
}

void CanvasWidget::resizeEvent(QResizeEvent *event)
{
    QWidget::resizeEvent(event);
    syncDisplayViewGeometry();
    invalidateActiveStrokePreview();
    notifyZoomChanged();
    updateSelectionActionBar();
    requestDisplayUpdate();
}

void CanvasWidget::enterEvent(QEnterEvent *event)
{
    m_pointerOverWidget = true;
    updatePointerPosition(event->position());
    QWidget::enterEvent(event);
}

void CanvasWidget::mousePressEvent(QMouseEvent *event)
{
    if (m_touchSequence && !m_touchGestureSuppressed)
    {
        event->accept();
        return;
    }
    if (!m_tabletSequence && m_tabletPointerEraser)
    {
        const QRect pointerRect = pointerUpdateRect();
        m_tabletPointerEraser = false;
        updateCursor();
        if (!pointerRect.isEmpty())
        {
            requestDisplayUpdate(pointerRect);
        }
    }
    updatePointerPosition(event->position());
    setFocus(Qt::MouseFocusReason);
    if (event->button() == Qt::LeftButton && m_spacePressed)
    {
        if (event->modifiers().testFlag(Qt::ControlModifier))
        {
            beginZoomDrag(event->position());
        }
        else if (event->modifiers().testFlag(Qt::ShiftModifier))
        {
            beginCanvasRotation(event->position());
        }
        else
        {
            beginPan(event->position());
        }
        event->accept();
        return;
    }
    if (event->button() == Qt::MiddleButton)
    {
        beginPan(event->position());
        event->accept();
        return;
    }
    if (event->button() == Qt::LeftButton && !m_tabletSequence
        && event->modifiers().testFlag(Qt::AltModifier)
        && isColorPickableTool())
    {
        beginColorPick(event->position());
        event->accept();
        return;
    }
    if (event->button() == Qt::LeftButton && !m_tabletSequence)
    {
        const QPointF documentPosition = mapToDocument(event->position());
        if (m_selectionMoveMode)
        {
            if (selectionContains(documentPosition))
            {
                beginSelectionMove(documentPosition);
            }
            else
            {
                emit interactionMessage(
                    tr("Drag inside the selection to move it."));
            }
            event->accept();
            return;
        }
        // Sampling reads the composed frame rather than a layer, so it is the
        // one tool that stays useful on an empty document.
        if (m_tool == Tool::Eyedropper)
        {
            beginColorPick(event->position());
            event->accept();
            return;
        }
        const Document &document = m_controller->document();
        if (!document.layer(document.activeLayerId))
        {
            emit interactionMessage(tr("Add a layer before using this tool."));
            event->accept();
            return;
        }
        switch (m_tool)
        {
        case Tool::Brush:
        case Tool::Eraser:
            beginStroke(event->position(), 1.0, false, event->timestamp());
            break;
        case Tool::Eyedropper:
            break;
        case Tool::Lasso:
            beginAreaSelection(documentPosition,
                selectionCombineForModifiers(event->modifiers()));
            break;
        case Tool::Wand:
            computeWandSelection(documentPosition,
                selectionCombineForModifiers(event->modifiers()));
            break;
        case Tool::Bucket:
            applyBucketFill(documentPosition);
            break;
        case Tool::Text:
            beginTextInteraction(documentPosition);
            break;
        }
        event->accept();
        return;
    }
    QWidget::mousePressEvent(event);
}

void CanvasWidget::mouseMoveEvent(QMouseEvent *event)
{
    if (m_touchSequence && !m_touchGestureSuppressed)
    {
        event->accept();
        return;
    }
    if (!m_tabletSequence && m_tabletPointerEraser)
    {
        const QRect pointerRect = pointerUpdateRect();
        m_tabletPointerEraser = false;
        updateCursor();
        if (!pointerRect.isEmpty())
        {
            requestDisplayUpdate(pointerRect);
        }
    }
    updatePointerPosition(event->position());
    if (m_rotatingCanvas)
    {
        continueCanvasRotation(event->position());
        event->accept();
        return;
    }
    if (m_zoomDragging)
    {
        continueZoomDrag(event->position());
        event->accept();
        return;
    }
    if (m_pickingColor && !m_tabletSequence)
    {
        pickColorAt(event->position());
        event->accept();
        return;
    }
    if (m_panning)
    {
        continuePan(event->position());
        event->accept();
        return;
    }
    if (m_drawing && !m_tabletSequence)
    {
        continueStroke(event->position(), 1.0, event->timestamp());
        event->accept();
        return;
    }
    if (m_movingSelection)
    {
        continueSelectionMove(mapToDocument(event->position()));
        event->accept();
        return;
    }
    if (m_areaSelectionActive)
    {
        continueAreaSelection(mapToDocument(event->position()));
        event->accept();
        return;
    }
    if (m_textDragging && !m_tabletSequence)
    {
        continueTextDrag(mapToDocument(event->position()));
        event->accept();
        return;
    }
    QWidget::mouseMoveEvent(event);
}

void CanvasWidget::mouseReleaseEvent(QMouseEvent *event)
{
    if (m_touchSequence && !m_touchGestureSuppressed)
    {
        event->accept();
        return;
    }
    if (!m_tabletSequence && m_tabletPointerEraser)
    {
        const QRect pointerRect = pointerUpdateRect();
        m_tabletPointerEraser = false;
        updateCursor();
        if (!pointerRect.isEmpty())
        {
            requestDisplayUpdate(pointerRect);
        }
    }
    updatePointerPosition(event->position());
    if (m_rotatingCanvas && event->button() == Qt::LeftButton)
    {
        continueCanvasRotation(event->position());
        endCanvasRotation();
        event->accept();
        return;
    }
    if (m_zoomDragging && event->button() == Qt::LeftButton)
    {
        endZoomDrag();
        event->accept();
        return;
    }
    if (m_pickingColor && !m_tabletSequence
        && event->button() == Qt::LeftButton)
    {
        endColorPick();
        event->accept();
        return;
    }
    if (m_panning
        && (event->button() == Qt::MiddleButton
            || event->button() == Qt::LeftButton))
    {
        endPan();
        event->accept();
        return;
    }
    if (event->button() == Qt::LeftButton && m_drawing && !m_tabletSequence)
    {
        endStroke(event->position(), event->timestamp());
        event->accept();
        return;
    }
    if (event->button() == Qt::LeftButton && m_movingSelection)
    {
        continueSelectionMove(mapToDocument(event->position()));
        commitSelectionMove();
        event->accept();
        return;
    }
    if (event->button() == Qt::LeftButton && m_areaSelectionActive)
    {
        continueAreaSelection(mapToDocument(event->position()));
        finishAreaSelection();
        event->accept();
        return;
    }
    if (event->button() == Qt::LeftButton && m_textDragging
        && !m_tabletSequence)
    {
        continueTextDrag(mapToDocument(event->position()));
        endTextDrag();
        event->accept();
        return;
    }
    QWidget::mouseReleaseEvent(event);
}

void CanvasWidget::mouseDoubleClickEvent(QMouseEvent *event)
{
    if (m_touchSequence && !m_touchGestureSuppressed)
    {
        event->accept();
        return;
    }
    if (event->button() == Qt::MiddleButton)
    {
        fitToWindow();
        event->accept();
        return;
    }
    QWidget::mouseDoubleClickEvent(event);
}

void CanvasWidget::wheelEvent(QWheelEvent *event)
{
    const QPointF cursor = event->position();
    updatePointerPosition(cursor);
    if (m_touchSequence || m_drawing || m_tabletSequence || m_movingSelection
        || m_areaSelectionActive || m_textDragging)
    {
        event->accept();
        return;
    }
    if (event->modifiers().testFlag(Qt::ShiftModifier))
    {
        const QPoint angleDelta = event->angleDelta();
        const int wheelDelta =
            angleDelta.y() != 0 ? angleDelta.y() : angleDelta.x();
        if (wheelDelta != 0)
        {
            setCanvasRotation(
                m_canvasRotation + canvasRotationStep * wheelDelta / 120.0);
        }
        event->accept();
        return;
    }
    const qreal factor = std::pow(1.0015, event->angleDelta().y());
    zoomToward(m_zoom * factor, cursor);
    event->accept();
}

void CanvasWidget::tabletEvent(QTabletEvent *event)
{
    if (event->type() == QEvent::TabletPress && m_touchSequence
        && !m_touchGestureSuppressed)
    {
        suppressTouchSequence();
    }
    else if (m_touchSequence && !m_touchGestureSuppressed)
    {
        event->accept();
        return;
    }
    const QRect previousPointerRect = pointerUpdateRect();
    const bool eraser =
        event->pointerType() == QPointingDevice::PointerType::Eraser;
    m_tabletPointerEraser = eraser;
    updatePointerPosition(event->position());
    if (!previousPointerRect.isEmpty())
    {
        requestDisplayUpdate(previousPointerRect);
    }
    updateCursor();
    if (event->type() == QEvent::TabletPress)
    {
        if (m_tabletSequence)
        {
            cancelActiveInteraction();
            m_tabletPointerEraser = eraser;
            updateCursor();
        }
        if (event->button() != Qt::LeftButton)
        {
            QWidget::tabletEvent(event);
            return;
        }
        if (m_spacePressed)
        {
            m_tabletSequence = true;
            if (event->modifiers().testFlag(Qt::ControlModifier))
            {
                beginZoomDrag(event->position());
            }
            else if (event->modifiers().testFlag(Qt::ShiftModifier))
            {
                beginCanvasRotation(event->position());
            }
            else
            {
                beginPan(event->position());
            }
            event->accept();
            return;
        }
        if (event->modifiers().testFlag(Qt::AltModifier)
            && isColorPickableTool())
        {
            m_tabletSequence = true;
            beginColorPick(event->position());
            event->accept();
            return;
        }
        if (m_selectionMoveMode)
        {
            m_tabletSequence = true;
            const QPointF documentPosition = mapToDocument(event->position());
            if (selectionContains(documentPosition))
            {
                beginSelectionMove(documentPosition);
            }
            else
            {
                emit interactionMessage(
                    tr("Drag inside the selection to move it."));
            }
            event->accept();
            return;
        }
        m_tabletSequence = true;
        if (!eraser && m_tool == Tool::Lasso)
        {
            beginAreaSelection(mapToDocument(event->position()),
                selectionCombineForModifiers(event->modifiers()));
            event->accept();
            return;
        }
        if (!eraser && m_tool == Tool::Wand)
        {
            computeWandSelection(mapToDocument(event->position()),
                selectionCombineForModifiers(event->modifiers()));
            event->accept();
            return;
        }
        if (!eraser && m_tool == Tool::Bucket)
        {
            applyBucketFill(mapToDocument(event->position()));
            event->accept();
            return;
        }
        if (!eraser && m_tool == Tool::Eyedropper)
        {
            beginColorPick(event->position());
            event->accept();
            return;
        }
        if (!eraser && m_tool == Tool::Text)
        {
            beginTextInteraction(mapToDocument(event->position()));
            event->accept();
            return;
        }
        beginStroke(
            event->position(), event->pressure(), eraser, event->timestamp());
        event->accept();
        return;
    }
    if (event->type() == QEvent::TabletMove)
    {
        if (!m_tabletSequence)
        {
            event->accept();
            return;
        }
        if (m_rotatingCanvas)
        {
            continueCanvasRotation(event->position());
        }
        else if (m_zoomDragging)
        {
            continueZoomDrag(event->position());
        }
        else if (m_pickingColor)
        {
            pickColorAt(event->position());
        }
        else if (m_panning)
        {
            continuePan(event->position());
        }
        else if (m_movingSelection)
        {
            continueSelectionMove(mapToDocument(event->position()));
        }
        else if (m_areaSelectionActive)
        {
            continueAreaSelection(mapToDocument(event->position()));
        }
        else if (m_textDragging)
        {
            continueTextDrag(mapToDocument(event->position()));
        }
        else if (m_drawing)
        {
            continueStroke(
                event->position(), event->pressure(), event->timestamp());
        }
        event->accept();
        return;
    }
    if (event->type() == QEvent::TabletRelease && m_tabletSequence)
    {
        if (m_rotatingCanvas)
        {
            continueCanvasRotation(event->position());
            endCanvasRotation();
        }
        else if (m_zoomDragging)
        {
            endZoomDrag();
        }
        else if (m_pickingColor)
        {
            endColorPick();
        }
        else if (m_panning)
        {
            endPan();
        }
        else if (m_movingSelection)
        {
            continueSelectionMove(mapToDocument(event->position()));
            commitSelectionMove();
        }
        else if (m_areaSelectionActive)
        {
            continueAreaSelection(mapToDocument(event->position()));
            finishAreaSelection();
        }
        else if (m_textDragging)
        {
            continueTextDrag(mapToDocument(event->position()));
            endTextDrag();
        }
        else if (m_drawing)
        {
            endStroke(event->position(), event->timestamp());
        }
        m_tabletSequence = false;
        event->accept();
        return;
    }
    QWidget::tabletEvent(event);
}

void CanvasWidget::keyPressEvent(QKeyEvent *event)
{
    if (event->key() == Qt::Key_Shift && !event->isAutoRepeat())
    {
        m_shiftPressed = true;
        updateCursor();
        event->accept();
        return;
    }
    if (event->key() == Qt::Key_Space && !event->isAutoRepeat())
    {
        setPanModifierActive(true);
        event->accept();
        return;
    }
    if (event->key() == Qt::Key_Escape)
    {
        handleEscape();
        event->accept();
        return;
    }
    if ((event->key() == Qt::Key_Return || event->key() == Qt::Key_Enter)
        && m_textPlacementActive && m_tool == Tool::Text)
    {
        applyTextPlacement();
        event->accept();
        return;
    }
    if ((event->key() == Qt::Key_Delete || event->key() == Qt::Key_Backspace)
        && !m_selectedStrokes.isEmpty())
    {
        deleteSelection();
        event->accept();
        return;
    }
    QWidget::keyPressEvent(event);
}

void CanvasWidget::keyReleaseEvent(QKeyEvent *event)
{
    if (event->key() == Qt::Key_Shift && !event->isAutoRepeat())
    {
        m_shiftPressed = false;
        updateCursor();
        event->accept();
        return;
    }
    if (event->key() == Qt::Key_Space && !event->isAutoRepeat())
    {
        setPanModifierActive(false);
        event->accept();
        return;
    }
    QWidget::keyReleaseEvent(event);
}

void CanvasWidget::leaveEvent(QEvent *event)
{
    const QRect pointerRect = pointerUpdateRect();
    m_pointerInside = false;
    m_pointerOverWidget = false;
    emit pointerPositionChanged(QPointF(), false);
    if (!pointerRect.isEmpty())
    {
        requestDisplayUpdate(pointerRect);
    }
    QWidget::leaveEvent(event);
}
}

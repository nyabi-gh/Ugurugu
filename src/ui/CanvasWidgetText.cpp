// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "document/TextStrokeBuilder.hpp"
#include "ui/CanvasViewport.hpp"
#include "ui/CanvasWidget.hpp"
#include "ui/Theme.hpp"

#include <QFontMetricsF>
#include <QPainter>
#include <QRandomGenerator>

#include <algorithm>
#include <optional>

namespace ugurugu
{

namespace
{

constexpr qreal minimumTextFontSize = 8.0;
constexpr qreal maximumTextFontSize = 512.0;

}

QString CanvasWidget::textContent() const
{
    return m_textContent;
}

QString CanvasWidget::textFontFamily() const
{
    return m_textFontFamily;
}

qreal CanvasWidget::textFontSize() const
{
    return m_textFontSize;
}

bool CanvasWidget::textFilled() const
{
    return m_textFilled;
}

bool CanvasWidget::hasTextPlacement() const
{
    return m_textPlacementActive;
}

void CanvasWidget::setTextContent(const QString &text)
{
    if (m_textContent == text)
    {
        return;
    }
    m_textContent = text;
    emit textContentChanged(text);
    if (m_textPlacementActive)
    {
        requestDisplayUpdate();
    }
}

void CanvasWidget::setTextFontFamily(const QString &family)
{
    if (m_textFontFamily == family)
    {
        return;
    }
    m_textFontFamily = family;
    emit textFontFamilyChanged(family);
    if (m_textPlacementActive)
    {
        requestDisplayUpdate();
    }
}

void CanvasWidget::setTextFontSize(qreal size)
{
    const qreal clamped =
        std::clamp(size, minimumTextFontSize, maximumTextFontSize);
    if (qFuzzyCompare(m_textFontSize, clamped))
    {
        return;
    }
    m_textFontSize = clamped;
    emit textFontSizeChanged(clamped);
    if (m_textPlacementActive)
    {
        requestDisplayUpdate();
    }
}

void CanvasWidget::setTextFilled(bool filled)
{
    if (m_textFilled == filled)
    {
        return;
    }
    m_textFilled = filled;
    emit textFilledChanged(filled);
    if (m_textPlacementActive)
    {
        requestDisplayUpdate();
    }
}

QFont CanvasWidget::textFont() const
{
    QFont font = m_textFontFamily.isEmpty() ? QFont() : QFont(m_textFontFamily);
    font.setPixelSize(std::max(1, qRound(m_textFontSize)));
    return font;
}

QPainterPath CanvasWidget::textPreviewPath() const
{
    return TextStrokeBuilder::layoutPath(m_textContent, textFont())
        .translated(m_textAnchor);
}

QRectF CanvasWidget::textPlacementBounds() const
{
    const QPainterPath path = textPreviewPath();
    if (!path.isEmpty())
    {
        return path.boundingRect();
    }
    const QFontMetricsF metrics(textFont());
    return QRectF(m_textAnchor,
        QSizeF(std::max(metrics.averageCharWidth() * 4.0, 24.0),
            metrics.lineSpacing()));
}

void CanvasWidget::beginTextInteraction(const QPointF &documentPosition)
{
    if (m_textPlacementActive
        && textPlacementBounds()
            .adjusted(-8.0, -8.0, 8.0, 8.0)
            .contains(documentPosition))
    {
        m_textDragging = true;
        m_textDragStart = documentPosition;
        m_textAnchorAtDragStart = m_textAnchor;
        return;
    }
    m_textAnchor = clampedDocumentPosition(documentPosition);
    m_textDragging = false;
    if (!m_textPlacementActive)
    {
        m_textPlacementActive = true;
        emit textPlacementChanged(true);
    }
    requestDisplayUpdate();
}

void CanvasWidget::continueTextDrag(const QPointF &documentPosition)
{
    if (!m_textDragging)
    {
        return;
    }
    m_textAnchor =
        m_textAnchorAtDragStart + (documentPosition - m_textDragStart);
    requestDisplayUpdate();
}

void CanvasWidget::endTextDrag()
{
    m_textDragging = false;
}

void CanvasWidget::cancelTextPlacement()
{
    if (!m_textPlacementActive)
    {
        return;
    }
    m_textPlacementActive = false;
    m_textDragging = false;
    emit textPlacementChanged(false);
    requestDisplayUpdate();
}

bool CanvasWidget::applyTextPlacement()
{
    if (!m_textPlacementActive)
    {
        return false;
    }
    const Document &document = m_controller->document();
    const QUuid layerId = document.activeLayerId;
    const Layer *layer = document.layer(layerId);
    if (!layer || layer->kind != LayerKind::Paint)
    {
        emit interactionMessage(tr("Add a layer before using this tool."));
        return false;
    }
    if (m_groupSelectionActive)
    {
        emit interactionMessage(
            tr("Groups can't be painted on. Select a paint layer to draw."));
        return false;
    }
    if (!reportLayerAcceptsPaint(*layer))
    {
        return false;
    }

    TextStrokeBuilder::Options options;
    options.text = m_textContent;
    options.font = textFont();
    options.anchor = m_textAnchor;
    options.color = m_brushColor;
    options.outlineWidth = m_brushWidth;
    options.brush = m_brushSettings;
    options.brush.antialiasing = m_brushAntialiasing;
    options.filled = m_textFilled;
    options.canvasSize = document.size;
    options.baseSeed = QRandomGenerator::global()->generate64();
    QVector<Stroke> strokes = TextStrokeBuilder::build(options);
    if (strokes.isEmpty())
    {
        emit interactionMessage(tr("Type some text to place it."));
        return false;
    }
    if (!m_selectionMask.isNull() && m_selectionLayer == layerId)
    {
        for (Stroke &stroke : strokes)
        {
            stroke.clipMask = m_selectionMask;
        }
    }

    auto *undoStack = m_controller->undoStack();
    undoStack->beginMacro(tr("Add text"));
    std::optional<DocumentController::AddStrokeResult> failure;
    for (Stroke &stroke : strokes)
    {
        const DocumentController::AddStrokeResult result =
            m_controller->addStroke(layerId, std::move(stroke));
        if (result != DocumentController::AddStrokeResult::Added
            && result
                   != DocumentController::AddStrokeResult::
                       AddedWithResampledPoints)
        {
            failure = result;
            break;
        }
    }
    undoStack->endMacro();
    if (failure)
    {
        switch (*failure)
        {
        case DocumentController::AddStrokeResult::RejectedInvalidLayer:
            emit interactionMessage(
                tr("The text could not be added because its layer is no "
                   "longer available."));
            break;
        case DocumentController::AddStrokeResult::RejectedStrokeLimit:
            emit interactionMessage(
                tr("The text could not be added because the project stroke "
                   "limit was reached."));
            break;
        case DocumentController::AddStrokeResult::RejectedPointLimit:
            emit interactionMessage(
                tr("The text could not be added because the project point "
                   "limit was reached."));
            break;
        default:
            emit interactionMessage(tr("The text could not be added."));
            break;
        }
        return false;
    }
    emit colorUsed(m_brushColor);
    cancelTextPlacement();
    return true;
}

void CanvasWidget::drawTextPlacementOverlay(
    QPainter &painter, const QTransform &transform)
{
    if (!m_textPlacementActive || m_tool != Tool::Text)
    {
        return;
    }
    painter.save();
    const qreal scale = canvas_detail::uniformScale(transform);
    const QPainterPath path = transform.map(textPreviewPath());
    if (!path.isEmpty())
    {
        if (m_textFilled)
        {
            painter.fillPath(path, m_brushColor);
        }
        QPen pen(m_brushColor, std::max(0.5, m_brushWidth * scale));
        pen.setCapStyle(Qt::RoundCap);
        pen.setJoinStyle(Qt::RoundJoin);
        painter.setPen(pen);
        painter.setBrush(Qt::NoBrush);
        painter.drawPath(path);
    }

    QPainterPath documentPlacementPath;
    documentPlacementPath.addRect(textPlacementBounds().adjusted(
        -6.0 / scale, -6.0 / scale, 6.0 / scale, 6.0 / scale));
    const QPainterPath placementPath = transform.map(documentPlacementPath);
    const QRectF bounds = placementPath.boundingRect();
    QPen framePen(Theme::accent(), 1.0);
    framePen.setStyle(Qt::DashLine);
    painter.setPen(framePen);
    painter.setBrush(Qt::NoBrush);
    painter.drawPath(placementPath);

    painter.setPen(Theme::textMuted());
    const QString hint = m_textContent.trimmed().isEmpty()
                             ? tr("Type in the Text panel to see it here.")
                             : tr("Drag to move · Enter applies · Esc cancels");
    painter.drawText(QRectF(bounds.left(), bounds.bottom() + 4.0, 320.0, 24.0),
        Qt::AlignLeft | Qt::AlignTop,
        hint);
    painter.restore();
}

}

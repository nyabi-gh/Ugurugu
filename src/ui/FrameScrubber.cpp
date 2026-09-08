// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "ui/FrameScrubber.hpp"

#include "document/DocumentController.hpp"
#include "ui/CanvasWidget.hpp"
#include "ui/Theme.hpp"

#include <QKeyEvent>
#include <QMouseEvent>
#include <QPainter>
#include <QPainterPath>

#include <algorithm>
#include <cmath>

namespace ugurugu
{

namespace
{

constexpr qreal trackRadius = 6.0;
constexpr qreal slotMargin = 2.0;

}

FrameScrubber::FrameScrubber(
    DocumentController *controller, CanvasWidget *canvas, QWidget *parent)
    : QWidget(parent)
    , m_controller(controller)
    , m_canvas(canvas)
{
    setSizePolicy(QSizePolicy::Expanding, QSizePolicy::Fixed);
    setFocusPolicy(Qt::StrongFocus);
    setMouseTracking(true);
    setCursor(Qt::PointingHandCursor);
    setAccessibleName(tr("Frame scrubber"));
    updateAccessibleValue();

    connect(m_canvas,
        &CanvasWidget::currentFrameChanged,
        this,
        [this](int)
        {
            updateAccessibleValue();
            update();
        });
    connect(m_canvas,
        &CanvasWidget::animatingChanged,
        this,
        [this](bool)
        {
            update();
        });
    connect(m_controller,
        &DocumentController::documentChanged,
        this,
        [this]()
        {
            updateAccessibleValue();
            update();
        });
}

QSize FrameScrubber::sizeHint() const
{
    return QSize(360, 26);
}

QSize FrameScrubber::minimumSizeHint() const
{
    return QSize(160, 26);
}

int FrameScrubber::frameCount() const
{
    return std::max(1, m_controller->document().animationFrames);
}

QRectF FrameScrubber::trackRect() const
{
    return QRectF(rect()).adjusted(0.5, 2.5, -0.5, -2.5);
}

int FrameScrubber::frameAt(const QPointF &position) const
{
    const QRectF track = trackRect();
    const int frames = frameCount();
    const qreal slotWidth = track.width() / frames;
    if (slotWidth <= 0.0)
    {
        return 0;
    }
    const int frame =
        static_cast<int>(std::floor((position.x() - track.left()) / slotWidth));
    return std::clamp(frame, 0, frames - 1);
}

void FrameScrubber::scrubTo(const QPointF &position)
{
    m_canvas->setAnimating(false);
    m_canvas->setCurrentFrame(frameAt(position));
}

void FrameScrubber::updateAccessibleValue()
{
    const int frames = frameCount();
    const int current = std::clamp(m_canvas->currentFrame(), 0, frames - 1);
    setAccessibleDescription(tr("Frame %1 of %2").arg(current + 1).arg(frames));
}

void FrameScrubber::paintEvent(QPaintEvent *)
{
    QPainter painter(this);
    painter.setRenderHint(QPainter::Antialiasing, true);

    const QRectF track = trackRect();
    const int frames = frameCount();
    const qreal slotWidth = track.width() / frames;
    const int current = std::clamp(m_canvas->currentFrame(), 0, frames - 1);

    QPainterPath trackPath;
    trackPath.addRoundedRect(track, trackRadius, trackRadius);
    painter.fillPath(trackPath, Theme::statusBackground());

    if (m_hoverFrame >= 0 && m_hoverFrame < frames && m_hoverFrame != current)
    {
        const QRectF hoverRect(track.left() + slotWidth * m_hoverFrame,
            track.top() + slotMargin,
            slotWidth,
            track.height() - slotMargin * 2.0);
        painter.setPen(Qt::NoPen);
        painter.setBrush(Theme::controlBackground());
        painter.drawRoundedRect(
            hoverRect.adjusted(0.5, 0.0, -0.5, 0.0), 4.0, 4.0);
    }

    const bool animating = m_canvas->isAnimating();
    painter.setPen(Qt::NoPen);
    painter.setBrush(Theme::textDisabled());
    for (int frame = 0; frame < frames; ++frame)
    {
        if (!animating && frame == current)
        {
            continue;
        }
        const QPointF center(
            track.left() + slotWidth * (frame + 0.5), track.center().y());
        painter.drawEllipse(center, 1.4, 1.4);
    }

    if (animating)
    {
        constexpr qreal markerHeight = 3.0;
        const QRectF marker(track.left() + slotWidth * current,
            track.bottom() - slotMargin - markerHeight,
            std::max(slotWidth, 6.0),
            markerHeight);
        QColor markerColor = Theme::accent();
        markerColor.setAlphaF(0.55f);
        painter.setBrush(markerColor);
        painter.drawRoundedRect(marker.adjusted(0.5, 0.0, -0.5, 0.0), 1.5, 1.5);
    }
    else
    {
        const QRectF playhead(track.left() + slotWidth * current,
            track.top() + slotMargin,
            std::max(slotWidth, 6.0),
            track.height() - slotMargin * 2.0);
        painter.setBrush(Theme::accent());
        painter.drawRoundedRect(
            playhead.adjusted(0.5, 0.0, -0.5, 0.0), 4.0, 4.0);
    }

    painter.setBrush(Qt::NoBrush);
    painter.setPen(QPen(hasFocus() ? Theme::accent() : Theme::border(), 1.0));
    painter.drawPath(trackPath);
}

void FrameScrubber::mousePressEvent(QMouseEvent *event)
{
    if (event->button() == Qt::LeftButton)
    {
        m_dragging = true;
        scrubTo(event->position());
        return;
    }
    QWidget::mousePressEvent(event);
}

void FrameScrubber::mouseMoveEvent(QMouseEvent *event)
{
    if (m_dragging)
    {
        scrubTo(event->position());
        return;
    }
    const int hovered = frameAt(event->position());
    if (hovered != m_hoverFrame)
    {
        m_hoverFrame = hovered;
        update();
    }
}

void FrameScrubber::mouseReleaseEvent(QMouseEvent *event)
{
    if (event->button() == Qt::LeftButton)
    {
        m_dragging = false;
        return;
    }
    QWidget::mouseReleaseEvent(event);
}

void FrameScrubber::leaveEvent(QEvent *event)
{
    m_hoverFrame = -1;
    update();
    QWidget::leaveEvent(event);
}

void FrameScrubber::keyPressEvent(QKeyEvent *event)
{
    const int frames = frameCount();
    const int current = m_canvas->currentFrame();
    if (event->key() == Qt::Key_Left)
    {
        m_canvas->setAnimating(false);
        m_canvas->setCurrentFrame((current + frames - 1) % frames);
        return;
    }
    if (event->key() == Qt::Key_Right)
    {
        m_canvas->setAnimating(false);
        m_canvas->setCurrentFrame((current + 1) % frames);
        return;
    }
    QWidget::keyPressEvent(event);
}

}

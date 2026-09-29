// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "ui/ToolPopover.hpp"

#include "ui/Theme.hpp"

#include <QPainter>
#include <QPainterPath>
#include <QScreen>
#include <QVBoxLayout>

#include <algorithm>

namespace ugurugu
{

namespace
{

constexpr int shadowMargin = 18;
constexpr qreal frameRadius = 12.0;

}

ToolPopover::ToolPopover(QWidget *parent)
    : QWidget(parent, Qt::Popup | Qt::FramelessWindowHint)
{
    setAttribute(Qt::WA_TranslucentBackground);
    auto *layout = new QVBoxLayout(this);
    layout->setContentsMargins(shadowMargin + 14,
        shadowMargin + 12,
        shadowMargin + 14,
        shadowMargin + 12);
}

void ToolPopover::setContentWidget(QWidget *content)
{
    layout()->addWidget(content);
}

void ToolPopover::popupBeside(QWidget *anchor)
{
    if (m_lastHide.isValid() && m_lastHide.elapsed() < 160)
    {
        m_lastHide.invalidate();
        return;
    }
    adjustSize();
    QPoint target = anchor->mapToGlobal(
        QPoint(anchor->width() + 6 - shadowMargin, -6 - shadowMargin));
    if (const QScreen *screen = anchor->screen())
    {
        const QRect available = screen->availableGeometry();
        target.setX(std::clamp(
            target.x(), available.left(), available.right() - width() + 1));
        target.setY(std::clamp(
            target.y(), available.top(), available.bottom() - height() + 1));
    }
    move(target);
    show();
}

void ToolPopover::paintEvent(QPaintEvent *event)
{
    Q_UNUSED(event);
    QPainter painter(this);
    painter.setRenderHint(QPainter::Antialiasing);
    const QRectF frame = QRectF(rect()).adjusted(shadowMargin + 0.5,
        shadowMargin + 0.5,
        -shadowMargin - 0.5,
        -shadowMargin - 0.5);

    painter.setPen(Qt::NoPen);
    for (int step = shadowMargin - 3; step > 0; --step)
    {
        QColor shadow(Qt::black);
        shadow.setAlphaF(static_cast<float>(
            0.030 * (1.0 - static_cast<qreal>(step) / (shadowMargin - 3))));
        QPainterPath blur;
        blur.addRoundedRect(
            frame.adjusted(-step, -step + 2.0, step, step + 2.0),
            frameRadius + step,
            frameRadius + step);
        painter.fillPath(blur, shadow);
    }

    QPainterPath path;
    path.addRoundedRect(frame, frameRadius, frameRadius);
    painter.fillPath(path, Theme::panelBackground());
    painter.setPen(QPen(Theme::border(), 1.0));
    painter.drawPath(path);
}

void ToolPopover::showEvent(QShowEvent *event)
{
    QWidget::showEvent(event);
    emit popoverShown();
}

void ToolPopover::hideEvent(QHideEvent *event)
{
    QWidget::hideEvent(event);
    m_lastHide.start();
    emit popoverHidden();
}

}

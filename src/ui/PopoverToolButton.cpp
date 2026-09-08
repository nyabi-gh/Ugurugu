// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "ui/PopoverToolButton.hpp"

#include "ui/Theme.hpp"
#include "ui/ToolPopover.hpp"

#include <QMouseEvent>
#include <QPainter>
#include <QPainterPath>

#include <numbers>

namespace ugurugu
{

PopoverToolButton::PopoverToolButton(QWidget *parent)
    : QToolButton(parent)
{
    m_longPressTimer.setSingleShot(true);
    m_longPressTimer.setInterval(480);
    connect(&m_longPressTimer,
        &QTimer::timeout,
        this,
        [this]()
        {
            if (!isDown())
            {
                return;
            }
            setDown(false);
            if (defaultAction() && !defaultAction()->isChecked())
            {
                defaultAction()->trigger();
            }
            openPopover();
        });

    m_hoverAnimation.setDuration(420);
    m_hoverAnimation.setStartValue(0.0);
    m_hoverAnimation.setEndValue(2.0 * std::numbers::pi_v<qreal>);
    connect(&m_hoverAnimation,
        &QVariantAnimation::valueChanged,
        this,
        [this](const QVariant &value)
        {
            applyWobbleFrame(value.toReal());
        });
    connect(&m_hoverAnimation,
        &QVariantAnimation::finished,
        this,
        [this]()
        {
            if (defaultAction())
            {
                setIcon(defaultAction()->icon());
            }
        });
}

void PopoverToolButton::setPopover(ToolPopover *popover)
{
    m_popover = popover;
    update();
}

void PopoverToolButton::setHoverGlyph(IconGlyph glyph)
{
    m_hoverGlyph = glyph;
    m_hasHoverGlyph = true;
}

void PopoverToolButton::enterEvent(QEnterEvent *event)
{
    if (m_hasHoverGlyph
        && m_hoverAnimation.state() != QAbstractAnimation::Running)
    {
        m_hoverAnimation.start();
    }
    QToolButton::enterEvent(event);
}

void PopoverToolButton::applyWobbleFrame(qreal phase)
{
    const QColor color =
        isChecked() ? Theme::accentText() : Theme::textPrimary();
    const int size = iconSize().width();
    setIcon(QIcon(
        Icons::pixmap(m_hoverGlyph, size, color, phase, devicePixelRatio())));
}

void PopoverToolButton::mousePressEvent(QMouseEvent *event)
{
    if (event->button() == Qt::LeftButton)
    {
        m_checkedAtPress = isChecked();
        if (m_popover)
        {
            m_longPressTimer.start();
        }
    }
    QToolButton::mousePressEvent(event);
}

void PopoverToolButton::mouseReleaseEvent(QMouseEvent *event)
{
    m_longPressTimer.stop();
    const bool insideRelease = event->button() == Qt::LeftButton
                               && rect().contains(event->position().toPoint());
    QToolButton::mouseReleaseEvent(event);
    if (m_popover && insideRelease && m_checkedAtPress)
    {
        openPopover();
    }
}

void PopoverToolButton::paintEvent(QPaintEvent *event)
{
    QToolButton::paintEvent(event);
    if (!m_popover)
    {
        return;
    }
    QPainter painter(this);
    painter.setRenderHint(QPainter::Antialiasing);
    const QPointF corner(width() - 4.0, height() - 4.0);
    QPainterPath indicator;
    indicator.moveTo(corner);
    indicator.lineTo(corner + QPointF(-5.0, 0.0));
    indicator.lineTo(corner + QPointF(0.0, -5.0));
    indicator.closeSubpath();
    painter.fillPath(
        indicator, isChecked() ? Theme::accentText() : Theme::textMuted());
}

void PopoverToolButton::openPopover()
{
    if (m_popover)
    {
        m_popover->popupBeside(this);
    }
}

}

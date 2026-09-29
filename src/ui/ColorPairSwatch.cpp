// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "ui/ColorPairSwatch.hpp"

#include "ui/Theme.hpp"

#include <QMouseEvent>
#include <QPainter>

namespace ugurugu
{

ColorPairSwatch::ColorPairSwatch(QWidget *parent)
    : QWidget(parent)
{
    setObjectName(QStringLiteral("colorPairSwatch"));
    setCursor(Qt::PointingHandCursor);
    setFocusPolicy(Qt::ClickFocus);
    setToolTip(
        tr("Current and previous color. Click the rear swatch to swap."));
    refreshAccessibleText();
}

QColor ColorPairSwatch::currentColor() const
{
    return m_current;
}

QColor ColorPairSwatch::previousColor() const
{
    return m_previous;
}

QSize ColorPairSwatch::sizeHint() const
{
    return QSize(68, 58);
}

void ColorPairSwatch::setCurrentColor(const QColor &color)
{
    if (!color.isValid() || color == m_current)
    {
        return;
    }
    m_previous = m_current;
    m_current = color;
    refreshAccessibleText();
    update();
}

void ColorPairSwatch::paintEvent(QPaintEvent *)
{
    QPainter painter(this);
    painter.setRenderHint(QPainter::Antialiasing, true);
    paintColor(painter, previousRect(), m_previous, QColor(115, 120, 128), 1);
    paintColor(painter, currentRect(), m_current, Theme::accent(), 3);
}

void ColorPairSwatch::mousePressEvent(QMouseEvent *event)
{
    if (event->button() == Qt::LeftButton
        && previousRect().contains(event->position().toPoint()))
    {
        const QColor selected = m_previous;
        m_previous = m_current;
        m_current = selected;
        refreshAccessibleText();
        update();
        emit colorSelected(m_current);
        event->accept();
        return;
    }
    QWidget::mousePressEvent(event);
}

QRect ColorPairSwatch::currentRect() const
{
    return QRect(4, 3, 38, 38);
}

QRect ColorPairSwatch::previousRect() const
{
    return QRect(27, 18, 34, 34);
}

void ColorPairSwatch::paintColor(QPainter &painter,
    const QRect &rect,
    const QColor &color,
    const QColor &border,
    int borderWidth)
{
    constexpr int tile = 6;
    painter.save();
    painter.setClipRect(rect);
    for (int y = rect.top(); y <= rect.bottom(); y += tile)
    {
        for (int x = rect.left(); x <= rect.right(); x += tile)
        {
            const bool light =
                ((x - rect.left()) / tile + (y - rect.top()) / tile) % 2 == 0;
            painter.fillRect(QRect(x, y, tile, tile),
                light ? QColor(235, 235, 235) : QColor(170, 170, 170));
        }
    }
    painter.fillRect(rect, color);
    painter.restore();
    painter.setBrush(Qt::NoBrush);
    painter.setPen(QPen(border, borderWidth));
    const int inset = borderWidth / 2;
    painter.drawRoundedRect(rect.adjusted(inset, inset, -inset, -inset), 4, 4);
}

void ColorPairSwatch::refreshAccessibleText()
{
    setAccessibleName(tr("Current color %1, previous color %2")
            .arg(m_current.name(QColor::HexArgb),
                m_previous.name(QColor::HexArgb)));
}

}

// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "ui/WandReferenceButton.hpp"

#include "ui/Theme.hpp"

#include <QPainter>

#include <array>
#include <utility>

namespace ugurugu
{

WandReferenceButton::WandReferenceButton(CanvasWandReference reference,
    QString title,
    QString description,
    QWidget *parent)
    : PopoverOptionButton(std::move(title), std::move(description), parent)
    , m_reference(reference)
{
}

CanvasWandReference WandReferenceButton::reference() const
{
    return m_reference;
}

void WandReferenceButton::paintPreview(
    QPainter &painter, const QRectF &bounds) const
{
    const std::array<QRectF, 3> layers{
        QRectF(bounds.left() + 2.0, bounds.top() + 2.0, 34.0, 12.0),
        QRectF(bounds.left() + 5.0, bounds.top() + 14.0, 34.0, 12.0),
        QRectF(bounds.left() + 8.0, bounds.top() + 26.0, 34.0, 12.0)};

    for (int index = 2; index >= 0; --index)
    {
        const bool active =
            m_reference == CanvasWandReference::AllVisibleLayers
            || (m_reference == CanvasWandReference::ActiveLayer && index == 1)
            || (m_reference == CanvasWandReference::ReferenceLayers
                && index != 1);
        QColor fill = Theme::controlBackground();
        QColor border = Theme::border();
        if (active)
        {
            fill = Theme::accent();
            fill.setAlpha(64);
            border = Theme::accent();
        }
        painter.setBrush(fill);
        painter.setPen(QPen(border, active ? 1.4 : 1.0));
        painter.drawRoundedRect(layers.at(index), 3.0, 3.0);

        if (m_reference == CanvasWandReference::ReferenceLayers && active)
        {
            painter.setPen(Qt::NoPen);
            painter.setBrush(Theme::accent());
            painter.drawEllipse(QPointF(layers.at(index).right() - 4.0,
                                    layers.at(index).center().y()),
                2.0,
                2.0);
        }
    }
}

}

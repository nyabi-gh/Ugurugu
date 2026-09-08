// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "ui/SelectionShapeButton.hpp"

#include "ui/Theme.hpp"

#include <QPainter>
#include <QPainterPath>

#include <utility>

namespace ugurugu
{

SelectionShapeButton::SelectionShapeButton(CanvasSelectionShape shape,
    QString title,
    QString description,
    QWidget *parent)
    : PopoverOptionButton(std::move(title), std::move(description), parent)
    , m_shape(shape)
{
}

CanvasSelectionShape SelectionShapeButton::shape() const
{
    return m_shape;
}

void SelectionShapeButton::paintPreview(
    QPainter &painter, const QRectF &bounds) const
{
    QPainterPath path;
    const QRectF shapeBounds = bounds.adjusted(4.0, 5.0, -4.0, -5.0);
    switch (m_shape)
    {
    case CanvasSelectionShape::Freehand:
        path.moveTo(shapeBounds.left() + 2.0, shapeBounds.center().y() + 2.0);
        path.cubicTo(shapeBounds.left() - 1.0,
            shapeBounds.top() + 3.0,
            shapeBounds.center().x() - 2.0,
            shapeBounds.top() - 1.0,
            shapeBounds.center().x() + 3.0,
            shapeBounds.top() + 4.0);
        path.cubicTo(shapeBounds.right() + 3.0,
            shapeBounds.top() + 9.0,
            shapeBounds.right() - 1.0,
            shapeBounds.bottom() - 1.0,
            shapeBounds.center().x(),
            shapeBounds.bottom());
        path.cubicTo(shapeBounds.left() + 5.0,
            shapeBounds.bottom() + 1.0,
            shapeBounds.left() + 1.0,
            shapeBounds.center().y() + 7.0,
            shapeBounds.left() + 2.0,
            shapeBounds.center().y() + 2.0);
        break;
    case CanvasSelectionShape::Rectangle:
        path.addRoundedRect(shapeBounds, 2.0, 2.0);
        break;
    case CanvasSelectionShape::Ellipse:
        path.addEllipse(shapeBounds);
        break;
    }

    QColor fill = Theme::accent();
    fill.setAlpha(42);
    painter.setBrush(fill);
    QPen pen(Theme::accent(), 1.5);
    pen.setStyle(Qt::DashLine);
    painter.setPen(pen);
    painter.drawPath(path);
}

}

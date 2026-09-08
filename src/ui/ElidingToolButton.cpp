// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "ui/ElidingToolButton.hpp"

#include <QPaintEvent>
#include <QStyleOptionToolButton>
#include <QStylePainter>

namespace ugurugu
{

ElidingToolButton::ElidingToolButton(const QString &text, QWidget *parent)
    : QToolButton(parent)
{
    setText(text);
    setToolButtonStyle(Qt::ToolButtonTextOnly);
    // A tool button is horizontally QSizePolicy::Fixed by default, so layouts
    // take its full-text sizeHint as the minimum and never consult
    // minimumSizeHint below.
    setSizePolicy(QSizePolicy::Preferred, QSizePolicy::Fixed);
}

QSize ElidingToolButton::minimumSizeHint() const
{
    QStyleOptionToolButton option;
    initStyleOption(&option);
    const int ellipsis = fontMetrics().horizontalAdvance(QStringLiteral("…"));
    const QSize contents(ellipsis, fontMetrics().height());
    return style()
        ->sizeFromContents(QStyle::CT_ToolButton, &option, contents, this)
        .expandedTo(QSize(0, QToolButton::minimumSizeHint().height()));
}

void ElidingToolButton::paintEvent(QPaintEvent *event)
{
    Q_UNUSED(event);
    QStylePainter painter(this);
    QStyleOptionToolButton option;
    initStyleOption(&option);
    const QRect label = style()->subControlRect(
        QStyle::CC_ToolButton, &option, QStyle::SC_ToolButton, this);
    option.text =
        fontMetrics().elidedText(text(), Qt::ElideRight, label.width());
    painter.drawComplexControl(QStyle::CC_ToolButton, option);
}

}

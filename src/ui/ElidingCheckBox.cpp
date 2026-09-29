// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "ui/ElidingCheckBox.hpp"

#include <QPaintEvent>
#include <QStyleOptionButton>
#include <QStylePainter>

namespace ugurugu
{

ElidingCheckBox::ElidingCheckBox(const QString &text, QWidget *parent)
    : QCheckBox(text, parent)
{
    // A checkbox is horizontally QSizePolicy::Minimum by default, so layouts
    // take its full-text sizeHint as the minimum and never consult
    // minimumSizeHint below.
    setSizePolicy(QSizePolicy::Preferred, QSizePolicy::Fixed);
}

QSize ElidingCheckBox::minimumSizeHint() const
{
    QStyleOptionButton option;
    initStyleOption(&option);
    const int indicator =
        style()->pixelMetric(QStyle::PM_IndicatorWidth, &option, this);
    const int spacing =
        style()->pixelMetric(QStyle::PM_CheckBoxLabelSpacing, &option, this);
    const int ellipsis = fontMetrics().horizontalAdvance(QStringLiteral("…"));
    return QSize(
        indicator + spacing + ellipsis, QCheckBox::minimumSizeHint().height());
}

void ElidingCheckBox::paintEvent(QPaintEvent *event)
{
    Q_UNUSED(event);
    QStylePainter painter(this);
    QStyleOptionButton option;
    initStyleOption(&option);
    const QRect label =
        style()->subElementRect(QStyle::SE_CheckBoxContents, &option, this);
    option.text =
        fontMetrics().elidedText(text(), Qt::ElideRight, label.width());
    painter.drawControl(QStyle::CE_CheckBox, option);
}

}

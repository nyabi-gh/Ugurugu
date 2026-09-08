// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "ui/PopoverOptionButton.hpp"

#include "ui/Theme.hpp"

#include <QFontMetrics>
#include <QPainter>
#include <QPainterPath>
#include <QSizePolicy>

#include <algorithm>
#include <utility>

namespace ugurugu
{

namespace
{

constexpr int cardWidth = 264;
constexpr int cardPadding = 11;
constexpr int previewSize = 40;
constexpr int previewColumn = 66;
constexpr int titleGap = 1;
// The two text bands were laid out by hand against the default font. Keeping
// them as floors reproduces that layout exactly and lets a larger system font
// push them open instead of clipping.
constexpr int titleBandFloor = 19;
constexpr int descriptionBandFloor = 18;

int titleBandHeight(const QFont &font)
{
    return std::max(titleBandFloor, QFontMetrics(font).height());
}

int descriptionBandHeight(const QFont &font)
{
    return std::max(descriptionBandFloor, QFontMetrics(font).height());
}

QFont titleFont(const QFont &base)
{
    QFont font = Theme::scaledFont(base, Theme::TextRole::Title);
    font.setWeight(QFont::DemiBold);
    return font;
}

QFont descriptionFont(const QFont &base)
{
    return Theme::scaledFont(base, Theme::TextRole::Caption);
}

// The card is as tall as the preview until the system font grows past it, so
// the default layout is unchanged and larger text still gets its two lines.
int cardHeight(const QFont &base)
{
    const int textHeight = titleBandHeight(titleFont(base)) + titleGap
                           + descriptionBandHeight(descriptionFont(base));
    return 2 * cardPadding + std::max(previewSize, textHeight);
}

}

PopoverOptionButton::PopoverOptionButton(
    QString title, QString description, QWidget *parent)
    : QAbstractButton(parent)
    , m_title(std::move(title))
    , m_description(std::move(description))
{
    setCheckable(true);
    setCursor(Qt::PointingHandCursor);
    setToolTip(m_description);
    setAccessibleName(m_title);
    setAccessibleDescription(m_description);
    setAttribute(Qt::WA_Hover);
    setSizePolicy(QSizePolicy::Ignored, QSizePolicy::Preferred);
}

QSize PopoverOptionButton::sizeHint() const
{
    return QSize(cardWidth, cardHeight(font()));
}

void PopoverOptionButton::paintEvent(QPaintEvent *event)
{
    Q_UNUSED(event);
    QPainter painter(this);
    painter.setRenderHint(QPainter::Antialiasing);

    const QRectF frame = QRectF(rect()).adjusted(0.75, 0.75, -0.75, -0.75);
    QPainterPath card;
    card.addRoundedRect(frame, 8.0, 8.0);

    QColor background =
        isChecked() ? Theme::hoverBackground() : Theme::canvasBackground();
    if (underMouse() && !isChecked())
    {
        background = Theme::hoverBackground();
    }
    painter.fillPath(card, background);
    painter.setPen(isChecked() || hasFocus() ? QPen(Theme::accent(), 1.5)
                                             : QPen(Theme::border(), 1.0));
    painter.drawPath(card);

    paintPreview(painter, QRectF(12.0, cardPadding, 42.0, previewSize));

    const QFont title = titleFont(font());
    const QFont description = descriptionFont(font());
    const int titleHeight = titleBandHeight(title);
    const int descriptionHeight = descriptionBandHeight(description);
    const qreal textWidth = width() - previewColumn - 12.0;
    const qreal textTop = cardPadding;

    painter.setFont(title);
    painter.setPen(Theme::textPrimary());
    painter.drawText(QRectF(previewColumn, textTop, textWidth, titleHeight),
        Qt::AlignLeft | Qt::AlignVCenter,
        painter.fontMetrics().elidedText(
            m_title, Qt::ElideRight, qRound(textWidth)));

    painter.setFont(description);
    painter.setPen(Theme::textMuted());
    painter.drawText(QRectF(previewColumn,
                         textTop + titleHeight + titleGap,
                         textWidth,
                         descriptionHeight),
        Qt::AlignLeft | Qt::AlignVCenter,
        painter.fontMetrics().elidedText(
            m_description, Qt::ElideRight, qRound(textWidth)));
}

}

// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "ui/SelectionActionBar.hpp"

#include "ui/Theme.hpp"

#include <QAction>
#include <QFrame>
#include <QHBoxLayout>
#include <QPainter>
#include <QPainterPath>
#include <QShowEvent>
#include <QToolButton>

#include <algorithm>

namespace ugurugu
{

namespace
{

constexpr int shadowMargin = 10;
constexpr int framePadding = 5;
constexpr int buttonExtent = 34;
constexpr int iconExtent = 18;
constexpr int separatorHeight = 20;
constexpr qreal frameRadius = 10.0;

QString actionButtonName(const QAction *action, int fallbackIndex)
{
    QString name = action ? action->objectName() : QString();
    if (name.endsWith(QStringLiteral("Action")))
    {
        name.chop(6);
    }
    if (!name.isEmpty())
    {
        return name + QStringLiteral("Button");
    }
    return QStringLiteral("selectionActionButton%1").arg(fallbackIndex);
}

QString actionLabel(const QAction *action)
{
    if (!action)
    {
        return {};
    }
    QString label = action->text();
    label.remove(QLatin1Char('&'));
    return label;
}

void syncButtonMetadata(QToolButton *button, const QAction *action)
{
    if (!button || !action)
    {
        return;
    }
    const QString label = actionLabel(action);
    button->setAccessibleName(label);
    button->setToolTip(action->toolTip().isEmpty() ? label : action->toolTip());
}

}

SelectionActionBar::SelectionActionBar(QWidget *parent)
    : QWidget(parent)
{
    setObjectName(QStringLiteral("SelectionActionBar"));
    setAttribute(Qt::WA_TranslucentBackground);
    setAttribute(Qt::WA_NoSystemBackground);
    setAutoFillBackground(false);
    setSizePolicy(QSizePolicy::Fixed, QSizePolicy::Fixed);

    m_layout = new QHBoxLayout(this);
    m_layout->setContentsMargins(shadowMargin + framePadding,
        shadowMargin + framePadding,
        shadowMargin + framePadding,
        shadowMargin + framePadding);
    m_layout->setSpacing(3);
}

QToolButton *SelectionActionBar::addActionButton(QAction *action)
{
    if (!action)
    {
        return nullptr;
    }

    auto *button = new QToolButton(this);
    button->setObjectName(actionButtonName(action, ++m_buttonCount));
    button->setAutoRaise(true);
    button->setToolButtonStyle(Qt::ToolButtonIconOnly);
    button->setIconSize(QSize(iconExtent, iconExtent));
    button->setFixedSize(buttonExtent, buttonExtent);
    button->setFocusPolicy(Qt::StrongFocus);
    button->setDefaultAction(action);
    syncButtonMetadata(button, action);
    connect(action,
        &QAction::changed,
        button,
        [button, action]()
        {
            syncButtonMetadata(button, action);
        });
    m_layout->addWidget(button);
    updateGeometry();
    adjustSize();
    return button;
}

void SelectionActionBar::addSeparator()
{
    auto *separator = new QFrame(this);
    separator->setObjectName(
        QStringLiteral("selectionActionSeparator%1").arg(++m_separatorCount));
    separator->setFrameShape(QFrame::VLine);
    separator->setFrameShadow(QFrame::Plain);
    separator->setLineWidth(1);
    separator->setFixedHeight(separatorHeight);
    separator->setStyleSheet(QStringLiteral("color: %1;")
            .arg(Theme::border().name(QColor::HexArgb)));
    m_layout->addWidget(separator, 0, Qt::AlignVCenter);
    updateGeometry();
    adjustSize();
}

QSize SelectionActionBar::sizeHint() const
{
    const QSize layoutHint = m_layout ? m_layout->sizeHint() : QSize();
    return layoutHint.expandedTo(QSize(shadowMargin * 2 + framePadding * 2,
        shadowMargin * 2 + framePadding * 2 + buttonExtent));
}

QSize SelectionActionBar::minimumSizeHint() const
{
    return sizeHint();
}

void SelectionActionBar::paintEvent(QPaintEvent *event)
{
    Q_UNUSED(event);
    QPainter painter(this);
    painter.setRenderHint(QPainter::Antialiasing);

    const QRectF frame = QRectF(rect()).adjusted(shadowMargin + 0.5,
        shadowMargin + 0.5,
        -shadowMargin - 0.5,
        -shadowMargin - 0.5);
    if (!frame.isValid())
    {
        return;
    }

    painter.setPen(Qt::NoPen);
    for (int step = shadowMargin - 2; step > 0; --step)
    {
        QColor shadow(Qt::black);
        shadow.setAlphaF(static_cast<float>(
            0.035 * (1.0 - static_cast<qreal>(step) / (shadowMargin - 2))));
        QPainterPath blur;
        blur.addRoundedRect(
            frame.adjusted(-step, -step + 2.0, step, step + 2.0),
            frameRadius + step,
            frameRadius + step);
        painter.fillPath(blur, shadow);
    }

    QPainterPath panel;
    panel.addRoundedRect(frame, frameRadius, frameRadius);
    painter.fillPath(panel, Theme::panelBackground());
    painter.setPen(QPen(Theme::border(), 1.0));
    painter.drawPath(panel);
}

void SelectionActionBar::showEvent(QShowEvent *event)
{
    adjustSize();
    QWidget::showEvent(event);
    raise();
}

}

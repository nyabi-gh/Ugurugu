// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "ui/ColorHistoryGrid.hpp"

#include "ui/Theme.hpp"

#include <QGridLayout>
#include <QResizeEvent>
#include <QSettings>
#include <QSizePolicy>
#include <QStringList>
#include <QToolButton>

#include <algorithm>

namespace ugurugu
{

namespace
{

constexpr int historyCapacity = 256;
constexpr int swatchSize = 22;
constexpr int swatchSpacing = 2;
constexpr int gridMargin = 2;
constexpr QLatin1StringView historyKey("brush/colorHistory");
constexpr QLatin1StringView legacyRecentColorsKey("brush/recentColors");

}

ColorHistoryGrid::ColorHistoryGrid(QWidget *parent)
    : QWidget(parent)
{
    setObjectName(QStringLiteral("colorHistoryGrid"));
    setSizePolicy(QSizePolicy::Preferred, QSizePolicy::Fixed);
    m_persistTimer.setSingleShot(true);
    m_persistTimer.setInterval(150);
    connect(
        &m_persistTimer, &QTimer::timeout, this, &ColorHistoryGrid::persist);

    m_layout = new QGridLayout(this);
    m_layout->setSizeConstraint(QLayout::SetNoConstraint);
    m_layout->setContentsMargins(
        gridMargin, gridMargin, gridMargin, gridMargin);
    m_layout->setHorizontalSpacing(swatchSpacing);
    m_layout->setVerticalSpacing(swatchSpacing);

    for (int index = 0; index < historyCapacity; ++index)
    {
        auto *button = new QToolButton(this);
        button->setFixedSize(swatchSize, swatchSize);
        button->setCursor(Qt::PointingHandCursor);
        connect(button,
            &QToolButton::clicked,
            this,
            [this, index]()
            {
                if (index < m_colors.size())
                {
                    emit colorSelected(m_colors[index]);
                }
            });
        m_buttons.append(button);
    }
    relayoutForWidth(288);

    QSettings settings;
    QStringList stored = settings.value(historyKey).toStringList();
    if (stored.isEmpty())
    {
        stored = settings.value(legacyRecentColorsKey).toStringList();
    }
    for (const QString &name : stored)
    {
        const QColor color(name);
        if (color.isValid() && !m_colors.contains(color)
            && m_colors.size() < historyCapacity)
        {
            m_colors.append(color);
        }
    }
    refreshButtons();
}

void ColorHistoryGrid::resizeEvent(QResizeEvent *event)
{
    QWidget::resizeEvent(event);
    relayoutForWidth(event->size().width());
}

void ColorHistoryGrid::relayoutForWidth(int width)
{
    const int availableWidth = std::max(1, width - gridMargin * 2);
    const int columns = std::max(
        1, (availableWidth + swatchSpacing) / (swatchSize + swatchSpacing));
    if (columns == m_columns)
    {
        return;
    }
    m_columns = columns;
    for (int index = 0; index < m_buttons.size(); ++index)
    {
        m_layout->addWidget(
            m_buttons[index], index / m_columns, index % m_columns);
    }
    const int rows = static_cast<int>(
        (m_buttons.size() + m_columns - 1) / std::max(1, m_columns));
    setMinimumHeight(gridMargin * 2 + rows * swatchSize
                     + std::max(0, rows - 1) * swatchSpacing);
    updateGeometry();
}

ColorHistoryGrid::~ColorHistoryGrid()
{
    if (m_persistTimer.isActive())
    {
        persist();
    }
}

QSize ColorHistoryGrid::minimumSizeHint() const
{
    return QSize(swatchSize + gridMargin * 2, 0);
}

void ColorHistoryGrid::setActiveColor(const QColor &color)
{
    if (!color.isValid() || m_activeColor == color)
    {
        return;
    }
    m_activeColor = color;
    refreshButtons();
}

void ColorHistoryGrid::recordColor(const QColor &color)
{
    if (!color.isValid())
    {
        return;
    }
    m_activeColor = color;
    m_colors.removeAll(color);
    m_colors.prepend(color);
    while (m_colors.size() > historyCapacity)
    {
        m_colors.removeLast();
    }
    refreshButtons();
    m_persistTimer.start();
}

void ColorHistoryGrid::clear()
{
    m_colors.clear();
    refreshButtons();
    m_persistTimer.stop();
    persist();
}

void ColorHistoryGrid::refreshButtons()
{
    for (int index = 0; index < m_buttons.size(); ++index)
    {
        QToolButton *button = m_buttons[index];
        const bool hasColor = index < m_colors.size();
        button->setEnabled(hasColor);
        if (!hasColor)
        {
            button->setToolTip({});
            button->setAccessibleName(tr("Empty color history slot"));
            button->setStyleSheet(QStringLiteral(
                "QToolButton { background: rgba(255, 255, 255, 8); "
                "border: 1px solid rgba(255, 255, 255, 24); }"));
            continue;
        }
        const QColor color = m_colors[index];
        const bool active = color == m_activeColor;
        button->setToolTip(color.name(QColor::HexArgb));
        button->setAccessibleName(
            tr("History color %1").arg(color.name(QColor::HexArgb)));
        button->setStyleSheet(
            QStringLiteral("QToolButton { background: %1; border: %2; }"
                           "QToolButton:hover { border-color: %3; }"
                           "QToolButton:focus { border-color: %3; }")
                .arg(color.name(QColor::HexArgb),
                    active
                        ? QStringLiteral("2px solid %1")
                              .arg(Theme::accent().name())
                        : QStringLiteral("1px solid rgba(255, 255, 255, 60)"),
                    Theme::accent().name()));
    }
}

void ColorHistoryGrid::persist() const
{
    QStringList names;
    names.reserve(m_colors.size());
    for (const QColor &color : m_colors)
    {
        names.append(color.name(QColor::HexArgb));
    }
    QSettings().setValue(historyKey, names);
}

}

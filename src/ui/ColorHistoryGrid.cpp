// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "ui/ColorHistoryGrid.hpp"

#include "ui/Theme.hpp"

#include <QGridLayout>
#include <QPaintEvent>
#include <QPainter>
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

// Paints its own swatch. A style sheet per button made every recorded color
// re-polish all 256 buttons, since a new color shifts every slot.
class ColorSwatchButton final : public QToolButton
{
public:
    using QToolButton::QToolButton;

    void setSwatch(const QColor &color, bool active)
    {
        if (m_color == color && m_active == active)
        {
            return;
        }
        m_color = color;
        m_active = active;
        update();
    }

protected:
    void paintEvent(QPaintEvent *) override
    {
        QPainter painter(this);
        const QRect bounds = rect();
        if (!m_color.isValid())
        {
            painter.fillRect(bounds, QColor(255, 255, 255, 8));
            painter.setPen(QColor(255, 255, 255, 24));
            painter.drawRect(bounds.adjusted(0, 0, -1, -1));
            return;
        }
        painter.fillRect(bounds, m_color);
        const bool highlighted = underMouse() || hasFocus();
        const int borderWidth = m_active ? 2 : 1;
        const QColor border = m_active || highlighted
                                  ? Theme::accent()
                                  : QColor(255, 255, 255, 60);
        for (int inset = 0; inset < borderWidth; ++inset)
        {
            painter.setPen(border);
            painter.drawRect(
                bounds.adjusted(inset, inset, -1 - inset, -1 - inset));
        }
    }

    void enterEvent(QEnterEvent *event) override
    {
        QToolButton::enterEvent(event);
        update();
    }

    void leaveEvent(QEvent *event) override
    {
        QToolButton::leaveEvent(event);
        update();
    }

private:
    QColor m_color;
    bool m_active = false;
};

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
        auto *button = new ColorSwatchButton(this);
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
        auto *button = static_cast<ColorSwatchButton *>(m_buttons[index]);
        const bool hasColor = index < m_colors.size();
        const QColor color = hasColor ? m_colors[index] : QColor();
        if (button->isEnabled() != hasColor)
        {
            button->setEnabled(hasColor);
        }
        button->setSwatch(color, hasColor && color == m_activeColor);
        const QString name = hasColor ? color.name(QColor::HexArgb) : QString();
        if (button->toolTip() != name)
        {
            button->setToolTip(name);
            button->setAccessibleName(hasColor
                                          ? tr("History color %1").arg(name)
                                          : tr("Empty color history slot"));
        }
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

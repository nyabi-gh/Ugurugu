// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "ui/ColorHistoryDock.hpp"

#include "ui/CanvasWidget.hpp"
#include "ui/ColorHistoryGrid.hpp"
#include "ui/ElidingToolButton.hpp"
#include "ui/PaletteDockTitleBar.hpp"

#include <QHBoxLayout>
#include <QScrollArea>
#include <QVBoxLayout>

namespace ugurugu
{

ColorHistoryDock::ColorHistoryDock(CanvasWidget *canvas, QWidget *parent)
    : QDockWidget(tr("Color history"), parent)
    , m_canvas(canvas)
{
    setObjectName(QStringLiteral("ColorHistoryDock"));
    setFeatures(QDockWidget::DockWidgetMovable
                | QDockWidget::DockWidgetFloatable
                | QDockWidget::DockWidgetClosable);
    setMinimumWidth(150);
    installCompactPaletteTitleBar(this);

    auto *body = new QWidget(this);
    auto *layout = new QVBoxLayout(body);
    layout->setContentsMargins(6, 6, 6, 6);
    layout->setSpacing(4);

    auto *actions = new QHBoxLayout;
    actions->setContentsMargins(0, 0, 0, 0);
    actions->addStretch(1);
    auto *clearButton = new ElidingToolButton(tr("Clear history"), body);
    clearButton->setObjectName(QStringLiteral("clearColorHistoryButton"));
    actions->addWidget(clearButton);
    layout->addLayout(actions);

    m_grid = new ColorHistoryGrid(body);
    auto *scroll = new QScrollArea(body);
    scroll->setObjectName(QStringLiteral("colorHistoryScrollArea"));
    scroll->setWidgetResizable(true);
    scroll->setFrameShape(QFrame::NoFrame);
    scroll->setHorizontalScrollBarPolicy(Qt::ScrollBarAlwaysOff);
    scroll->setWidget(m_grid);
    layout->addWidget(scroll, 1);
    setWidget(body);

    connect(m_canvas,
        &CanvasWidget::brushColorChanged,
        m_grid,
        &ColorHistoryGrid::setActiveColor);
    connect(m_canvas,
        &CanvasWidget::colorUsed,
        m_grid,
        &ColorHistoryGrid::recordColor);
    connect(m_grid,
        &ColorHistoryGrid::colorSelected,
        m_canvas,
        &CanvasWidget::setBrushColor);
    connect(
        clearButton, &QToolButton::clicked, m_grid, &ColorHistoryGrid::clear);
    m_grid->setActiveColor(m_canvas->brushColor());
}

}

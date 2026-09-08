// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "ui/ToolDock.hpp"

#include "ui/BrushPopoverPanel.hpp"
#include "ui/BucketPopoverPanel.hpp"
#include "ui/CanvasWidget.hpp"
#include "ui/EraserPopoverPanel.hpp"
#include "ui/LassoPopoverPanel.hpp"
#include "ui/PaletteDockTitleBar.hpp"
#include "ui/TextPopoverPanel.hpp"
#include "ui/WandPopoverPanel.hpp"

#include <QLabel>
#include <QLatin1StringView>
#include <QScrollArea>
#include <QSettings>
#include <QStackedWidget>
#include <QVBoxLayout>

#include <algorithm>

namespace ugurugu
{

namespace
{

constexpr QLatin1StringView dockWidthKey("dock/toolWidth");
constexpr int minimumDockWidth = 150;
constexpr int maximumDockWidth = 460;
constexpr int defaultDockWidth = 330;

}

ToolDock::ToolDock(CanvasWidget *canvas, QWidget *parent)
    : QDockWidget(tr("Tool settings"), parent)
    , m_canvas(canvas)
{
    setObjectName(QStringLiteral("ToolDock"));
    setFeatures(QDockWidget::DockWidgetMovable
                | QDockWidget::DockWidgetFloatable
                | QDockWidget::DockWidgetClosable);
    setMinimumWidth(minimumDockWidth);
    installCompactPaletteTitleBar(this);
    buildPanels();
}

void ToolDock::buildPanels()
{
    m_toolPanels = new QStackedWidget(this);
    m_toolPanels->setSizePolicy(QSizePolicy::Ignored, QSizePolicy::Preferred);
    const auto addToolPanel = [this](CanvasTool tool, QWidget *panel)
    {
        auto *page = new QWidget(m_toolPanels);
        auto *layout = new QVBoxLayout(page);
        layout->setContentsMargins(8, 8, 8, 8);
        layout->setSpacing(0);
        layout->addWidget(panel, 0, Qt::AlignTop);
        layout->addStretch(1);
        m_toolPanelIndexes.insert(
            static_cast<int>(tool), m_toolPanels->addWidget(page));
    };

    auto *brushPanel = new BrushPopoverPanel(m_canvas);
    brushPanel->setAnimationActive(true);
    addToolPanel(CanvasTool::Brush, brushPanel);
    addToolPanel(CanvasTool::Eraser, new EraserPopoverPanel(m_canvas));
    addToolPanel(CanvasTool::Lasso, new LassoPopoverPanel(m_canvas));
    addToolPanel(CanvasTool::Wand, new WandPopoverPanel(m_canvas));
    addToolPanel(CanvasTool::Bucket, new BucketPopoverPanel(m_canvas));
    addToolPanel(CanvasTool::Text, new TextPopoverPanel(m_canvas));

    auto *eyedropperPanel = new QLabel(
        tr("Click the canvas to pick up its color.\nAlt+click does the same "
           "from the brush, eraser or paint bucket."),
        m_toolPanels);
    eyedropperPanel->setWordWrap(true);
    eyedropperPanel->setProperty("fieldLabel", true);
    addToolPanel(CanvasTool::Eyedropper, eyedropperPanel);

    auto *scroll = new QScrollArea(this);
    scroll->setObjectName(QStringLiteral("toolSettingsScrollArea"));
    scroll->setWidgetResizable(true);
    scroll->setFrameShape(QFrame::NoFrame);
    scroll->setHorizontalScrollBarPolicy(Qt::ScrollBarAlwaysOff);
    scroll->setWidget(m_toolPanels);
    setWidget(scroll);

    connect(
        m_canvas, &CanvasWidget::toolChanged, this, &ToolDock::showToolPanel);
    showToolPanel(m_canvas->tool());
}

void ToolDock::showToolPanel(CanvasTool tool)
{
    const auto index = m_toolPanelIndexes.constFind(static_cast<int>(tool));
    if (index != m_toolPanelIndexes.cend())
    {
        m_toolPanels->setCurrentIndex(*index);
    }
}

int ToolDock::preferredWidth() const
{
    return std::clamp(QSettings().value(dockWidthKey, defaultDockWidth).toInt(),
        minimumDockWidth,
        maximumDockWidth);
}

void ToolDock::rememberWidth() const
{
    if (!isFloating() && width() > 0)
    {
        QSettings().setValue(dockWidthKey, width());
    }
}

}

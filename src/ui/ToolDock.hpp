// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#pragma once

#include "ui/CanvasTypes.hpp"

#include <QDockWidget>
#include <QHash>

class QStackedWidget;

namespace ugurugu
{

class CanvasWidget;

class ToolDock final : public QDockWidget
{
    Q_OBJECT

public:
    explicit ToolDock(CanvasWidget *canvas, QWidget *parent = nullptr);

    int preferredWidth() const;
    void rememberWidth() const;

private:
    void buildPanels();
    void showToolPanel(CanvasTool tool);

    CanvasWidget *m_canvas = nullptr;
    QStackedWidget *m_toolPanels = nullptr;
    QHash<int, int> m_toolPanelIndexes;
};

}

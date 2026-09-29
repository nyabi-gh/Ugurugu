// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#pragma once

#include <QDockWidget>

namespace ugurugu
{

class CanvasWidget;
class ColorHistoryGrid;

class ColorHistoryDock final : public QDockWidget
{
    Q_OBJECT

public:
    explicit ColorHistoryDock(CanvasWidget *canvas, QWidget *parent = nullptr);

private:
    CanvasWidget *m_canvas = nullptr;
    ColorHistoryGrid *m_grid = nullptr;
};

}

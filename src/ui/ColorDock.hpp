// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#pragma once

#include <QDockWidget>

class QLabel;

namespace ugurugu
{

class CanvasWidget;
class ColorPairSwatch;
class ColorWheel;

class ColorDock final : public QDockWidget
{
    Q_OBJECT

public:
    explicit ColorDock(CanvasWidget *canvas, QWidget *parent = nullptr);

private:
    CanvasWidget *m_canvas = nullptr;
    ColorWheel *m_colorWheel = nullptr;
    ColorPairSwatch *m_colorSwatch = nullptr;
    QLabel *m_colorValue = nullptr;
    bool m_updatingColor = false;
};

}

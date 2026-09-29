// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#pragma once

#include <QWidget>

namespace ugurugu
{

class CanvasWidget;

class EraserPopoverPanel final : public QWidget
{
    Q_OBJECT

public:
    explicit EraserPopoverPanel(
        CanvasWidget *canvas, QWidget *parent = nullptr);
};

}

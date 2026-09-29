// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#pragma once

#include <QWidget>

namespace ugurugu
{

class CanvasWidget;

class WandPopoverPanel final : public QWidget
{
    Q_OBJECT

public:
    explicit WandPopoverPanel(CanvasWidget *canvas, QWidget *parent = nullptr);
};

}

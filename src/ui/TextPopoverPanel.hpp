// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#pragma once

#include <QWidget>

namespace ugurugu
{

class CanvasWidget;

class TextPopoverPanel final : public QWidget
{
    Q_OBJECT

public:
    explicit TextPopoverPanel(CanvasWidget *canvas, QWidget *parent = nullptr);
};

}

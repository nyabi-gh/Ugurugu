// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#pragma once

#include <QWidget>

namespace ugurugu
{

class CanvasWidget;

class BucketPopoverPanel final : public QWidget
{
    Q_OBJECT

public:
    explicit BucketPopoverPanel(
        CanvasWidget *canvas, QWidget *parent = nullptr);
};

}

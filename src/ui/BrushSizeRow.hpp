// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#pragma once

#include <QWidget>

namespace ugurugu
{

class CanvasWidget;

class BrushSizeRow final : public QWidget
{
    Q_OBJECT

public:
    enum class Target
    {
        Brush,
        Eraser
    };

    BrushSizeRow(CanvasWidget *canvas,
        Target target,
        const QString &objectNamePrefix,
        QWidget *parent = nullptr);
};

}

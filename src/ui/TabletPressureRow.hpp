// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#pragma once

#include <QWidget>

namespace ugurugu
{

class CanvasWidget;

class TabletPressureRow final : public QWidget
{
    Q_OBJECT

public:
    TabletPressureRow(CanvasWidget *canvas,
        const QString &objectNamePrefix,
        QWidget *parent = nullptr);
};

}

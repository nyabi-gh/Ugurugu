// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#pragma once

#include <QCheckBox>

namespace ugurugu
{

// A checkbox whose label never dictates how narrow its container may become.
// Palette docks declare a minimum width and expect their contents to reflow
// below it; a plain QCheckBox reports the full label width as its minimum and
// would pin the whole dock area open instead, by an amount that varies with
// the platform's font metrics and with the active translation.
class ElidingCheckBox final : public QCheckBox
{
    Q_OBJECT

public:
    explicit ElidingCheckBox(const QString &text, QWidget *parent = nullptr);

    QSize minimumSizeHint() const override;

protected:
    void paintEvent(QPaintEvent *event) override;
};

}

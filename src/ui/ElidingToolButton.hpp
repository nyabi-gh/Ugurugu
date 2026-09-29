// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#pragma once

#include <QToolButton>

namespace ugurugu
{

// A text-only tool button whose label never dictates how narrow its container
// may become. Palette docks declare a minimum width and expect their contents
// to reflow below it; a plain QToolButton reports its full label as its
// minimum and would pin the whole dock area open instead, by an amount that
// varies with the platform's font metrics and the active translation.
class ElidingToolButton final : public QToolButton
{
    Q_OBJECT

public:
    explicit ElidingToolButton(const QString &text, QWidget *parent = nullptr);

    QSize minimumSizeHint() const override;

protected:
    void paintEvent(QPaintEvent *event) override;
};

}

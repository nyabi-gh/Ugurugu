// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#pragma once

#include <QToolButton>

namespace ugurugu
{

class WobblePlayButton final : public QToolButton
{
    Q_OBJECT

public:
    explicit WobblePlayButton(QWidget *parent = nullptr);

private:
    void refreshIcon();
};

}

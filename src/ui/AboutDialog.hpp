// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#pragma once

#include <QDialog>

namespace ugurugu
{

// Carries the notices GPL-3.0-or-later asks an interactive program to make
// reachable: the copyright line, the license the program is offered under,
// and the absence of any warranty.
class AboutDialog final : public QDialog
{
    Q_OBJECT

public:
    explicit AboutDialog(QWidget *parent = nullptr);
};

}

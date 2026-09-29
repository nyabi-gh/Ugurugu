// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#pragma once

#include <QDialog>

namespace ugurugu
{

class HelpDialog final : public QDialog
{
    Q_OBJECT

public:
    explicit HelpDialog(QWidget *parent = nullptr);
};

}

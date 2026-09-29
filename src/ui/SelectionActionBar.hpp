// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#pragma once

#include <QWidget>

class QAction;
class QHBoxLayout;
class QPaintEvent;
class QShowEvent;
class QToolButton;

namespace ugurugu
{

class SelectionActionBar final : public QWidget
{
    Q_OBJECT

public:
    explicit SelectionActionBar(QWidget *parent = nullptr);

    QToolButton *addActionButton(QAction *action);
    void addSeparator();

    QSize sizeHint() const override;
    QSize minimumSizeHint() const override;

protected:
    void paintEvent(QPaintEvent *event) override;
    void showEvent(QShowEvent *event) override;

private:
    QHBoxLayout *m_layout = nullptr;
    int m_buttonCount = 0;
    int m_separatorCount = 0;
};

}

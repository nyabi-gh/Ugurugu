// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#pragma once

#include <QElapsedTimer>
#include <QWidget>

namespace ugurugu
{

class ToolPopover final : public QWidget
{
    Q_OBJECT

public:
    explicit ToolPopover(QWidget *parent = nullptr);

    void setContentWidget(QWidget *content);
    void popupBeside(QWidget *anchor);

signals:
    void popoverShown();
    void popoverHidden();

protected:
    void paintEvent(QPaintEvent *event) override;
    void showEvent(QShowEvent *event) override;
    void hideEvent(QHideEvent *event) override;

private:
    QElapsedTimer m_lastHide;
};

}

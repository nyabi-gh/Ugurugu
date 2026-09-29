// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#pragma once

#include <QDockWidget>

class QComboBox;
class QPushButton;

namespace ugurugu
{

class DocumentController;
class WobblePopoverPanel;

class WobbleDock final : public QDockWidget
{
    Q_OBJECT

public:
    explicit WobbleDock(
        DocumentController *controller, QWidget *parent = nullptr);

private:
    void applyScope();
    void syncLayerState();

    DocumentController *m_controller = nullptr;
    QComboBox *m_scope = nullptr;
    QPushButton *m_followDocumentButton = nullptr;
    WobblePopoverPanel *m_panel = nullptr;
};

}

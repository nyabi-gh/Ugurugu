// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#pragma once

#include <QTimer>
#include <QVector>
#include <QWidget>

class QButtonGroup;
class QStackedWidget;

namespace ugurugu
{

class BrushPresetButton;
class CanvasWidget;

class BrushPopoverPanel final : public QWidget
{
    Q_OBJECT

public:
    explicit BrushPopoverPanel(CanvasWidget *canvas, QWidget *parent = nullptr);

    void setAnimationActive(bool active);

private:
    void syncToPreset(const QString &presetId);
    void advancePreviews();

    CanvasWidget *m_canvas;
    QButtonGroup *m_tabGroup = nullptr;
    QStackedWidget *m_stack = nullptr;
    QVector<BrushPresetButton *> m_presetButtons;
    QTimer m_previewTimer;
    int m_previewFrame = 0;
};

}

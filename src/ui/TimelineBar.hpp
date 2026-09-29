// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#pragma once

#include <QWidget>

class QEvent;
class QLabel;
class QSpinBox;
class QToolButton;

namespace ugurugu
{

class CanvasWidget;
class DocumentController;
class FrameScrubber;
class WobblePlayButton;

class TimelineBar final : public QWidget
{
    Q_OBJECT

public:
    TimelineBar(DocumentController *controller,
        CanvasWidget *canvas,
        QWidget *parent = nullptr);
    ~TimelineBar() override;

signals:
    void collapseRequested();

protected:
    bool eventFilter(QObject *watched, QEvent *event) override;

private:
    void buildLayout();
    void connectControls();
    void syncFromDocument();

    DocumentController *m_controller;
    CanvasWidget *m_canvas;
    WobblePlayButton *m_playButton = nullptr;
    QSpinBox *m_currentFrameSpin = nullptr;
    FrameScrubber *m_scrubber = nullptr;
    QSpinBox *m_framesSpin = nullptr;
    QSpinBox *m_fpsSpin = nullptr;
    QToolButton *m_collapseButton = nullptr;
    bool m_syncing = false;
};

}

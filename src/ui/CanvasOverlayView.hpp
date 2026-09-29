// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#pragma once

#include <QWidget>

namespace ugurugu
{

class CanvasWidget;

// Transparent sibling stacked over CanvasFrameView that keeps the selection
// outlines, cursor ring, border, shadow and hint text on the QPainter path
// while the frame pixels underneath are composited by the GPU.
class CanvasOverlayView final : public QWidget
{
    Q_OBJECT

public:
    explicit CanvasOverlayView(CanvasWidget *canvas);

protected:
    void paintEvent(QPaintEvent *event) override;

private:
    CanvasWidget *m_canvas;
};

}

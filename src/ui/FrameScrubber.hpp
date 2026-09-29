// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#pragma once

#include <QWidget>

namespace ugurugu
{

class CanvasWidget;
class DocumentController;

class FrameScrubber final : public QWidget
{
    Q_OBJECT

public:
    FrameScrubber(DocumentController *controller,
        CanvasWidget *canvas,
        QWidget *parent = nullptr);

    QSize sizeHint() const override;
    QSize minimumSizeHint() const override;

protected:
    void paintEvent(QPaintEvent *event) override;
    void mousePressEvent(QMouseEvent *event) override;
    void mouseMoveEvent(QMouseEvent *event) override;
    void mouseReleaseEvent(QMouseEvent *event) override;
    void leaveEvent(QEvent *event) override;
    void keyPressEvent(QKeyEvent *event) override;

private:
    int frameCount() const;
    QRectF trackRect() const;
    int frameAt(const QPointF &position) const;
    void scrubTo(const QPointF &position);
    void updateAccessibleValue();

    DocumentController *m_controller;
    CanvasWidget *m_canvas;
    int m_hoverFrame = -1;
    bool m_dragging = false;
};

}

// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "ui/CanvasOverlayView.hpp"

#include "ui/CanvasWidget.hpp"

#include <QPaintEvent>
#include <QPainter>

namespace ugurugu
{

CanvasOverlayView::CanvasOverlayView(CanvasWidget *canvas)
    : QWidget(canvas)
    , m_canvas(canvas)
{
    setAttribute(Qt::WA_TransparentForMouseEvents);
    setAttribute(Qt::WA_NoSystemBackground);
    setFocusPolicy(Qt::NoFocus);
}

void CanvasOverlayView::paintEvent(QPaintEvent *event)
{
    QPainter painter(this);
    m_canvas->paintOverlay(painter, event->region());
}

}

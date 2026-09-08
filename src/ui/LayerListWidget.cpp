// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "ui/LayerListWidget.hpp"

#include <QDropEvent>
#include <QKeyEvent>

namespace ugurugu
{

LayerListWidget::LayerListWidget(QWidget *parent)
    : QListWidget(parent)
{
    setSelectionMode(QAbstractItemView::SingleSelection);
    setDragDropMode(QAbstractItemView::InternalMove);
    setDefaultDropAction(Qt::MoveAction);
    setDropIndicatorShown(true);
}

void LayerListWidget::dropEvent(QDropEvent *event)
{
    if (event->source() != this)
    {
        event->ignore();
        return;
    }

    const QModelIndex target = indexAt(event->position().toPoint());
    event->setDropAction(Qt::IgnoreAction);
    event->accept();
    DropPlacement placement = DropPlacement::OnViewport;
    if (target.isValid())
    {
        switch (dropIndicatorPosition())
        {
        case QAbstractItemView::AboveItem:
            placement = DropPlacement::AboveTarget;
            break;
        case QAbstractItemView::BelowItem:
            placement = DropPlacement::BelowTarget;
            break;
        case QAbstractItemView::OnItem:
            placement = DropPlacement::OnTarget;
            break;
        case QAbstractItemView::OnViewport:
            placement = DropPlacement::OnViewport;
            break;
        }
    }
    emit dropRequested(
        currentRow(), target.isValid() ? target.row() : -1, placement);
}

void LayerListWidget::keyPressEvent(QKeyEvent *event)
{
    if (event->key() == Qt::Key_Space && event->modifiers() == Qt::NoModifier
        && !event->isAutoRepeat() && currentIndex().isValid())
    {
        emit visibilityToggleRequested(currentIndex());
        event->accept();
        return;
    }
    QListWidget::keyPressEvent(event);
}

}

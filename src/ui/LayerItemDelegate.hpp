// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#pragma once

#include <QFont>
#include <QStyledItemDelegate>

namespace ugurugu
{

namespace LayerItemRoles
{
constexpr int LayerId = Qt::UserRole;
constexpr int Visible = Qt::UserRole + 1;
constexpr int Thumbnail = Qt::UserRole + 2;
constexpr int Kind = Qt::UserRole + 3;
constexpr int Depth = Qt::UserRole + 4;
constexpr int Clipped = Qt::UserRole + 5;
constexpr int Reference = Qt::UserRole + 6;
constexpr int OpacityPercent = Qt::UserRole + 7;
constexpr int BlendModeName = Qt::UserRole + 8;
// Paint layers only: a group cannot carry a wobble override.
constexpr int WobbleToggleable = Qt::UserRole + 9;
constexpr int WobbleStopped = Qt::UserRole + 10;
}

class LayerItemDelegate final : public QStyledItemDelegate
{
    Q_OBJECT

public:
    explicit LayerItemDelegate(QObject *parent = nullptr);

    void paint(QPainter *painter,
        const QStyleOptionViewItem &option,
        const QModelIndex &index) const override;
    QSize sizeHint(const QStyleOptionViewItem &option,
        const QModelIndex &index) const override;
    bool editorEvent(QEvent *event,
        QAbstractItemModel *model,
        const QStyleOptionViewItem &option,
        const QModelIndex &index) override;
    QWidget *createEditor(QWidget *parent,
        const QStyleOptionViewItem &option,
        const QModelIndex &index) const override;
    void updateEditorGeometry(QWidget *editor,
        const QStyleOptionViewItem &option,
        const QModelIndex &index) const override;

signals:
    void visibilityToggled(const QModelIndex &index);
    void wobbleToggled(const QModelIndex &index);

private:
    QRect thumbnailRect(const QRect &rowRect, int depth) const;
    QRect eyeRect(const QRect &rowRect) const;
    QRect wobbleRect(const QRect &rowRect) const;
    QRect textColumnRect(const QRect &rowRect, int depth) const;
    QRect metaRect(const QRect &rowRect, int depth, const QFont &font) const;
    QRect nameRect(const QRect &rowRect, int depth, const QFont &font) const;
};

}

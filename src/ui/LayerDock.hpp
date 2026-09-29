// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#pragma once

#include "ui/LayerListWidget.hpp"

#include <QDockWidget>
#include <QHash>
#include <QPixmap>
#include <QPointer>
#include <QSet>
#include <QTimer>
#include <QUuid>

class QLabel;
class QListWidgetItem;
class QCheckBox;
class QComboBox;
class QSlider;
class QToolButton;

namespace ugurugu
{

class DocumentController;

class LayerDock final : public QDockWidget
{
    Q_OBJECT

public:
    explicit LayerDock(
        DocumentController *controller, QWidget *parent = nullptr);

signals:
    void groupSelectionChanged(bool groupSelected);

protected:
    bool event(QEvent *event) override;

private:
    void buildContent();
    void connectControls();
    void rebuild();
    void syncActiveLayer(const QUuid &id);
    void updateControls();
    void scheduleAllThumbnails();
    void scheduleLayerThumbnail(const QUuid &id);
    void queueThumbnail(const QUuid &id);
    void regenerateThumbnails();
    void commitOpacity(const QUuid &id, int value);
    void handleLayerDrop(
        int sourceRow, int targetRow, LayerListWidget::DropPlacement placement);
    QUuid selectedLayerId() const;

    QPointer<DocumentController> m_controller;
    LayerListWidget *m_layerList = nullptr;
    QToolButton *m_addButton = nullptr;
    QToolButton *m_addGroupButton = nullptr;
    QToolButton *m_duplicateButton = nullptr;
    QToolButton *m_mergeDownButton = nullptr;
    QToolButton *m_deleteButton = nullptr;
    QToolButton *m_moveUpButton = nullptr;
    QToolButton *m_moveDownButton = nullptr;
    QComboBox *m_blendModeCombo = nullptr;
    QComboBox *m_parentGroupCombo = nullptr;
    QCheckBox *m_clipCheck = nullptr;
    QCheckBox *m_referenceCheck = nullptr;
    QSlider *m_opacitySlider = nullptr;
    QLabel *m_opacityValue = nullptr;
    QTimer m_thumbnailTimer;
    QHash<QUuid, QPixmap> m_thumbnails;
    QSet<QUuid> m_pendingThumbnails;
    QHash<QUuid, quint64> m_thumbnailRevisions;
    quint64 m_nextThumbnailRevision = 0;
    bool m_thumbnailRendering = false;
    bool m_regenerateAllThumbnails = false;
    bool m_groupSelectionActive = false;
    bool m_syncing = false;
    bool m_opacityDragging = false;
    QUuid m_opacityLayerId;
    QUuid m_selectedLayerId;
};

}

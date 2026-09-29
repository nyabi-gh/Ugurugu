// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#pragma once

#include <QTimer>
#include <QUuid>
#include <QWidget>

namespace ugurugu
{

class DocumentController;

class WobblePreview final : public QWidget
{
    Q_OBJECT

public:
    explicit WobblePreview(
        DocumentController *controller, QWidget *parent = nullptr);

    QSize sizeHint() const override;
    bool isAnimationActive() const;
    void setScopeLayer(const QUuid &layerId);

protected:
    void paintEvent(QPaintEvent *event) override;
    void showEvent(QShowEvent *event) override;
    void hideEvent(QHideEvent *event) override;
    void changeEvent(QEvent *event) override;

private:
    qreal currentAmount() const;
    void syncAnimationState();

    DocumentController *m_controller;
    QUuid m_scopeLayer;
    QTimer m_timer;
    QTimer m_settleTimer;
    qreal m_phase = 0.0;
    qreal m_lastAmount = 0.0;
    bool m_shown = false;
};

}

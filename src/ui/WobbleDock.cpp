// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "ui/WobbleDock.hpp"

#include "document/DocumentController.hpp"
#include "ui/PaletteDockTitleBar.hpp"
#include "ui/WobblePopoverPanel.hpp"

#include <QComboBox>
#include <QPushButton>
#include <QScrollArea>
#include <QVBoxLayout>

namespace ugurugu
{

WobbleDock::WobbleDock(DocumentController *controller, QWidget *parent)
    : QDockWidget(tr("Wobble"), parent)
    , m_controller(controller)
{
    setObjectName(QStringLiteral("WobbleDock"));
    setFeatures(QDockWidget::DockWidgetMovable
                | QDockWidget::DockWidgetFloatable
                | QDockWidget::DockWidgetClosable);
    setMinimumWidth(150);
    installCompactPaletteTitleBar(this);

    auto *body = new QWidget(this);
    auto *layout = new QVBoxLayout(body);
    layout->setContentsMargins(8, 8, 8, 8);
    layout->setSpacing(8);

    m_scope = new QComboBox(body);
    m_scope->setObjectName(QStringLiteral("wobbleScopeCombo"));
    m_scope->addItem(tr("Whole drawing"));
    m_scope->addItem(tr("Active layer"));
    layout->addWidget(m_scope);

    m_followDocumentButton =
        new QPushButton(tr("Follow drawing settings"), body);
    m_followDocumentButton->setObjectName(
        QStringLiteral("followDocumentWobbleButton"));
    m_followDocumentButton->setToolTip(
        tr("Remove this layer's override and use the whole drawing settings."));
    layout->addWidget(m_followDocumentButton);

    m_panel = new WobblePopoverPanel(m_controller, body);
    layout->addWidget(m_panel);
    layout->addStretch(1);

    auto *scroll = new QScrollArea(this);
    scroll->setObjectName(QStringLiteral("wobbleSettingsScrollArea"));
    scroll->setWidgetResizable(true);
    scroll->setFrameShape(QFrame::NoFrame);
    scroll->setHorizontalScrollBarPolicy(Qt::ScrollBarAlwaysOff);
    scroll->setWidget(body);
    setWidget(scroll);

    connect(m_scope,
        &QComboBox::currentIndexChanged,
        this,
        &WobbleDock::applyScope);
    connect(m_followDocumentButton,
        &QPushButton::clicked,
        this,
        [this]()
        {
            m_controller->setLayerWobbleOverride(
                m_controller->document().activeLayerId, {}, {});
        });
    connect(m_controller,
        &DocumentController::activeLayerChanged,
        this,
        [this](const QUuid &)
        {
            applyScope();
        });
    connect(m_controller,
        &DocumentController::documentChanged,
        this,
        &WobbleDock::syncLayerState);
    connect(m_controller,
        &DocumentController::documentReplaced,
        this,
        [this]()
        {
            applyScope();
        });

    applyScope();
}

void WobbleDock::applyScope()
{
    const QUuid layerId = m_scope->currentIndex() == 1
                              ? m_controller->document().activeLayerId
                              : QUuid();
    m_panel->setScopeLayer(layerId);
    syncLayerState();
}

void WobbleDock::syncLayerState()
{
    const bool layerScope = m_scope->currentIndex() == 1;
    const Layer *layer = layerScope
                             ? m_controller->document().layer(
                                   m_controller->document().activeLayerId)
                             : nullptr;
    const bool hasOverride = layer && layer->kind == LayerKind::Paint
                             && (layer->wobbleAmount || layer->motion);
    m_followDocumentButton->setVisible(layerScope);
    m_followDocumentButton->setEnabled(hasOverride);
}

}

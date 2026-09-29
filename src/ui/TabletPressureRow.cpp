// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "ui/TabletPressureRow.hpp"

#include "ui/CanvasWidget.hpp"

#include <QCheckBox>
#include <QHBoxLayout>
#include <QLabel>
#include <QSignalBlocker>

namespace ugurugu
{

TabletPressureRow::TabletPressureRow(
    CanvasWidget *canvas, const QString &objectNamePrefix, QWidget *parent)
    : QWidget(parent)
{
    auto *layout = new QHBoxLayout(this);
    layout->setContentsMargins(0, 0, 0, 0);
    layout->setSpacing(8);

    auto *label = new QLabel(tr("PEN PRESSURE"), this);
    label->setObjectName(objectNamePrefix + QStringLiteral("Label"));
    label->setProperty("fieldLabel", true);
    layout->addWidget(label);
    layout->addStretch(1);

    auto *toggle = new QCheckBox(this);
    toggle->setObjectName(objectNamePrefix + QStringLiteral("Toggle"));
    toggle->setToolTip(tr("Vary stroke size and opacity with tablet pressure"));
    toggle->setAccessibleName(tr("Use tablet pressure"));
    toggle->setChecked(canvas->tabletPressureEnabled());
    label->setBuddy(toggle);
    layout->addWidget(toggle);

    connect(toggle,
        &QCheckBox::toggled,
        canvas,
        &CanvasWidget::setTabletPressureEnabled);
    connect(canvas,
        &CanvasWidget::tabletPressureEnabledChanged,
        this,
        [toggle](bool enabled)
        {
            const QSignalBlocker blocker(toggle);
            toggle->setChecked(enabled);
        });
}

}

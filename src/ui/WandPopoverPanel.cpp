// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "ui/WandPopoverPanel.hpp"

#include "ui/CanvasWidget.hpp"
#include "ui/WandReferenceButton.hpp"

#include <QButtonGroup>
#include <QLabel>
#include <QVBoxLayout>

#include <array>

namespace ugurugu
{

WandPopoverPanel::WandPopoverPanel(CanvasWidget *canvas, QWidget *parent)
    : QWidget(parent)
{
    auto *layout = new QVBoxLayout(this);
    layout->setContentsMargins(0, 0, 0, 0);
    layout->setSpacing(8);

    auto *label = new QLabel(tr("REFERENCE"), this);
    label->setProperty("fieldLabel", true);
    layout->addWidget(label);

    struct Option final
    {
        CanvasWidget::WandReference reference;
        QString title;
        QString description;
        QString objectName;
    };

    const std::array<Option, 3> options{{
        {CanvasWidget::WandReference::ActiveLayer,
            tr("Active layer"),
            tr("Use only the layer you are editing"),
            QStringLiteral("wandReferenceActiveButton")},
        {CanvasWidget::WandReference::ReferenceLayers,
            tr("Reference layers"),
            tr("Use layers marked as references"),
            QStringLiteral("wandReferenceMarkedButton")},
        {CanvasWidget::WandReference::AllVisibleLayers,
            tr("All visible layers"),
            tr("Combine every visible layer"),
            QStringLiteral("wandReferenceVisibleButton")},
    }};

    auto *group = new QButtonGroup(this);
    group->setExclusive(true);
    for (const Option &option : options)
    {
        auto *button = new WandReferenceButton(
            option.reference, option.title, option.description, this);
        button->setObjectName(option.objectName);
        button->setChecked(option.reference == canvas->wandReference());
        group->addButton(button, static_cast<int>(option.reference));
        layout->addWidget(button);
    }

    connect(group,
        &QButtonGroup::idClicked,
        this,
        [canvas](int id)
        {
            canvas->setWandReference(
                static_cast<CanvasWidget::WandReference>(id));
        });
    connect(canvas,
        &CanvasWidget::wandReferenceChanged,
        this,
        [group](CanvasWidget::WandReference reference)
        {
            if (QAbstractButton *button =
                    group->button(static_cast<int>(reference)))
            {
                button->setChecked(true);
            }
        });
}

}

// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "ui/BucketPopoverPanel.hpp"

#include "ui/CanvasWidget.hpp"
#include "ui/WandReferenceButton.hpp"

#include <QButtonGroup>
#include <QCheckBox>
#include <QFormLayout>
#include <QHBoxLayout>
#include <QLabel>
#include <QRadioButton>
#include <QSlider>
#include <QSpinBox>
#include <QVBoxLayout>

#include <array>

namespace ugurugu
{

BucketPopoverPanel::BucketPopoverPanel(CanvasWidget *canvas, QWidget *parent)
    : QWidget(parent)
{
    auto *layout = new QVBoxLayout(this);
    layout->setContentsMargins(0, 0, 0, 0);
    layout->setSpacing(8);

    auto *referenceLabel = new QLabel(tr("REFERENCE"), this);
    referenceLabel->setProperty("fieldLabel", true);
    layout->addWidget(referenceLabel);

    struct ReferenceOption final
    {
        CanvasWidget::WandReference reference;
        QString title;
        QString description;
        QString objectName;
    };
    const std::array<ReferenceOption, 3> references{{
        {CanvasWidget::WandReference::ActiveLayer,
            tr("Active layer"),
            tr("Use only the layer you are editing"),
            QStringLiteral("bucketReferenceActiveButton")},
        {CanvasWidget::WandReference::ReferenceLayers,
            tr("Reference layers"),
            tr("Use layers marked as references"),
            QStringLiteral("bucketReferenceMarkedButton")},
        {CanvasWidget::WandReference::AllVisibleLayers,
            tr("All visible layers"),
            tr("Combine every visible layer"),
            QStringLiteral("bucketReferenceVisibleButton")},
    }};
    auto *referenceGroup = new QButtonGroup(this);
    referenceGroup->setExclusive(true);
    for (const ReferenceOption &option : references)
    {
        auto *button = new WandReferenceButton(
            option.reference, option.title, option.description, this);
        button->setObjectName(option.objectName);
        button->setChecked(option.reference == canvas->wandReference());
        referenceGroup->addButton(button, static_cast<int>(option.reference));
        layout->addWidget(button);
    }
    connect(referenceGroup,
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
        [referenceGroup](CanvasWidget::WandReference reference)
        {
            if (QAbstractButton *button =
                    referenceGroup->button(static_cast<int>(reference)))
            {
                button->setChecked(true);
            }
        });

    auto *comparisonLabel = new QLabel(tr("COMPARISON"), this);
    comparisonLabel->setProperty("fieldLabel", true);
    layout->addWidget(comparisonLabel);
    auto *comparisonGroup = new QButtonGroup(this);
    comparisonGroup->setExclusive(true);
    auto *alphaButton = new QRadioButton(tr("Alpha boundary"), this);
    alphaButton->setObjectName(QStringLiteral("bucketAlphaBoundaryButton"));
    auto *colorButton = new QRadioButton(tr("Color tolerance"), this);
    colorButton->setObjectName(QStringLiteral("bucketColorToleranceButton"));
    comparisonGroup->addButton(alphaButton,
        static_cast<int>(CanvasWidget::FillComparison::AlphaBoundary));
    comparisonGroup->addButton(
        colorButton, static_cast<int>(CanvasWidget::FillComparison::Color));
    comparisonGroup->button(static_cast<int>(canvas->fillComparison()))
        ->setChecked(true);
    layout->addWidget(alphaButton);
    layout->addWidget(colorButton);

    auto *toleranceRow = new QFormLayout;
    toleranceRow->setRowWrapPolicy(QFormLayout::WrapLongRows);
    auto *toleranceLabel = new QLabel(tr("Tolerance"), this);
    auto *toleranceControls = new QWidget(this);
    auto *toleranceControlsLayout = new QHBoxLayout(toleranceControls);
    toleranceControlsLayout->setContentsMargins(0, 0, 0, 0);
    auto *toleranceSlider = new QSlider(Qt::Horizontal, toleranceControls);
    toleranceSlider->setObjectName(QStringLiteral("bucketToleranceSlider"));
    toleranceSlider->setRange(0, 255);
    toleranceSlider->setValue(canvas->fillTolerance());
    auto *toleranceSpin = new QSpinBox(toleranceControls);
    toleranceSpin->setObjectName(QStringLiteral("bucketToleranceSpin"));
    toleranceSpin->setRange(0, 255);
    toleranceSpin->setValue(canvas->fillTolerance());
    toleranceControlsLayout->addWidget(toleranceSlider, 1);
    toleranceControlsLayout->addWidget(toleranceSpin);
    toleranceRow->addRow(toleranceLabel, toleranceControls);
    layout->addLayout(toleranceRow);

    const auto syncToleranceEnabled =
        [canvas, toleranceLabel, toleranceSlider, toleranceSpin]()
    {
        const bool enabled =
            canvas->fillComparison() == CanvasWidget::FillComparison::Color;
        toleranceLabel->setEnabled(enabled);
        toleranceSlider->setEnabled(enabled);
        toleranceSpin->setEnabled(enabled);
    };
    connect(comparisonGroup,
        &QButtonGroup::idClicked,
        this,
        [canvas](int id)
        {
            canvas->setFillComparison(
                static_cast<CanvasWidget::FillComparison>(id));
        });
    connect(canvas,
        &CanvasWidget::fillComparisonChanged,
        this,
        [comparisonGroup, syncToleranceEnabled](
            CanvasWidget::FillComparison comparison)
        {
            if (QAbstractButton *button =
                    comparisonGroup->button(static_cast<int>(comparison)))
            {
                button->setChecked(true);
            }
            syncToleranceEnabled();
        });
    connect(toleranceSlider,
        &QSlider::valueChanged,
        toleranceSpin,
        &QSpinBox::setValue);
    connect(toleranceSpin,
        &QSpinBox::valueChanged,
        toleranceSlider,
        &QSlider::setValue);
    connect(toleranceSpin,
        &QSpinBox::valueChanged,
        canvas,
        &CanvasWidget::setFillTolerance);
    connect(canvas,
        &CanvasWidget::fillToleranceChanged,
        toleranceSpin,
        &QSpinBox::setValue);
    syncToleranceEnabled();

    auto *antialiasing = new QCheckBox(tr("Antialias fill edge"), this);
    antialiasing->setObjectName(QStringLiteral("bucketAntialiasingCheckBox"));
    antialiasing->setChecked(canvas->bucketAntialiasing());
    layout->addWidget(antialiasing);
    connect(antialiasing,
        &QCheckBox::toggled,
        canvas,
        &CanvasWidget::setBucketAntialiasing);
    connect(canvas,
        &CanvasWidget::bucketAntialiasingChanged,
        antialiasing,
        &QCheckBox::setChecked);
}

}

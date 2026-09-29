// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "ui/LassoPopoverPanel.hpp"

#include "ui/CanvasWidget.hpp"
#include "ui/SelectionShapeButton.hpp"

#include <QButtonGroup>
#include <QComboBox>
#include <QLabel>
#include <QRadioButton>
#include <QSignalBlocker>
#include <QVBoxLayout>

#include <array>

namespace ugurugu
{

LassoPopoverPanel::LassoPopoverPanel(CanvasWidget *canvas, QWidget *parent)
    : QWidget(parent)
{
    auto *layout = new QVBoxLayout(this);
    layout->setContentsMargins(0, 0, 0, 0);
    layout->setSpacing(8);

    auto *modeLabel = new QLabel(tr("MODE"), this);
    modeLabel->setProperty("fieldLabel", true);
    layout->addWidget(modeLabel);

    auto *modeGroup = new QButtonGroup(this);
    modeGroup->setExclusive(true);
    auto *selectMode = new QRadioButton(tr("Select"), this);
    selectMode->setObjectName(QStringLiteral("lassoSelectModeButton"));
    auto *paintMode = new QRadioButton(tr("Paint"), this);
    paintMode->setObjectName(QStringLiteral("lassoPaintModeButton"));
    modeGroup->addButton(
        selectMode, static_cast<int>(CanvasWidget::LassoMode::Select));
    modeGroup->addButton(
        paintMode, static_cast<int>(CanvasWidget::LassoMode::Paint));
    modeGroup->button(static_cast<int>(canvas->lassoMode()))->setChecked(true);
    layout->addWidget(selectMode);
    layout->addWidget(paintMode);

    connect(modeGroup,
        &QButtonGroup::idClicked,
        this,
        [canvas](int id)
        {
            canvas->setLassoMode(static_cast<CanvasWidget::LassoMode>(id));
        });
    connect(canvas,
        &CanvasWidget::lassoModeChanged,
        this,
        [modeGroup](CanvasWidget::LassoMode mode)
        {
            if (QAbstractButton *button =
                    modeGroup->button(static_cast<int>(mode)))
            {
                button->setChecked(true);
            }
        });

    auto *label = new QLabel(tr("SHAPE"), this);
    label->setProperty("fieldLabel", true);
    layout->addWidget(label);

    struct Option final
    {
        CanvasWidget::SelectionShape shape;
        QString title;
        QString description;
        QString objectName;
    };

    const std::array<Option, 3> options{{
        {CanvasWidget::SelectionShape::Freehand,
            tr("Freehand"),
            tr("Draw freely around an area"),
            QStringLiteral("selectionShapeFreehandButton")},
        {CanvasWidget::SelectionShape::Rectangle,
            tr("Rectangle"),
            tr("Drag between opposite corners"),
            QStringLiteral("selectionShapeRectangleButton")},
        {CanvasWidget::SelectionShape::Ellipse,
            tr("Ellipse"),
            tr("Drag to fit an oval area"),
            QStringLiteral("selectionShapeEllipseButton")},
    }};

    auto *group = new QButtonGroup(this);
    group->setExclusive(true);
    for (const Option &option : options)
    {
        auto *button = new SelectionShapeButton(
            option.shape, option.title, option.description, this);
        button->setObjectName(option.objectName);
        button->setChecked(option.shape == canvas->selectionShape());
        group->addButton(button, static_cast<int>(option.shape));
        layout->addWidget(button);
    }

    connect(group,
        &QButtonGroup::idClicked,
        this,
        [canvas](int id)
        {
            canvas->setSelectionShape(
                static_cast<CanvasWidget::SelectionShape>(id));
        });
    connect(canvas,
        &CanvasWidget::selectionShapeChanged,
        this,
        [group](CanvasWidget::SelectionShape shape)
        {
            if (QAbstractButton *button =
                    group->button(static_cast<int>(shape)))
            {
                button->setChecked(true);
            }
        });

    auto *transformLabel = new QLabel(tr("TRANSFORM"), this);
    transformLabel->setProperty("fieldLabel", true);
    layout->addWidget(transformLabel);

    auto *sampling = new QComboBox(this);
    sampling->setObjectName(QStringLiteral("selectionTransformSamplingCombo"));
    sampling->setAccessibleName(tr("Transform method"));
    sampling->setToolTip(
        tr("Smooth interpolates neighboring pixels during selection "
           "transforms. Pixel-based skips interpolation without removing "
           "existing transparency, but can create stepped edges."));
    sampling->addItem(tr("Smooth"), static_cast<int>(SamplingMode::Smooth));
    sampling->addItem(
        tr("Pixel-based"), static_cast<int>(SamplingMode::Nearest));
    sampling->setCurrentIndex(sampling->findData(
        static_cast<int>(canvas->selectionTransformSampling())));
    sampling->setMinimumWidth(48);
    sampling->setSizePolicy(QSizePolicy::Ignored, QSizePolicy::Fixed);
    transformLabel->setBuddy(sampling);
    layout->addWidget(sampling);

    connect(sampling,
        &QComboBox::currentIndexChanged,
        canvas,
        [canvas, sampling](int)
        {
            canvas->setSelectionTransformSampling(
                static_cast<SamplingMode>(sampling->currentData().toInt()));
        });
    connect(canvas,
        &CanvasWidget::selectionTransformSamplingChanged,
        this,
        [sampling](SamplingMode mode)
        {
            const int index = sampling->findData(static_cast<int>(mode));
            if (index >= 0)
            {
                const QSignalBlocker blocker(sampling);
                sampling->setCurrentIndex(index);
            }
        });
}

}

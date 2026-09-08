// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "ui/BrushSizeRow.hpp"

#include "document/DocumentLimits.hpp"
#include "ui/CanvasWidget.hpp"
#include "ui/SpinBoxCaretGuard.hpp"

#include <QFormLayout>
#include <QHBoxLayout>
#include <QLabel>
#include <QSignalBlocker>
#include <QSlider>
#include <QSpinBox>

#include <algorithm>
#include <cmath>

namespace ugurugu
{

BrushSizeRow::BrushSizeRow(CanvasWidget *canvas,
    Target target,
    const QString &objectNamePrefix,
    QWidget *parent)
    : QWidget(parent)
{
    const bool controlsEraser = target == Target::Eraser;
    const QString accessibleName =
        controlsEraser ? tr("Eraser size") : tr("Brush size");

    auto *layout = new QFormLayout(this);
    layout->setContentsMargins(0, 0, 0, 0);
    layout->setSpacing(8);
    layout->setRowWrapPolicy(QFormLayout::WrapLongRows);

    auto *label = new QLabel(tr("SIZE"), this);
    label->setProperty("fieldLabel", true);

    const int minimum =
        static_cast<int>(std::ceil(DocumentLimits::minimumStrokeWidth));

    auto *controls = new QWidget(this);
    auto *controlsLayout = new QHBoxLayout(controls);
    controlsLayout->setContentsMargins(0, 0, 0, 0);
    controlsLayout->setSpacing(8);

    auto *slider = new QSlider(Qt::Horizontal, controls);
    slider->setObjectName(objectNamePrefix + QStringLiteral("Slider"));
    slider->setRange(minimum, 128);
    slider->setMinimumWidth(40);
    slider->setToolTip(accessibleName);
    slider->setAccessibleName(accessibleName);
    controlsLayout->addWidget(slider, 1);

    auto *spin = new QSpinBox(controls);
    spin->setObjectName(objectNamePrefix + QStringLiteral("Spin"));
    spin->setRange(minimum,
        static_cast<int>(std::floor(DocumentLimits::maximumStrokeWidth)));
    spin->setSuffix(tr(" px"));
    installSuffixCaretGuard(spin);
    spin->setAccessibleName(accessibleName);
    label->setBuddy(spin);
    controlsLayout->addWidget(spin);
    layout->addRow(label, controls);

    const int initial =
        qRound(controlsEraser ? canvas->eraserWidth() : canvas->brushWidth());
    spin->setValue(initial);
    slider->setValue(std::clamp(initial, slider->minimum(), slider->maximum()));

    connect(spin,
        &QSpinBox::valueChanged,
        this,
        [canvas, controlsEraser, slider](int value)
        {
            if (controlsEraser)
            {
                canvas->setEraserWidth(value);
            }
            else
            {
                canvas->setBrushWidth(value);
            }
            QSignalBlocker blocker(slider);
            slider->setValue(
                std::clamp(value, slider->minimum(), slider->maximum()));
        });
    connect(slider,
        &QSlider::valueChanged,
        spin,
        qOverload<int>(&QSpinBox::setValue));
    const auto syncControls = [spin, slider](qreal width)
    {
        const int value = qRound(width);
        QSignalBlocker spinBlocker(spin);
        QSignalBlocker sliderBlocker(slider);
        spin->setValue(value);
        slider->setValue(
            std::clamp(value, slider->minimum(), slider->maximum()));
    };
    if (controlsEraser)
    {
        connect(canvas, &CanvasWidget::eraserWidthChanged, this, syncControls);
    }
    else
    {
        connect(canvas, &CanvasWidget::brushWidthChanged, this, syncControls);
    }
}

}

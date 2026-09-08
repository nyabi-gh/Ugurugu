// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "ui/StrokeStabilizationRow.hpp"

#include "ui/CanvasWidget.hpp"
#include "ui/SpinBoxCaretGuard.hpp"

#include <QFormLayout>
#include <QHBoxLayout>
#include <QLabel>
#include <QSignalBlocker>
#include <QSlider>
#include <QSpinBox>

namespace ugurugu
{

StrokeStabilizationRow::StrokeStabilizationRow(CanvasWidget *canvas,
    Target target,
    const QString &objectNamePrefix,
    QWidget *parent)
    : QWidget(parent)
{
    const bool controlsEraser = target == Target::Eraser;
    const QString accessibleName = tr("Stroke stabilization");

    auto *layout = new QFormLayout(this);
    layout->setContentsMargins(0, 0, 0, 0);
    layout->setSpacing(8);
    layout->setRowWrapPolicy(QFormLayout::WrapLongRows);

    auto *label = new QLabel(tr("STABILIZATION"), this);
    label->setProperty("fieldLabel", true);

    auto *controls = new QWidget(this);
    auto *controlsLayout = new QHBoxLayout(controls);
    controlsLayout->setContentsMargins(0, 0, 0, 0);
    controlsLayout->setSpacing(8);

    auto *slider = new QSlider(Qt::Horizontal, controls);
    slider->setObjectName(objectNamePrefix + QStringLiteral("Slider"));
    slider->setRange(0, 100);
    slider->setMinimumWidth(40);
    slider->setToolTip(accessibleName);
    slider->setAccessibleName(accessibleName);
    controlsLayout->addWidget(slider, 1);

    auto *spin = new QSpinBox(controls);
    spin->setObjectName(objectNamePrefix + QStringLiteral("Spin"));
    spin->setRange(0, 100);
    spin->setSuffix(tr("%"));
    installSuffixCaretGuard(spin);
    spin->setAccessibleName(accessibleName);
    label->setBuddy(spin);
    controlsLayout->addWidget(spin);
    layout->addRow(label, controls);

    const int initial = qRound((controlsEraser ? canvas->eraserStabilization()
                                               : canvas->brushStabilization())
                               * 100.0);
    slider->setValue(initial);
    spin->setValue(initial);

    connect(spin,
        &QSpinBox::valueChanged,
        this,
        [canvas, controlsEraser, slider](int value)
        {
            if (controlsEraser)
            {
                canvas->setEraserStabilization(value / 100.0);
            }
            else
            {
                canvas->setBrushStabilization(value / 100.0);
            }
            QSignalBlocker blocker(slider);
            slider->setValue(value);
        });
    connect(slider,
        &QSlider::valueChanged,
        spin,
        qOverload<int>(&QSpinBox::setValue));
    const auto syncControls = [spin, slider](qreal strength)
    {
        const int value = qRound(strength * 100.0);
        QSignalBlocker spinBlocker(spin);
        QSignalBlocker sliderBlocker(slider);
        spin->setValue(value);
        slider->setValue(value);
    };
    if (controlsEraser)
    {
        connect(canvas,
            &CanvasWidget::eraserStabilizationChanged,
            this,
            syncControls);
    }
    else
    {
        connect(canvas,
            &CanvasWidget::brushStabilizationChanged,
            this,
            syncControls);
    }
}

}

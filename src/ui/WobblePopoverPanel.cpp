// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "ui/WobblePopoverPanel.hpp"

#include "document/DocumentController.hpp"
#include "document/DocumentLimits.hpp"
#include "ui/ResponsiveGrid.hpp"
#include "ui/SpinBoxCaretGuard.hpp"
#include "ui/WobblePreview.hpp"

#include <QCheckBox>
#include <QComboBox>
#include <QDoubleSpinBox>
#include <QFormLayout>
#include <QHBoxLayout>
#include <QLabel>
#include <QSignalBlocker>
#include <QSlider>
#include <QSpinBox>
#include <QVBoxLayout>

namespace ugurugu
{

namespace
{

struct PercentControls final
{
    QSlider *slider = nullptr;
    QSpinBox *spin = nullptr;
};

PercentControls addPercentControls(QWidget *parent,
    QFormLayout *form,
    const QString &label,
    const QString &objectName)
{
    auto *slider = new QSlider(Qt::Horizontal, parent);
    slider->setObjectName(objectName + QStringLiteral("Slider"));
    slider->setRange(0, 100);
    auto *spin = new QSpinBox(parent);
    spin->setObjectName(objectName + QStringLiteral("Spin"));
    spin->setRange(0, 100);
    spin->setSuffix(QStringLiteral("%"));
    installSuffixCaretGuard(spin);
    auto *row = new QWidget(parent);
    auto *rowLayout = new QHBoxLayout(row);
    rowLayout->setContentsMargins(0, 0, 0, 0);
    rowLayout->addWidget(slider, 1);
    rowLayout->addWidget(spin);
    form->addRow(label, row);
    QObject::connect(slider, &QSlider::valueChanged, spin, &QSpinBox::setValue);
    QObject::connect(spin, &QSpinBox::valueChanged, slider, &QSlider::setValue);
    return {slider, spin};
}

}

WobblePopoverPanel::WobblePopoverPanel(
    DocumentController *controller, QWidget *parent)
    : QWidget(parent)
    , m_controller(controller)
{
    auto *layout = new QVBoxLayout(this);
    layout->setContentsMargins(0, 0, 0, 0);
    layout->setSpacing(10);

    auto *heading = new QLabel(tr("WOBBLE"), this);
    heading->setProperty("fieldLabel", true);
    layout->addWidget(heading);

    auto *amountGrid = new ResponsiveGrid(84, 3, 6, this);
    amountGrid->setObjectName(QStringLiteral("wobbleAmountGrid"));
    m_preview = new WobblePreview(controller, amountGrid);
    m_preview->setObjectName(QStringLiteral("wobbleSettingsPreview"));
    amountGrid->addWidget(m_preview);
    auto *amountSlider = new QSlider(Qt::Horizontal, amountGrid);
    amountSlider->setObjectName(QStringLiteral("wobbleSlider"));
    amountSlider->setRange(qRound(DocumentLimits::minimumWobbleAmount * 10.0),
        qRound(DocumentLimits::maximumWobbleAmount * 10.0));
    amountGrid->addWidget(amountSlider);
    auto *amountSpin = new QDoubleSpinBox(amountGrid);
    amountSpin->setObjectName(QStringLiteral("wobbleSpin"));
    amountSpin->setRange(DocumentLimits::minimumWobbleAmount,
        DocumentLimits::maximumWobbleAmount);
    amountSpin->setDecimals(1);
    amountSpin->setSingleStep(0.1);
    amountSpin->setSuffix(tr(" px"));
    installSuffixCaretGuard(amountSpin);
    amountGrid->addWidget(amountSpin);
    layout->addWidget(amountGrid);

    auto *form = new QFormLayout;
    form->setContentsMargins(0, 0, 0, 0);
    form->setSpacing(8);
    form->setRowWrapPolicy(QFormLayout::WrapLongRows);
    auto *style = new QComboBox(this);
    style->setObjectName(QStringLiteral("motionStyleCombo"));
    style->addItem(tr("Classic"), static_cast<int>(MotionStyle::Classic));
    style->addItem(tr("Smooth"), static_cast<int>(MotionStyle::Smooth));
    style->addItem(tr("Stepped"), static_cast<int>(MotionStyle::Stepped));
    form->addRow(tr("Motion style"), style);

    auto *poseCount = new QSpinBox(this);
    poseCount->setObjectName(QStringLiteral("motionPoseCountSpin"));
    poseCount->setRange(DocumentLimits::minimumMotionPoseCount,
        DocumentLimits::maximumMotionPoseCount);
    form->addRow(tr("Pose count"), poseCount);

    auto *detail = new QSpinBox(this);
    detail->setObjectName(QStringLiteral("motionDetailSpin"));
    detail->setRange(DocumentLimits::minimumMotionDetail,
        DocumentLimits::maximumMotionDetail);
    form->addRow(tr("Detail"), detail);

    const PercentControls linked = addPercentControls(
        this, form, tr("Linked"), QStringLiteral("motionLinked"));
    const PercentControls randomness = addPercentControls(
        this, form, tr("Randomness"), QStringLiteral("motionRandomness"));

    auto *brokenLine = new QCheckBox(tr("Broken line"), this);
    brokenLine->setObjectName(QStringLiteral("brokenLineCheckBox"));
    form->addRow(QString(), brokenLine);
    const PercentControls breakAmount = addPercentControls(
        this, form, tr("Break amount"), QStringLiteral("breakAmount"));
    auto *breakRange = new QDoubleSpinBox(this);
    breakRange->setObjectName(QStringLiteral("breakRangeSpin"));
    breakRange->setRange(
        DocumentLimits::minimumBreakRange, DocumentLimits::maximumBreakRange);
    breakRange->setDecimals(1);
    breakRange->setSuffix(tr(" px"));
    installSuffixCaretGuard(breakRange);
    form->addRow(tr("Break range"), breakRange);
    layout->addLayout(form);

    connect(amountSlider,
        &QSlider::valueChanged,
        amountSpin,
        [amountSpin](int value)
        {
            amountSpin->setValue(value / 10.0);
        });
    connect(amountSpin,
        &QDoubleSpinBox::valueChanged,
        amountSlider,
        [amountSlider](double value)
        {
            amountSlider->setValue(qRound(value * 10.0));
        });
    connect(amountSpin,
        &QDoubleSpinBox::valueChanged,
        this,
        [this, controller](double value)
        {
            if (editScopedLayer(
                    [value](qreal &amount, MotionSettings &)
                    {
                        amount = value;
                    }))
            {
                return;
            }
            controller->setWobbleAmount(value);
        });
    connect(style,
        &QComboBox::currentIndexChanged,
        this,
        [this, controller, style](int)
        {
            const auto next =
                static_cast<MotionStyle>(style->currentData().toInt());
            if (editScopedLayer(
                    [next](qreal &, MotionSettings &motion)
                    {
                        motion.style = next;
                    }))
            {
                return;
            }
            controller->setMotionStyle(next);
        });
    connect(poseCount,
        &QSpinBox::valueChanged,
        this,
        [this, controller](int value)
        {
            if (editScopedLayer(
                    [value](qreal &, MotionSettings &motion)
                    {
                        motion.poseCount = value;
                    }))
            {
                return;
            }
            controller->setMotionPoseCount(value);
        });
    connect(detail,
        &QSpinBox::valueChanged,
        this,
        [this, controller](int value)
        {
            if (editScopedLayer(
                    [value](qreal &, MotionSettings &motion)
                    {
                        motion.detail = value;
                    }))
            {
                return;
            }
            controller->setMotionDetail(value);
        });
    connect(linked.spin,
        &QSpinBox::valueChanged,
        this,
        [this, controller](int value)
        {
            if (editScopedLayer(
                    [value](qreal &, MotionSettings &motion)
                    {
                        motion.linked = value / 100.0;
                    }))
            {
                return;
            }
            controller->setMotionLinked(value / 100.0);
        });
    connect(randomness.spin,
        &QSpinBox::valueChanged,
        this,
        [this, controller](int value)
        {
            if (editScopedLayer(
                    [value](qreal &, MotionSettings &motion)
                    {
                        motion.randomness = value / 100.0;
                    }))
            {
                return;
            }
            controller->setMotionRandomness(value / 100.0);
        });
    connect(brokenLine,
        &QCheckBox::toggled,
        this,
        [this, controller](bool enabled)
        {
            if (editScopedLayer(
                    [enabled](qreal &, MotionSettings &motion)
                    {
                        motion.brokenLine = enabled;
                    }))
            {
                return;
            }
            controller->setBrokenLineEnabled(enabled);
        });
    connect(breakAmount.spin,
        &QSpinBox::valueChanged,
        this,
        [this, controller](int value)
        {
            if (editScopedLayer(
                    [value](qreal &, MotionSettings &motion)
                    {
                        motion.breakAmount = value / 100.0;
                    }))
            {
                return;
            }
            controller->setBreakAmount(value / 100.0);
        });
    connect(breakRange,
        &QDoubleSpinBox::valueChanged,
        this,
        [this, controller](double value)
        {
            if (editScopedLayer(
                    [value](qreal &, MotionSettings &motion)
                    {
                        motion.breakRange = value;
                    }))
            {
                return;
            }
            controller->setBreakRange(value);
        });

    const auto sync = [this,
                          controller,
                          amountSlider,
                          amountSpin,
                          style,
                          poseCount,
                          detail,
                          linked,
                          randomness,
                          brokenLine,
                          breakAmount,
                          breakRange]()
    {
        const QSignalBlocker amountSliderBlocker(amountSlider);
        const QSignalBlocker amountSpinBlocker(amountSpin);
        const QSignalBlocker styleBlocker(style);
        const QSignalBlocker poseCountBlocker(poseCount);
        const QSignalBlocker detailBlocker(detail);
        const QSignalBlocker linkedSliderBlocker(linked.slider);
        const QSignalBlocker linkedSpinBlocker(linked.spin);
        const QSignalBlocker randomnessSliderBlocker(randomness.slider);
        const QSignalBlocker randomnessSpinBlocker(randomness.spin);
        const QSignalBlocker brokenLineBlocker(brokenLine);
        const QSignalBlocker breakAmountSliderBlocker(breakAmount.slider);
        const QSignalBlocker breakAmountSpinBlocker(breakAmount.spin);
        const QSignalBlocker breakRangeBlocker(breakRange);
        const Document &document = controller->document();
        // A layer that has not overridden anything still shows the document's
        // values, so switching scope never blanks the panel.
        const Layer *scoped =
            m_scopeLayer.isNull() ? nullptr : document.layer(m_scopeLayer);
        const qreal wobbleAmount =
            scoped ? effectiveWobbleAmount(document, *scoped)
                   : document.wobbleAmount;
        const MotionSettings motion =
            scoped ? effectiveMotion(document, *scoped) : document.motion;
        amountSlider->setValue(qRound(wobbleAmount * 10.0));
        amountSpin->setValue(wobbleAmount);
        style->setCurrentIndex(style->findData(static_cast<int>(motion.style)));
        poseCount->setMaximum(motion.style == MotionStyle::Classic
                                  ? DocumentLimits::maximumMotionPoseCount
                                  : document.animationFrames);
        poseCount->setValue(motion.poseCount);
        poseCount->setEnabled(motion.style != MotionStyle::Classic);
        detail->setValue(motion.detail);
        linked.spin->setValue(qRound(motion.linked * 100.0));
        randomness.spin->setValue(qRound(motion.randomness * 100.0));
        brokenLine->setChecked(motion.brokenLine);
        breakAmount.spin->setValue(qRound(motion.breakAmount * 100.0));
        breakRange->setValue(motion.breakRange);
        breakAmount.slider->setEnabled(motion.brokenLine);
        breakAmount.spin->setEnabled(motion.brokenLine);
        breakRange->setEnabled(motion.brokenLine);
    };
    m_sync = sync;
    connect(controller,
        &DocumentController::documentChanged,
        this,
        [sync]()
        {
            sync();
        });
    sync();
}

void WobblePopoverPanel::setScopeLayer(const QUuid &layerId)
{
    if (m_scopeLayer == layerId)
    {
        return;
    }
    m_scopeLayer = layerId;
    m_preview->setScopeLayer(layerId);
    if (m_sync)
    {
        m_sync();
    }
}

QUuid WobblePopoverPanel::scopeLayer() const
{
    return m_scopeLayer;
}

bool WobblePopoverPanel::editScopedLayer(
    const std::function<void(qreal &, MotionSettings &)> &mutate)
{
    if (m_scopeLayer.isNull() || !m_controller)
    {
        return false;
    }
    const Document &document = m_controller->document();
    const Layer *layer = document.layer(m_scopeLayer);
    if (!layer || layer->kind != LayerKind::Paint)
    {
        return false;
    }
    // Seeding from the effective values means the first edit on a layer that
    // was following the document keeps everything else exactly as it looked.
    qreal amount = effectiveWobbleAmount(document, *layer);
    MotionSettings motion = effectiveMotion(document, *layer);
    mutate(amount, motion);
    m_controller->setLayerWobbleOverride(m_scopeLayer, amount, motion);
    return true;
}

}

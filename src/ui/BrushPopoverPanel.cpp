// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "ui/BrushPopoverPanel.hpp"

#include "brush/BrushPreset.hpp"
#include "ui/BrushPresetButton.hpp"
#include "ui/BrushSizeRow.hpp"
#include "ui/CanvasWidget.hpp"
#include "ui/ResponsiveGrid.hpp"
#include "ui/StrokeStabilizationRow.hpp"
#include "ui/TabletPressureRow.hpp"

#include <QButtonGroup>
#include <QCheckBox>
#include <QLabel>
#include <QSignalBlocker>
#include <QStackedWidget>
#include <QToolButton>
#include <QVBoxLayout>

namespace ugurugu
{

namespace
{

const QVector<BrushCategory> &orderedCategories()
{
    static const QVector<BrushCategory> categories{BrushCategory::Pen,
        BrushCategory::Marker,
        BrushCategory::Airbrush,
        BrushCategory::Spray};
    return categories;
}

}

BrushPopoverPanel::BrushPopoverPanel(CanvasWidget *canvas, QWidget *parent)
    : QWidget(parent)
    , m_canvas(canvas)
{
    auto *layout = new QVBoxLayout(this);
    layout->setContentsMargins(0, 0, 0, 0);
    layout->setSpacing(8);

    auto *tabGrid = new ResponsiveGrid(72, 4, 4, this);
    tabGrid->setObjectName(QStringLiteral("brushCategoryGrid"));
    m_tabGroup = new QButtonGroup(this);
    m_tabGroup->setExclusive(true);
    m_stack = new QStackedWidget(this);

    auto *presetGroup = new QButtonGroup(this);
    presetGroup->setExclusive(true);

    for (int categoryIndex = 0; categoryIndex < orderedCategories().size();
        ++categoryIndex)
    {
        const BrushCategory category = orderedCategories().at(categoryIndex);

        auto *tab = new QToolButton(this);
        tab->setText(BrushPresetCatalog::categoryName(category));
        tab->setCheckable(true);
        tab->setCursor(Qt::PointingHandCursor);
        tab->setProperty("categoryTab", true);
        m_tabGroup->addButton(tab, categoryIndex);
        tabGrid->addWidget(tab);

        auto *presetGrid = new ResponsiveGrid(124, 2, 4, m_stack);
        presetGrid->setObjectName(QStringLiteral("brushPresetGrid"));
        for (const BrushPreset &preset : BrushPresetCatalog::builtIns())
        {
            if (preset.category != category)
            {
                continue;
            }
            auto *button = new BrushPresetButton(preset, presetGrid);
            button->setTabletPressureEnabled(canvas->tabletPressureEnabled());
            presetGroup->addButton(button);
            presetGrid->addWidget(button);
            connect(button,
                &QAbstractButton::clicked,
                this,
                [this, button]()
                {
                    m_canvas->setBrushPreset(button->presetId());
                });
            m_presetButtons.append(button);
        }
        m_stack->addWidget(presetGrid);
    }
    connect(m_tabGroup,
        &QButtonGroup::idClicked,
        m_stack,
        &QStackedWidget::setCurrentIndex);

    layout->addWidget(tabGrid);
    layout->addWidget(m_stack);
    layout->addWidget(new BrushSizeRow(canvas,
        BrushSizeRow::Target::Brush,
        QStringLiteral("brushSize"),
        this));
    layout->addWidget(new TabletPressureRow(
        canvas, QStringLiteral("brushTabletPressure"), this));
    layout->addWidget(new StrokeStabilizationRow(canvas,
        StrokeStabilizationRow::Target::Brush,
        QStringLiteral("brushStabilization"),
        this));

    auto *antialiasRow = new QWidget(this);
    auto *antialiasLayout = new QHBoxLayout(antialiasRow);
    antialiasLayout->setContentsMargins(0, 0, 0, 0);
    antialiasLayout->setSpacing(8);

    auto *antialiasLabel = new QLabel(tr("ANTI-ALIASING"), antialiasRow);
    antialiasLabel->setProperty("fieldLabel", true);
    antialiasLayout->addWidget(antialiasLabel);
    antialiasLayout->addStretch(1);

    auto *antialiasToggle = new QCheckBox(antialiasRow);
    antialiasToggle->setObjectName(QStringLiteral("brushAntialiasingToggle"));
    antialiasToggle->setToolTip(tr("Smooth stroke edges"));
    antialiasToggle->setAccessibleName(tr("Anti-aliasing"));
    antialiasToggle->setChecked(canvas->brushAntialiasing());
    antialiasLayout->addWidget(antialiasToggle);

    connect(antialiasToggle,
        &QCheckBox::toggled,
        this,
        [this](bool checked)
        {
            m_canvas->setBrushAntialiasing(checked);
        });
    connect(canvas,
        &CanvasWidget::brushAntialiasingChanged,
        this,
        [antialiasToggle](bool antialiasing)
        {
            QSignalBlocker blocker(antialiasToggle);
            antialiasToggle->setChecked(antialiasing);
        });

    layout->addWidget(antialiasRow);

    m_previewTimer.setInterval(120);
    connect(&m_previewTimer,
        &QTimer::timeout,
        this,
        &BrushPopoverPanel::advancePreviews);

    connect(canvas,
        &CanvasWidget::tabletPressureEnabledChanged,
        this,
        [this](bool enabled)
        {
            for (BrushPresetButton *button : m_presetButtons)
            {
                button->setTabletPressureEnabled(enabled);
            }
        });
    connect(canvas,
        &CanvasWidget::brushPresetChanged,
        this,
        &BrushPopoverPanel::syncToPreset);
    syncToPreset(canvas->brushPresetId());
}

void BrushPopoverPanel::setAnimationActive(bool active)
{
    if (active)
    {
        m_previewTimer.start();
    }
    else
    {
        m_previewTimer.stop();
    }
}

void BrushPopoverPanel::syncToPreset(const QString &presetId)
{
    const BrushPreset *preset = BrushPresetCatalog::find(presetId);
    if (!preset)
    {
        return;
    }
    for (BrushPresetButton *button : m_presetButtons)
    {
        if (button->presetId() == presetId)
        {
            button->setChecked(true);
        }
    }
    const int categoryIndex =
        static_cast<int>(orderedCategories().indexOf(preset->category));
    if (categoryIndex >= 0)
    {
        m_tabGroup->button(categoryIndex)->setChecked(true);
        m_stack->setCurrentIndex(categoryIndex);
    }
}

void BrushPopoverPanel::advancePreviews()
{
    m_previewFrame =
        (m_previewFrame + 1) % BrushPresetButton::previewFrameCount;
    for (BrushPresetButton *button : m_presetButtons)
    {
        button->setPreviewFrame(m_previewFrame);
    }
}

}

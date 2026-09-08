// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "brush/BrushPreset.hpp"
#include "brush/EraserPreset.hpp"
#include "document/DocumentLimits.hpp"
#include "ui/CanvasWidget.hpp"
#include "ui/DrawingToolSettings.hpp"
#include "ui/MainWindow.hpp"
#include "ui/SettingsDialog.hpp"
#include "ui/TimelineBar.hpp"
#include "ui/WwpPresetCodec.hpp"

#include <QFile>
#include <QFileDialog>
#include <QFileInfo>
#include <QLatin1StringView>
#include <QMessageBox>
#include <QSaveFile>
#include <QSettings>
#include <QStatusBar>

#include <algorithm>
#include <cmath>

namespace ugurugu
{

using namespace drawing_tool_settings;

namespace
{

constexpr QLatin1StringView timelineVisibleKey("window/animationBarVisible");

}

bool MainWindow::timelineVisibleSetting()
{
    return QSettings().value(timelineVisibleKey, true).toBool();
}

void MainWindow::setTimelineVisible(bool visible)
{
    if (!m_timeline)
    {
        return;
    }
    m_timeline->setVisible(visible);
    QSettings().setValue(timelineVisibleKey, visible);
}

void MainWindow::restoreDrawingToolSettings()
{
    QSettings settings;
    const bool hasLegacyStabilization =
        settings.contains(legacyStabilizationKey);
    const qreal legacyStabilization = realSetting(
        settings, QString::fromLatin1(legacyStabilizationKey), 0.0, 0.0, 1.0);
    for (const BrushPreset &preset : BrushPresetCatalog::builtIns())
    {
        m_canvas->setBrushPresetWidth(preset.id,
            realSetting(settings,
                presetWidthKey(preset.id),
                preset.defaultSize,
                minimumRememberedStrokeWidth,
                DocumentLimits::maximumStrokeWidth));
        const QString stabilizationKey = presetStabilizationKey(preset.id);
        m_canvas->setBrushPresetStabilization(preset.id,
            realSetting(
                settings, stabilizationKey, legacyStabilization, 0.0, 1.0));
        if (hasLegacyStabilization && !settings.contains(stabilizationKey))
        {
            settings.setValue(stabilizationKey, legacyStabilization);
        }
    }

    const BrushPreset &defaultPreset = BrushPresetCatalog::defaultPreset();
    const QString storedPresetId =
        settings.value(activePresetKey, defaultPreset.id).toString();
    m_canvas->setBrushPreset(BrushPresetCatalog::find(storedPresetId)
                                 ? storedPresetId
                                 : defaultPreset.id);

    const EraserPreset &defaultEraser = EraserPresetCatalog::defaultPreset();
    const bool hasLegacyEraserWidth = settings.contains(eraserWidthKey);
    const bool hasLegacyEraserStabilization =
        settings.contains(eraserStabilizationKey);
    const qreal legacyEraserWidth = realSetting(settings,
        QString::fromLatin1(eraserWidthKey),
        defaultEraser.defaultSize,
        minimumRememberedStrokeWidth,
        DocumentLimits::maximumStrokeWidth);
    const qreal legacyEraserStabilization = realSetting(settings,
        QString::fromLatin1(eraserStabilizationKey),
        legacyStabilization,
        0.0,
        1.0);
    for (const EraserPreset &preset : EraserPresetCatalog::builtIns())
    {
        const qreal widthFallback =
            preset.id == defaultEraser.id && hasLegacyEraserWidth
                ? legacyEraserWidth
                : preset.defaultSize;
        const QString widthKey = eraserPresetWidthKey(preset.id);
        m_canvas->setEraserPresetWidth(preset.id,
            realSetting(settings,
                widthKey,
                widthFallback,
                minimumRememberedStrokeWidth,
                DocumentLimits::maximumStrokeWidth));
        const QString stabilizationKey =
            eraserPresetStabilizationKey(preset.id);
        m_canvas->setEraserPresetStabilization(preset.id,
            realSetting(settings,
                stabilizationKey,
                legacyEraserStabilization,
                0.0,
                1.0));
        if (hasLegacyEraserWidth && preset.id == defaultEraser.id
            && !settings.contains(widthKey))
        {
            settings.setValue(widthKey, legacyEraserWidth);
        }
        if ((hasLegacyEraserStabilization || hasLegacyStabilization)
            && !settings.contains(stabilizationKey))
        {
            settings.setValue(stabilizationKey, legacyEraserStabilization);
        }
    }
    const QString storedEraserPresetId =
        settings.value(activeEraserPresetKey, defaultEraser.id).toString();
    m_canvas->setEraserPreset(EraserPresetCatalog::find(storedEraserPresetId)
                                  ? storedEraserPresetId
                                  : defaultEraser.id);
    m_canvas->setBrushAntialiasing(
        boolSetting(settings, QString::fromLatin1(antialiasingKey), false));
    m_canvas->setTabletPressureEnabled(boolSetting(
        settings, QString::fromLatin1(tabletPressureEnabledKey), true));

    QColor storedColor(settings.value(activeColorKey).toString());
    if (!storedColor.isValid())
    {
        QStringList rememberedColors =
            settings.value(colorHistoryKey).toStringList();
        if (rememberedColors.isEmpty())
        {
            rememberedColors = settings.value(recentColorsKey).toStringList();
        }
        for (const QString &name : rememberedColors)
        {
            const QColor recentColor(name);
            if (recentColor.isValid())
            {
                storedColor = recentColor;
                break;
            }
        }
    }
    m_canvas->setBrushColor(
        storedColor.isValid() ? storedColor : QColor(Qt::black));

    const std::optional<CanvasWidget::WandReference> storedReference =
        wandReferenceFromSettingsId(
            settings.value(wandReferenceKey).toString());
    m_canvas->setWandReference(
        storedReference.value_or(CanvasWidget::WandReference::ActiveLayer));

    const std::optional<CanvasWidget::SelectionShape> storedSelectionShape =
        selectionShapeFromSettingsId(
            settings.value(selectionShapeKey).toString());
    m_canvas->setSelectionShape(
        storedSelectionShape.value_or(CanvasWidget::SelectionShape::Freehand));

    const std::optional<SamplingMode> storedTransformSampling =
        selectionTransformSamplingFromSettingsId(
            settings.value(selectionTransformSamplingKey).toString());
    m_canvas->setSelectionTransformSampling(
        storedTransformSampling.value_or(SamplingMode::Smooth));

    const std::optional<CanvasWidget::LassoMode> storedLassoMode =
        lassoModeFromSettingsId(settings.value(lassoModeKey).toString());
    m_canvas->setLassoMode(
        storedLassoMode.value_or(CanvasWidget::LassoMode::Select));

    const std::optional<CanvasWidget::FillComparison> storedFillComparison =
        fillComparisonFromSettingsId(
            settings.value(fillComparisonKey).toString());
    m_canvas->setFillComparison(storedFillComparison.value_or(
        CanvasWidget::FillComparison::AlphaBoundary));
    m_canvas->setFillTolerance(qRound(realSetting(
        settings, QString::fromLatin1(fillToleranceKey), 32.0, 0.0, 255.0)));
    m_canvas->setBucketAntialiasing(boolSetting(
        settings, QString::fromLatin1(bucketAntialiasingKey), true));

    m_canvas->setTextFontFamily(settings.value(textFontFamilyKey).toString());
    m_canvas->setTextFontSize(realSetting(
        settings, QString::fromLatin1(textFontSizeKey), 48.0, 8.0, 512.0));
    m_canvas->setTextFilled(
        boolSetting(settings, QString::fromLatin1(textFilledKey), false));

    const std::optional<CanvasWidget::Tool> storedTool =
        toolFromSettingsId(settings.value(activeToolKey).toString());
    m_canvas->setTool(storedTool.value_or(CanvasWidget::Tool::Brush));

    if (hasLegacyStabilization || hasLegacyEraserWidth
        || hasLegacyEraserStabilization)
    {
        settings.remove(legacyStabilizationKey);
        settings.remove(eraserWidthKey);
        settings.remove(eraserStabilizationKey);
        settings.sync();
    }
}

void MainWindow::connectDrawingToolSettings()
{
    const auto schedule = [this]()
    {
        scheduleDrawingToolSettingsSave();
    };
    connect(m_canvas,
        &CanvasWidget::toolChanged,
        this,
        [schedule](CanvasWidget::Tool)
        {
            schedule();
        });
    connect(m_canvas,
        &CanvasWidget::brushColorChanged,
        this,
        [schedule](const QColor &)
        {
            schedule();
        });
    connect(m_canvas,
        &CanvasWidget::brushWidthChanged,
        this,
        [schedule](qreal)
        {
            schedule();
        });
    connect(m_canvas,
        &CanvasWidget::eraserWidthChanged,
        this,
        [schedule](qreal)
        {
            schedule();
        });
    connect(m_canvas,
        &CanvasWidget::brushStabilizationChanged,
        this,
        [schedule](qreal)
        {
            schedule();
        });
    connect(m_canvas,
        &CanvasWidget::eraserStabilizationChanged,
        this,
        [schedule](qreal)
        {
            schedule();
        });
    connect(m_canvas,
        &CanvasWidget::brushAntialiasingChanged,
        this,
        [schedule](bool)
        {
            schedule();
        });
    connect(m_canvas,
        &CanvasWidget::tabletPressureEnabledChanged,
        this,
        [schedule](bool)
        {
            schedule();
        });
    connect(m_canvas,
        &CanvasWidget::brushPresetChanged,
        this,
        [schedule](const QString &)
        {
            schedule();
        });
    connect(m_canvas,
        &CanvasWidget::eraserPresetChanged,
        this,
        [schedule](const QString &)
        {
            schedule();
        });
    connect(m_canvas,
        &CanvasWidget::wandReferenceChanged,
        this,
        [schedule](CanvasWidget::WandReference)
        {
            schedule();
        });
    connect(m_canvas,
        &CanvasWidget::selectionShapeChanged,
        this,
        [schedule](CanvasWidget::SelectionShape)
        {
            schedule();
        });
    connect(m_canvas,
        &CanvasWidget::selectionTransformSamplingChanged,
        this,
        [schedule](SamplingMode)
        {
            schedule();
        });
    connect(m_canvas,
        &CanvasWidget::lassoModeChanged,
        this,
        [schedule](CanvasWidget::LassoMode)
        {
            schedule();
        });
    connect(m_canvas,
        &CanvasWidget::fillComparisonChanged,
        this,
        [schedule](CanvasWidget::FillComparison)
        {
            schedule();
        });
    connect(m_canvas,
        &CanvasWidget::fillToleranceChanged,
        this,
        [schedule](int)
        {
            schedule();
        });
    connect(m_canvas,
        &CanvasWidget::bucketAntialiasingChanged,
        this,
        [schedule](bool)
        {
            schedule();
        });
    connect(m_canvas,
        &CanvasWidget::textFontFamilyChanged,
        this,
        [schedule](const QString &)
        {
            schedule();
        });
    connect(m_canvas,
        &CanvasWidget::textFontSizeChanged,
        this,
        [schedule](qreal)
        {
            schedule();
        });
    connect(m_canvas,
        &CanvasWidget::textFilledChanged,
        this,
        [schedule](bool)
        {
            schedule();
        });
}

void MainWindow::scheduleDrawingToolSettingsSave()
{
    m_drawingToolSettingsSaveTimer.start();
}

void MainWindow::saveDrawingToolSettings()
{
    m_drawingToolSettingsSaveTimer.stop();
    QSettings settings;
    settings.setValue(activeToolKey, toolSettingsId(m_canvas->tool()));
    settings.setValue(activePresetKey, m_canvas->brushPresetId());
    settings.setValue(activeEraserPresetKey, m_canvas->eraserPresetId());
    settings.setValue(
        activeColorKey, m_canvas->brushColor().name(QColor::HexArgb));
    settings.setValue(antialiasingKey, m_canvas->brushAntialiasing());
    settings.setValue(
        tabletPressureEnabledKey, m_canvas->tabletPressureEnabled());
    settings.setValue(
        wandReferenceKey, wandReferenceSettingsId(m_canvas->wandReference()));
    settings.setValue(selectionShapeKey,
        selectionShapeSettingsId(m_canvas->selectionShape()));
    settings.setValue(selectionTransformSamplingKey,
        selectionTransformSamplingSettingsId(
            m_canvas->selectionTransformSampling()));
    settings.setValue(lassoModeKey, lassoModeSettingsId(m_canvas->lassoMode()));
    settings.setValue(fillComparisonKey,
        fillComparisonSettingsId(m_canvas->fillComparison()));
    settings.setValue(fillToleranceKey, m_canvas->fillTolerance());
    settings.setValue(bucketAntialiasingKey, m_canvas->bucketAntialiasing());
    settings.setValue(textFontFamilyKey, m_canvas->textFontFamily());
    settings.setValue(textFontSizeKey, m_canvas->textFontSize());
    settings.setValue(textFilledKey, m_canvas->textFilled());
    for (const BrushPreset &preset : BrushPresetCatalog::builtIns())
    {
        settings.setValue(
            presetWidthKey(preset.id), m_canvas->brushPresetWidth(preset.id));
        settings.setValue(presetStabilizationKey(preset.id),
            m_canvas->brushPresetStabilization(preset.id));
    }
    for (const EraserPreset &preset : EraserPresetCatalog::builtIns())
    {
        settings.setValue(eraserPresetWidthKey(preset.id),
            m_canvas->eraserPresetWidth(preset.id));
        settings.setValue(eraserPresetStabilizationKey(preset.id),
            m_canvas->eraserPresetStabilization(preset.id));
    }
    settings.sync();
}

void MainWindow::exportWwpPreset()
{
    saveDrawingToolSettings();
    QSettings settings;
    const QByteArray data = WwpPresetCodec::encode(
        WwpPresetCodec::capture(settings, m_controller.document()));
    if (data.isEmpty())
    {
        QMessageBox::critical(this,
            tr("Preset export failed"),
            tr("Could not prepare the preset."));
        return;
    }
    const QString selected = QFileDialog::getSaveFileName(this,
        tr("Export WWP preset"),
        saveDialogStartPath(QStringLiteral("wwpreset")),
        tr("Ugurugu presets (*.wwpreset)"));
    if (selected.isEmpty())
    {
        return;
    }
    const QString filePath =
        normalizedPath(selected, QStringLiteral("wwpreset"));
    QSaveFile file(filePath);
    if (!file.open(QIODevice::WriteOnly) || file.write(data) != data.size()
        || !file.commit())
    {
        QMessageBox::critical(this,
            tr("Preset export failed"),
            tr("Could not write the preset.\n\n%1").arg(file.errorString()));
        return;
    }
    statusBar()->showMessage(tr("Exported preset %1").arg(filePath), 4000);
}

void MainWindow::importWwpPreset()
{
    const QString filePath = QFileDialog::getOpenFileName(this,
        tr("Import WWP preset"),
        SettingsDialog::defaultSaveFolder(),
        tr("Ugurugu presets (*.wwpreset)"));
    if (filePath.isEmpty())
    {
        return;
    }
    QFile file(filePath);
    QString error;
    std::optional<WwpPreset> preset;
    if (!file.open(QIODevice::ReadOnly) || file.size() < 0
        || file.size() > WwpPresetCodec::maximumBytes)
    {
        error = tr("The preset could not be read or is too large.");
    }
    else
    {
        const QByteArray data = file.read(WwpPresetCodec::maximumBytes + 1);
        if (data.size() > WwpPresetCodec::maximumBytes || !file.atEnd())
        {
            error = tr("The preset could not be read or is too large.");
        }
        else
        {
            preset = WwpPresetCodec::decode(data, &error);
        }
    }
    if (!preset
        || !m_controller.applyMotionPreset(
            preset->wobbleAmount, preset->motion))
    {
        QMessageBox::critical(this,
            tr("Preset import failed"),
            tr("Could not import the preset.\n\n%1")
                .arg(error.isEmpty() ? tr("The preset settings are invalid.")
                                     : error));
        return;
    }
    QSettings settings;
    WwpPresetCodec::applyDrawingTools(*preset, settings);
    restoreDrawingToolSettings();
    statusBar()->showMessage(
        tr("Imported preset %1").arg(QFileInfo(filePath).fileName()), 4000);
}
}

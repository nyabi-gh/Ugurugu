// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "brush/EraserPreset.hpp"

#include <QCoreApplication>

namespace ugurugu
{

namespace
{

BrushSettings hardEraser()
{
    BrushSettings settings;
    settings.engine = BrushEngine::Line;
    settings.opacity = 1.0;
    settings.sizeDynamics = 0.8;
    settings.antialiasing = true;
    return settings;
}

BrushSettings softEraser()
{
    BrushSettings settings;
    settings.engine = BrushEngine::Airbrush;
    settings.opacity = 0.9;
    settings.flow = 0.16;
    settings.hardness = 0.0;
    settings.spacing = 0.08;
    settings.sizeDynamics = 0.25;
    settings.opacityDynamics = 0.65;
    settings.antialiasing = true;
    return settings;
}

BrushSettings kneadedEraser()
{
    BrushSettings settings;
    settings.engine = BrushEngine::Spray;
    settings.opacity = 0.72;
    settings.flow = 0.42;
    settings.spacing = 0.1;
    settings.scatter = 0.55;
    settings.particleSize = 0.13;
    settings.density = 1.2;
    settings.sizeDynamics = 0.3;
    settings.opacityDynamics = 0.55;
    settings.sizeJitter = 0.65;
    return settings;
}

}

const QVector<EraserPreset> &EraserPresetCatalog::builtIns()
{
    static const QVector<EraserPreset> presets{
        {QStringLiteral("hard-eraser"),
            QT_TRANSLATE_NOOP("EraserPresets", "Hard"),
            hardEraser(),
            6.0},
        {QStringLiteral("soft-eraser"),
            QT_TRANSLATE_NOOP("EraserPresets", "Soft"),
            softEraser(),
            64.0},
        {QStringLiteral("kneaded-eraser"),
            QT_TRANSLATE_NOOP("EraserPresets", "Kneaded"),
            kneadedEraser(),
            48.0}};
    return presets;
}

const EraserPreset &EraserPresetCatalog::defaultPreset()
{
    return builtIns().first();
}

const EraserPreset *EraserPresetCatalog::find(const QString &id)
{
    for (const EraserPreset &preset : builtIns())
    {
        if (preset.id == id)
        {
            return &preset;
        }
    }
    return nullptr;
}

QString EraserPresetCatalog::displayName(const EraserPreset &preset)
{
    return QCoreApplication::translate("EraserPresets", preset.sourceName);
}

}

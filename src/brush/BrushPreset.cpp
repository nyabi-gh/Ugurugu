// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "brush/BrushPreset.hpp"

#include <QCoreApplication>

namespace ugurugu
{

namespace
{

constexpr auto penCategoryName = QT_TRANSLATE_NOOP("BrushPresets", "Pen");
constexpr auto markerCategoryName = QT_TRANSLATE_NOOP("BrushPresets", "Marker");
constexpr auto airbrushCategoryName =
    QT_TRANSLATE_NOOP("BrushPresets", "Airbrush");
constexpr auto sprayCategoryName = QT_TRANSLATE_NOOP("BrushPresets", "Spray");

BrushSettings lineBrush(qreal opacity,
    qreal sizeDynamics,
    BrushTipShape tipShape = BrushTipShape::Round)
{
    BrushSettings settings;
    settings.engine = BrushEngine::Line;
    settings.tipShape = tipShape;
    settings.opacity = opacity;
    settings.sizeDynamics = sizeDynamics;
    return settings;
}

BrushSettings airbrush(qreal opacity,
    qreal flow,
    qreal hardness,
    qreal spacing,
    qreal sizeDynamics,
    qreal opacityDynamics)
{
    BrushSettings settings;
    settings.engine = BrushEngine::Airbrush;
    settings.opacity = opacity;
    settings.flow = flow;
    settings.hardness = hardness;
    settings.spacing = spacing;
    settings.sizeDynamics = sizeDynamics;
    settings.opacityDynamics = opacityDynamics;
    return settings;
}

BrushSettings spray(BrushTipShape tipShape,
    qreal opacity,
    qreal flow,
    qreal spacing,
    qreal scatter,
    qreal particleSize,
    qreal density,
    qreal sizeDynamics,
    qreal opacityDynamics,
    qreal sizeJitter,
    bool animatedJitter = false)
{
    BrushSettings settings;
    settings.engine = BrushEngine::Spray;
    settings.tipShape = tipShape;
    settings.opacity = opacity;
    settings.flow = flow;
    settings.spacing = spacing;
    settings.scatter = scatter;
    settings.particleSize = particleSize;
    settings.density = density;
    settings.sizeDynamics = sizeDynamics;
    settings.opacityDynamics = opacityDynamics;
    settings.sizeJitter = sizeJitter;
    settings.animatedJitter = animatedJitter;
    return settings;
}

}

const QVector<BrushPreset> &BrushPresetCatalog::builtIns()
{
    static const QVector<BrushPreset> presets{
        {QStringLiteral("ink-pen"),
            BrushCategory::Pen,
            QT_TRANSLATE_NOOP("BrushPresets", "Ink Pen"),
            lineBrush(1.0, 0.8),
            6.0},
        {QStringLiteral("g-pen"),
            BrushCategory::Pen,
            QT_TRANSLATE_NOOP("BrushPresets", "G-Pen"),
            lineBrush(1.0, 0.95),
            7.0},
        {QStringLiteral("round-pen"),
            BrushCategory::Pen,
            QT_TRANSLATE_NOOP("BrushPresets", "Round Pen"),
            lineBrush(1.0, 0.6),
            8.0},
        {QStringLiteral("monoline"),
            BrushCategory::Pen,
            QT_TRANSLATE_NOOP("BrushPresets", "Monoline"),
            lineBrush(1.0, 0.0),
            6.0},
        {QStringLiteral("bold-ink"),
            BrushCategory::Pen,
            QT_TRANSLATE_NOOP("BrushPresets", "Bold Ink"),
            lineBrush(1.0, 0.35),
            16.0},
        {QStringLiteral("opaque-marker"),
            BrushCategory::Marker,
            QT_TRANSLATE_NOOP("BrushPresets", "Opaque Marker"),
            lineBrush(0.92, 0.12, BrushTipShape::Square),
            20.0},
        {QStringLiteral("transparent-marker"),
            BrushCategory::Marker,
            QT_TRANSLATE_NOOP("BrushPresets", "Transparent Marker"),
            lineBrush(0.38, 0.05, BrushTipShape::Square),
            28.0},
        {QStringLiteral("highlighter"),
            BrushCategory::Marker,
            QT_TRANSLATE_NOOP("BrushPresets", "Highlighter"),
            lineBrush(0.22, 0.0, BrushTipShape::Square),
            36.0},
        {QStringLiteral("soft-airbrush"),
            BrushCategory::Airbrush,
            QT_TRANSLATE_NOOP("BrushPresets", "Soft Airbrush"),
            airbrush(0.9, 0.11, 0.0, 0.09, 0.15, 0.75),
            64.0},
        {QStringLiteral("hard-airbrush"),
            BrushCategory::Airbrush,
            QT_TRANSLATE_NOOP("BrushPresets", "Hard Airbrush"),
            airbrush(0.95, 0.18, 0.72, 0.12, 0.25, 0.55),
            48.0},
        {QStringLiteral("dense-airbrush"),
            BrushCategory::Airbrush,
            QT_TRANSLATE_NOOP("BrushPresets", "Dense Airbrush"),
            airbrush(1.0, 0.28, 0.35, 0.08, 0.2, 0.45),
            44.0},
        {QStringLiteral("fine-mist"),
            BrushCategory::Airbrush,
            QT_TRANSLATE_NOOP("BrushPresets", "Fine Mist"),
            airbrush(0.75, 0.07, 0.12, 0.07, 0.05, 0.85),
            34.0},
        {QStringLiteral("pixel-spray"),
            BrushCategory::Spray,
            QT_TRANSLATE_NOOP("BrushPresets", "Pixel Spray"),
            spray(BrushTipShape::Square,
                1.0,
                0.55,
                0.13,
                0.9,
                0.06,
                1.4,
                0.15,
                0.65,
                0.4),
            44.0},
        {QStringLiteral("rough-spray"),
            BrushCategory::Spray,
            QT_TRANSLATE_NOOP("BrushPresets", "Rough Spray"),
            spray(BrushTipShape::Round,
                0.9,
                0.38,
                0.14,
                1.1,
                0.11,
                1.0,
                0.2,
                0.55,
                0.85),
            58.0},
        {QStringLiteral("dust-spray"),
            BrushCategory::Spray,
            QT_TRANSLATE_NOOP("BrushPresets", "Dust Spray"),
            spray(BrushTipShape::Square,
                0.8,
                0.28,
                0.1,
                1.25,
                0.035,
                2.4,
                0.05,
                0.8,
                0.6),
            52.0},
        {QStringLiteral("droplet-spray"),
            BrushCategory::Spray,
            QT_TRANSLATE_NOOP("BrushPresets", "Droplet Spray"),
            spray(BrushTipShape::Round,
                1.0,
                0.68,
                0.2,
                1.2,
                0.22,
                0.35,
                0.3,
                0.35,
                0.75),
            72.0},
        {QStringLiteral("wobble-spray"),
            BrushCategory::Spray,
            QT_TRANSLATE_NOOP("BrushPresets", "Wobble Spray"),
            spray(BrushTipShape::Square,
                0.95,
                0.48,
                0.13,
                1.0,
                0.075,
                1.2,
                0.15,
                0.65,
                0.55,
                true),
            48.0}};
    return presets;
}

const BrushPreset &BrushPresetCatalog::defaultPreset()
{
    return builtIns().first();
}

const BrushPreset *BrushPresetCatalog::find(const QString &id)
{
    for (const BrushPreset &preset : builtIns())
    {
        if (preset.id == id)
        {
            return &preset;
        }
    }
    return nullptr;
}

QString BrushPresetCatalog::displayName(const BrushPreset &preset)
{
    return QCoreApplication::translate("BrushPresets", preset.sourceName);
}

QString BrushPresetCatalog::categoryName(BrushCategory category)
{
    switch (category)
    {
    case BrushCategory::Pen:
        return QCoreApplication::translate("BrushPresets", penCategoryName);
    case BrushCategory::Marker:
        return QCoreApplication::translate("BrushPresets", markerCategoryName);
    case BrushCategory::Airbrush:
        return QCoreApplication::translate(
            "BrushPresets", airbrushCategoryName);
    case BrushCategory::Spray:
        return QCoreApplication::translate("BrushPresets", sprayCategoryName);
    }
    return {};
}

}

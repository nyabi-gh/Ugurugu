// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "ui/DrawingToolSettings.hpp"

#include <QSettings>

#include <algorithm>
#include <cmath>

namespace ugurugu
{
namespace drawing_tool_settings
{

QString presetWidthKey(const QString &presetId)
{
    return QStringLiteral("drawingTools/brush/presetWidths/%1").arg(presetId);
}

QString presetStabilizationKey(const QString &presetId)
{
    return QStringLiteral("drawingTools/brush/presetStabilizations/%1")
        .arg(presetId);
}

QString eraserPresetWidthKey(const QString &presetId)
{
    return QStringLiteral("drawingTools/eraser/presetWidths/%1").arg(presetId);
}

QString eraserPresetStabilizationKey(const QString &presetId)
{
    return QStringLiteral("drawingTools/eraser/presetStabilizations/%1")
        .arg(presetId);
}

qreal realSetting(const QSettings &settings,
    const QString &key,
    qreal fallback,
    qreal minimum,
    qreal maximum)
{
    bool converted = false;
    const qreal value = settings.value(key).toDouble(&converted);
    if (!converted || !std::isfinite(value))
    {
        return fallback;
    }
    return std::clamp(value, minimum, maximum);
}

bool boolSetting(const QSettings &settings, const QString &key, bool fallback)
{
    if (!settings.contains(key))
    {
        return fallback;
    }
    const QVariant value = settings.value(key);
    if (value.metaType().id() == QMetaType::Bool)
    {
        return value.toBool();
    }
    const QString text = value.toString().trimmed().toLower();
    if (text == QStringLiteral("true") || text == QStringLiteral("1"))
    {
        return true;
    }
    if (text == QStringLiteral("false") || text == QStringLiteral("0"))
    {
        return false;
    }
    return fallback;
}

QString toolSettingsId(CanvasWidget::Tool tool)
{
    switch (tool)
    {
    case CanvasWidget::Tool::Brush:
        return QStringLiteral("brush");
    case CanvasWidget::Tool::Eraser:
        return QStringLiteral("eraser");
    case CanvasWidget::Tool::Lasso:
        return QStringLiteral("lasso");
    case CanvasWidget::Tool::Wand:
        return QStringLiteral("wand");
    case CanvasWidget::Tool::Bucket:
        return QStringLiteral("bucket");
    case CanvasWidget::Tool::Text:
        return QStringLiteral("text");
    case CanvasWidget::Tool::Eyedropper:
        return QStringLiteral("eyedropper");
    }
    return QStringLiteral("brush");
}

std::optional<CanvasWidget::Tool> toolFromSettingsId(const QString &id)
{
    if (id == QStringLiteral("brush"))
    {
        return CanvasWidget::Tool::Brush;
    }
    if (id == QStringLiteral("eraser"))
    {
        return CanvasWidget::Tool::Eraser;
    }
    if (id == QStringLiteral("lasso"))
    {
        return CanvasWidget::Tool::Lasso;
    }
    if (id == QStringLiteral("wand"))
    {
        return CanvasWidget::Tool::Wand;
    }
    if (id == QStringLiteral("bucket"))
    {
        return CanvasWidget::Tool::Bucket;
    }
    if (id == QStringLiteral("text"))
    {
        return CanvasWidget::Tool::Text;
    }
    if (id == QStringLiteral("eyedropper"))
    {
        return CanvasWidget::Tool::Eyedropper;
    }
    return std::nullopt;
}

QString selectionShapeSettingsId(CanvasWidget::SelectionShape shape)
{
    switch (shape)
    {
    case CanvasWidget::SelectionShape::Freehand:
        return QStringLiteral("freehand");
    case CanvasWidget::SelectionShape::Rectangle:
        return QStringLiteral("rectangle");
    case CanvasWidget::SelectionShape::Ellipse:
        return QStringLiteral("ellipse");
    }
    return QStringLiteral("freehand");
}

std::optional<CanvasWidget::SelectionShape> selectionShapeFromSettingsId(
    const QString &id)
{
    if (id == QStringLiteral("freehand"))
    {
        return CanvasWidget::SelectionShape::Freehand;
    }
    if (id == QStringLiteral("rectangle"))
    {
        return CanvasWidget::SelectionShape::Rectangle;
    }
    if (id == QStringLiteral("ellipse"))
    {
        return CanvasWidget::SelectionShape::Ellipse;
    }
    return std::nullopt;
}

QString selectionTransformSamplingSettingsId(SamplingMode sampling)
{
    return sampling == SamplingMode::Nearest ? QStringLiteral("nearest")
                                             : QStringLiteral("smooth");
}

std::optional<SamplingMode> selectionTransformSamplingFromSettingsId(
    const QString &id)
{
    if (id == QStringLiteral("smooth"))
    {
        return SamplingMode::Smooth;
    }
    if (id == QStringLiteral("nearest"))
    {
        return SamplingMode::Nearest;
    }
    return std::nullopt;
}

QString lassoModeSettingsId(CanvasWidget::LassoMode mode)
{
    return mode == CanvasWidget::LassoMode::Paint ? QStringLiteral("paint")
                                                  : QStringLiteral("select");
}

std::optional<CanvasWidget::LassoMode> lassoModeFromSettingsId(
    const QString &id)
{
    if (id == QStringLiteral("select"))
    {
        return CanvasWidget::LassoMode::Select;
    }
    if (id == QStringLiteral("paint"))
    {
        return CanvasWidget::LassoMode::Paint;
    }
    return std::nullopt;
}

QString wandReferenceSettingsId(CanvasWidget::WandReference reference)
{
    switch (reference)
    {
    case CanvasWidget::WandReference::ActiveLayer:
        return QStringLiteral("active");
    case CanvasWidget::WandReference::ReferenceLayers:
        return QStringLiteral("reference");
    case CanvasWidget::WandReference::AllVisibleLayers:
        return QStringLiteral("visible");
    }
    return QStringLiteral("active");
}

std::optional<CanvasWidget::WandReference> wandReferenceFromSettingsId(
    const QString &id)
{
    if (id == QStringLiteral("active"))
    {
        return CanvasWidget::WandReference::ActiveLayer;
    }
    if (id == QStringLiteral("reference"))
    {
        return CanvasWidget::WandReference::ReferenceLayers;
    }
    if (id == QStringLiteral("visible"))
    {
        return CanvasWidget::WandReference::AllVisibleLayers;
    }
    return std::nullopt;
}

QString fillComparisonSettingsId(CanvasWidget::FillComparison comparison)
{
    return comparison == CanvasWidget::FillComparison::Color
               ? QStringLiteral("color")
               : QStringLiteral("alpha");
}

std::optional<CanvasWidget::FillComparison> fillComparisonFromSettingsId(
    const QString &id)
{
    if (id == QStringLiteral("alpha"))
    {
        return CanvasWidget::FillComparison::AlphaBoundary;
    }
    if (id == QStringLiteral("color"))
    {
        return CanvasWidget::FillComparison::Color;
    }
    return std::nullopt;
}

}

}

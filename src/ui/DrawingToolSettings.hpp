// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#pragma once

#include "ui/CanvasWidget.hpp"

#include <QString>

#include <optional>

class QSettings;

namespace ugurugu
{
namespace drawing_tool_settings
{

// Keys and value encodings for the persisted drawing tool state.
//
// These strings and ids are written into the user's QSettings, so an existing
// name may not be renamed or repurposed — doing so silently discards whatever
// the user had configured. Add a new key instead, and keep reading the old
// one for as long as migration matters.

constexpr auto activeToolKey = "drawingTools/activeTool";
constexpr auto activePresetKey = "drawingTools/brush/presetId";
constexpr auto activeEraserPresetKey = "drawingTools/eraser/presetId";
constexpr auto activeColorKey = "drawingTools/brush/color";
constexpr auto colorHistoryKey = "brush/colorHistory";
constexpr auto recentColorsKey = "brush/recentColors";
constexpr auto antialiasingKey = "drawingTools/brush/antialiasing";
constexpr auto tabletPressureEnabledKey = "drawingTools/tablet/pressureEnabled";
constexpr auto eraserWidthKey = "drawingTools/eraser/width";
constexpr auto eraserStabilizationKey = "drawingTools/eraser/stabilization";
constexpr auto selectionShapeKey = "drawingTools/selection/shape";
constexpr auto selectionTransformSamplingKey =
    "drawingTools/selection/transformSampling";
constexpr auto lassoModeKey = "drawingTools/lasso/mode";
constexpr auto wandReferenceKey = "drawingTools/wand/reference";
constexpr auto fillComparisonKey = "drawingTools/fill/comparison";
constexpr auto fillToleranceKey = "drawingTools/fill/tolerance";
constexpr auto bucketAntialiasingKey = "drawingTools/fill/antialiasing";
constexpr auto textFontFamilyKey = "drawingTools/text/fontFamily";
constexpr auto textFontSizeKey = "drawingTools/text/size";
constexpr auto textFilledKey = "drawingTools/text/filled";
constexpr auto legacyStabilizationKey = "canvas/strokeStabilization";

constexpr qreal minimumRememberedStrokeWidth = 1.0;

QString presetWidthKey(const QString &presetId);
QString presetStabilizationKey(const QString &presetId);
QString eraserPresetWidthKey(const QString &presetId);
QString eraserPresetStabilizationKey(const QString &presetId);

// Both readers fall back rather than propagate a stored value that no longer
// parses, so a corrupted settings file degrades to defaults instead of
// putting the tools into an unusable state.
qreal realSetting(const QSettings &settings,
    const QString &key,
    qreal fallback,
    qreal minimum,
    qreal maximum);

bool boolSetting(const QSettings &settings, const QString &key, bool fallback);

// The stored id is the stable name; the enumerator value is not, so never
// persist the enum directly.
QString toolSettingsId(CanvasWidget::Tool tool);
std::optional<CanvasWidget::Tool> toolFromSettingsId(const QString &id);

QString selectionShapeSettingsId(CanvasWidget::SelectionShape shape);
std::optional<CanvasWidget::SelectionShape> selectionShapeFromSettingsId(
    const QString &id);

QString selectionTransformSamplingSettingsId(SamplingMode sampling);
std::optional<SamplingMode> selectionTransformSamplingFromSettingsId(
    const QString &id);

QString lassoModeSettingsId(CanvasWidget::LassoMode mode);
std::optional<CanvasWidget::LassoMode> lassoModeFromSettingsId(
    const QString &id);

QString wandReferenceSettingsId(CanvasWidget::WandReference reference);
std::optional<CanvasWidget::WandReference> wandReferenceFromSettingsId(
    const QString &id);

QString fillComparisonSettingsId(CanvasWidget::FillComparison comparison);
std::optional<CanvasWidget::FillComparison> fillComparisonFromSettingsId(
    const QString &id);

}

}

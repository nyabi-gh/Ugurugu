// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "ui/WwpPresetCodec.hpp"

#include "brush/BrushPreset.hpp"
#include "brush/EraserPreset.hpp"
#include "document/DocumentLimits.hpp"

#include <QJsonDocument>
#include <QJsonObject>
#include <QJsonParseError>
#include <QSet>
#include <QSettings>
#include <QStringList>

#include <cmath>

namespace ugurugu
{

namespace
{

constexpr int formatVersion = 1;

// Written into the "format" field of every preset this application saves.
QString formatIdentifier()
{
    return QStringLiteral("UGURUGU_PRESET");
}

// The same layout under earlier names of the application, so presets already
// on disk stay readable while only the current identifier is ever written.
QStringList legacyFormatIdentifiers()
{
    return {QStringLiteral("WAGLEWAGLEPAINT_PRESET")};
}

bool supportedFormatIdentifier(const QString &candidate)
{
    return candidate == formatIdentifier()
           || legacyFormatIdentifiers().contains(candidate);
}

void setError(QString *error, const QString &message)
{
    if (error)
    {
        *error = message;
    }
}

QSet<QString> allowedKeys()
{
    QSet<QString> keys{QStringLiteral("activeTool"),
        QStringLiteral("brush/presetId"),
        QStringLiteral("eraser/presetId"),
        QStringLiteral("brush/color"),
        QStringLiteral("brush/antialiasing"),
        QStringLiteral("selection/shape"),
        QStringLiteral("lasso/mode"),
        QStringLiteral("wand/reference"),
        QStringLiteral("fill/comparison"),
        QStringLiteral("fill/tolerance"),
        QStringLiteral("fill/antialiasing")};
    for (const BrushPreset &preset : BrushPresetCatalog::builtIns())
    {
        keys.insert(QStringLiteral("brush/presetWidths/%1").arg(preset.id));
        keys.insert(
            QStringLiteral("brush/presetStabilizations/%1").arg(preset.id));
    }
    for (const EraserPreset &preset : EraserPresetCatalog::builtIns())
    {
        keys.insert(QStringLiteral("eraser/presetWidths/%1").arg(preset.id));
        keys.insert(
            QStringLiteral("eraser/presetStabilizations/%1").arg(preset.id));
    }
    return keys;
}

QString motionStyleId(MotionStyle style)
{
    switch (style)
    {
    case MotionStyle::Classic:
        return QStringLiteral("classic");
    case MotionStyle::Smooth:
        return QStringLiteral("smooth");
    case MotionStyle::Stepped:
        return QStringLiteral("stepped");
    }
    return QStringLiteral("classic");
}

std::optional<MotionStyle> motionStyleFromId(const QString &id)
{
    if (id == QStringLiteral("classic"))
    {
        return MotionStyle::Classic;
    }
    if (id == QStringLiteral("smooth"))
    {
        return MotionStyle::Smooth;
    }
    if (id == QStringLiteral("stepped"))
    {
        return MotionStyle::Stepped;
    }
    return std::nullopt;
}

bool finiteNumber(const QJsonValue &value)
{
    return value.isDouble() && std::isfinite(value.toDouble());
}

bool integerNumber(const QJsonValue &value)
{
    return finiteNumber(value)
           && std::floor(value.toDouble()) == value.toDouble();
}

bool validToolValue(const QString &key, const QJsonValue &value)
{
    if (key == QStringLiteral("brush/antialiasing")
        || key == QStringLiteral("fill/antialiasing"))
    {
        return value.isBool();
    }
    if (key == QStringLiteral("activeTool")
        || key == QStringLiteral("brush/presetId")
        || key == QStringLiteral("eraser/presetId")
        || key == QStringLiteral("brush/color")
        || key == QStringLiteral("selection/shape")
        || key == QStringLiteral("lasso/mode")
        || key == QStringLiteral("wand/reference")
        || key == QStringLiteral("fill/comparison"))
    {
        return value.isString() && value.toString().size() <= 128;
    }
    return finiteNumber(value);
}

bool validMotion(const WwpPreset &preset)
{
    return std::isfinite(preset.wobbleAmount)
           && preset.wobbleAmount >= DocumentLimits::minimumWobbleAmount
           && preset.wobbleAmount <= DocumentLimits::maximumWobbleAmount
           && isValidMotionStyle(preset.motion.style)
           && preset.motion.poseCount >= DocumentLimits::minimumMotionPoseCount
           && preset.motion.poseCount <= DocumentLimits::maximumMotionPoseCount
           && preset.motion.detail >= DocumentLimits::minimumMotionDetail
           && preset.motion.detail <= DocumentLimits::maximumMotionDetail
           && std::isfinite(preset.motion.linked) && preset.motion.linked >= 0.0
           && preset.motion.linked <= 1.0
           && std::isfinite(preset.motion.randomness)
           && preset.motion.randomness >= 0.0 && preset.motion.randomness <= 1.0
           && std::isfinite(preset.motion.breakAmount)
           && preset.motion.breakAmount >= 0.0
           && preset.motion.breakAmount <= 1.0
           && std::isfinite(preset.motion.breakRange)
           && preset.motion.breakRange >= DocumentLimits::minimumBreakRange
           && preset.motion.breakRange <= DocumentLimits::maximumBreakRange;
}

}

WwpPreset WwpPresetCodec::capture(QSettings &settings, const Document &document)
{
    WwpPreset preset;
    preset.wobbleAmount = document.wobbleAmount;
    preset.motion = document.motion;
    settings.beginGroup(QStringLiteral("drawingTools"));
    for (const QString &key : allowedKeys())
    {
        if (settings.contains(key))
        {
            preset.drawingTools.insert(key, settings.value(key));
        }
    }
    settings.endGroup();
    return preset;
}

QByteArray WwpPresetCodec::encode(const WwpPreset &preset)
{
    if (!validMotion(preset))
    {
        return {};
    }
    QJsonObject tools;
    const QSet<QString> allowed = allowedKeys();
    for (auto iterator = preset.drawingTools.cbegin();
        iterator != preset.drawingTools.cend();
        ++iterator)
    {
        if (allowed.contains(iterator.key()))
        {
            tools.insert(
                iterator.key(), QJsonValue::fromVariant(iterator.value()));
        }
    }
    QJsonObject motion{{QStringLiteral("amount"), preset.wobbleAmount},
        {QStringLiteral("style"), motionStyleId(preset.motion.style)},
        {QStringLiteral("poseCount"), preset.motion.poseCount},
        {QStringLiteral("detail"), preset.motion.detail},
        {QStringLiteral("linked"), preset.motion.linked},
        {QStringLiteral("randomness"), preset.motion.randomness},
        {QStringLiteral("brokenLine"), preset.motion.brokenLine},
        {QStringLiteral("breakAmount"), preset.motion.breakAmount},
        {QStringLiteral("breakRange"), preset.motion.breakRange}};
    return QJsonDocument(
        QJsonObject{{QStringLiteral("format"), formatIdentifier()},
            {QStringLiteral("version"), formatVersion},
            {QStringLiteral("drawingTools"), tools},
            {QStringLiteral("motion"), motion}})
        .toJson(QJsonDocument::Indented);
}

std::optional<WwpPreset> WwpPresetCodec::decode(
    const QByteArray &data, QString *error)
{
    if (data.isEmpty() || data.size() > maximumBytes)
    {
        setError(
            error, QStringLiteral("The WWP preset is empty or too large."));
        return std::nullopt;
    }
    if (data.startsWith("WIGGLEWIGGLETOOL_PRESET=1"))
    {
        setError(error,
            QStringLiteral("Native WiggleWiggleTool presets are not supported "
                           "by this WWP preset importer."));
        return std::nullopt;
    }
    QJsonParseError parseError;
    const QJsonDocument json = QJsonDocument::fromJson(data, &parseError);
    if (parseError.error != QJsonParseError::NoError || !json.isObject())
    {
        setError(error, QStringLiteral("The WWP preset is not valid JSON."));
        return std::nullopt;
    }
    const QJsonObject root = json.object();
    if (!supportedFormatIdentifier(
            root.value(QStringLiteral("format")).toString())
        || root.value(QStringLiteral("version")).toInt(-1) != formatVersion
        || !root.value(QStringLiteral("drawingTools")).isObject()
        || !root.value(QStringLiteral("motion")).isObject())
    {
        setError(
            error, QStringLiteral("The file is not a supported WWP preset."));
        return std::nullopt;
    }

    WwpPreset preset;
    const QSet<QString> allowed = allowedKeys();
    const QJsonObject tools =
        root.value(QStringLiteral("drawingTools")).toObject();
    for (auto iterator = tools.constBegin(); iterator != tools.constEnd();
        ++iterator)
    {
        if (!allowed.contains(iterator.key())
            || !validToolValue(iterator.key(), iterator.value()))
        {
            setError(error,
                QStringLiteral(
                    "The WWP preset contains invalid tool settings."));
            return std::nullopt;
        }
        preset.drawingTools.insert(
            iterator.key(), iterator.value().toVariant());
    }

    const QJsonObject motion = root.value(QStringLiteral("motion")).toObject();
    const std::optional<MotionStyle> style =
        motionStyleFromId(motion.value(QStringLiteral("style")).toString());
    if (!style || !finiteNumber(motion.value(QStringLiteral("amount")))
        || !integerNumber(motion.value(QStringLiteral("poseCount")))
        || !integerNumber(motion.value(QStringLiteral("detail")))
        || !finiteNumber(motion.value(QStringLiteral("linked")))
        || !finiteNumber(motion.value(QStringLiteral("randomness")))
        || !motion.value(QStringLiteral("brokenLine")).isBool()
        || !finiteNumber(motion.value(QStringLiteral("breakAmount")))
        || !finiteNumber(motion.value(QStringLiteral("breakRange"))))
    {
        setError(error,
            QStringLiteral("The WWP preset contains invalid motion settings."));
        return std::nullopt;
    }
    preset.wobbleAmount = motion.value(QStringLiteral("amount")).toDouble();
    preset.motion.style = *style;
    preset.motion.poseCount = motion.value(QStringLiteral("poseCount")).toInt();
    preset.motion.detail = motion.value(QStringLiteral("detail")).toInt();
    preset.motion.linked = motion.value(QStringLiteral("linked")).toDouble();
    preset.motion.randomness =
        motion.value(QStringLiteral("randomness")).toDouble();
    preset.motion.brokenLine =
        motion.value(QStringLiteral("brokenLine")).toBool();
    preset.motion.breakAmount =
        motion.value(QStringLiteral("breakAmount")).toDouble();
    preset.motion.breakRange =
        motion.value(QStringLiteral("breakRange")).toDouble();
    if (!validMotion(preset))
    {
        setError(error,
            QStringLiteral("The WWP preset motion settings are out of range."));
        return std::nullopt;
    }
    return preset;
}

void WwpPresetCodec::applyDrawingTools(
    const WwpPreset &preset, QSettings &settings)
{
    settings.beginGroup(QStringLiteral("drawingTools"));
    for (auto iterator = preset.drawingTools.cbegin();
        iterator != preset.drawingTools.cend();
        ++iterator)
    {
        settings.setValue(iterator.key(), iterator.value());
    }
    settings.endGroup();
    settings.sync();
}

}

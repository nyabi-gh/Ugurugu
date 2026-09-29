// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#pragma once

#include "document/Document.hpp"
#include "io/serializer/MaskAssetTable.hpp"
#include "io/serializer/SerializerSchema.hpp"

#include <QHash>
#include <QJsonArray>
#include <QJsonObject>
#include <QJsonValue>
#include <QSize>
#include <QString>
#include <QTransform>

#include <optional>

namespace ugurugu
{
namespace serializer_detail
{

// The on-disk JSON representation, in both directions.
//
// Every *FromJson reader is total: it returns nullopt for input it cannot
// accept instead of producing a partly filled value, so a caller may treat a
// non-empty result as fully validated against the schema it was given.
// Readers take `fileSchemaVersion` because older documents are still
// accepted; writers always emit the current `schemaVersion`.
//
// Mask payloads are not inlined per stroke. Writers resolve masks through the
// asset tables and store only ids; readers take the already-parsed
// `referencedMasks` / `referencedBinaryMasks` tables and reject an id that is
// not in them.

QJsonArray pointToJson(const StrokePoint &point);

std::optional<StrokePoint> pointFromJson(const QJsonValue &value);

QJsonObject brushToJson(const BrushSettings &brush);

std::optional<BrushSettings> brushFromJson(
    const QJsonValue &value, QString *error);

QString samplingModeName(SamplingMode sampling);

QString layerBlendModeName(LayerBlendMode mode);

QString layerKindName(LayerKind kind);

QJsonObject motionSettingsToJson(const MotionSettings &settings);

std::optional<MotionSettings> motionSettingsFromJson(
    const QJsonValue &value, QString *error);

QJsonArray transformToJson(const QTransform &transform);

QJsonObject strokeToJson(const Stroke &stroke,
    const ClipMaskTable &clipMasks,
    const BinaryMaskTable &binaryMasks);

// Schema versions before the shared mask tables stored each mask inline.
std::optional<QImage> legacyClipMaskFromJson(const QJsonValue &value,
    QHash<QByteArray, QImage> &maskCache,
    quint64 &distinctMaskBytes,
    QString *error);

bool isValidMaskContentId(const QString &id);

std::optional<QHash<QString, QImage>> clipMaskTableFromJson(
    const QJsonValue &value,
    const QSize &canvasSize,
    bool requireCanvasSize,
    quint64 &remainingAssetBytes,
    QString *error);

std::optional<QHash<QString, PackedMaskRegion>> binaryMaskTableFromJson(
    const QJsonValue &value, quint64 &remainingAssetBytes, QString *error);

std::optional<SamplingMode> samplingModeFromJson(const QJsonValue &value);

std::optional<LayerBlendMode> layerBlendModeFromJson(const QJsonValue &value);

std::optional<LayerKind> layerKindFromJson(const QJsonValue &value);

std::optional<QSize> sizeFromJsonArray(const QJsonValue &value);

std::optional<QTransform> transformFromJson(const QJsonValue &value);

std::optional<Stroke> strokeFromJson(const QJsonValue &value,
    int fileSchemaVersion,
    const QSize &canvasSize,
    QHash<QByteArray, QImage> &maskCache,
    const QHash<QString, QImage> &referencedMasks,
    const QHash<QString, PackedMaskRegion> &referencedBinaryMasks,
    quint64 &distinctMaskBytes,
    QString *error);

std::optional<Layer> layerFromJson(const QJsonValue &value,
    int fileSchemaVersion,
    const QSize &canvasSize,
    QHash<QByteArray, QImage> &maskCache,
    const QHash<QString, QImage> &referencedMasks,
    const QHash<QString, PackedMaskRegion> &referencedBinaryMasks,
    quint64 &distinctMaskBytes,
    QString *error);

QJsonObject layerToJson(const Layer &layer,
    const ClipMaskTable &clipMasks,
    const BinaryMaskTable &binaryMasks);

// The layer without its strokes, for size planning against the byte budget.
QJsonObject layerSkeletonToJson(const Layer &layer);

QJsonObject rootToJson(const Document &document,
    const QJsonArray &layers,
    const QJsonArray &clipMasks,
    const QJsonArray &binaryMasks,
    const QJsonObject &additionalRootFields = {});

// Rejects any JSON number that is not exactly representable as an int,
// including one written with a fractional part.
std::optional<int> integerFromJson(const QJsonValue &value);

}

}

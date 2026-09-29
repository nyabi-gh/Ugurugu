// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#pragma once

#include "document/Document.hpp"

#include <QHash>
#include <QMap>
#include <QString>
#include <QUuid>
#include <QVector>

namespace ugurugu
{
namespace serializer_detail
{

// Written into every saved document and checked on load. Raising either one
// changes what older builds accept, so they may only move together with the
// corresponding reader change.
constexpr int schemaVersion = 13;
constexpr int algorithmVersion = 3;

// Namespaces the process-local compressed payload cache keys. It is not
// written to disk; bump it whenever the payload encoding changes so entries
// cached under the previous encoding can never be served to the new one.
constexpr int serializationFormatGeneration = 1;

struct ClipAssetMeta
{
    QString id;
    QImage image;
    qint64 serializedEntryBytes = 0;
    qsizetype compressedBytes = 0;
};

struct BinaryAssetMeta
{
    QString id;
    PackedMaskRegion region;
    qint64 serializedEntryBytes = 0;
    qsizetype compressedBytes = 0;
};

struct StrokeMeta
{
    Stroke snapshot;
    QString clipMaskId;
    QString fillMaskId;
    QString binaryMaskId;
    qint64 serializedBytes = 0;
};

struct PreparedPlan
{
    // Identity aliases are meaningful only while the immutable base
    // PreparedDocument keeps the corresponding Qt COW backing alive.
    QMap<QString, ClipAssetMeta> clipAssets;
    QMap<QString, BinaryAssetMeta> binaryAssets;
    QMap<QString, RasterAsset> rasterAssets;
    QHash<qint64, QString> clipIdsByIdentity;
    QHash<QString, QString> binaryIdsByIdentity;
    QHash<QUuid, StrokeMeta> strokes;
    qint64 compactSize = 0;
    qsizetype totalStrokeCount = 0;
    qsizetype totalPointCount = 0;
    quint64 distinctMaskBytes = 0;
};

struct ImmutableBackings
{
    QHash<qint64, QImage> images;
    QHash<QString, QByteArray> byteArrays;
    QHash<QString, QVector<StrokePoint>> pointVectors;
};

inline void setError(QString *error, const QString &message)
{
    if (error)
    {
        *error = message;
    }
}

}

}

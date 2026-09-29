// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#pragma once

#include "document/Document.hpp"
#include "document/DocumentLimits.hpp"

#include <QByteArray>
#include <QImage>
#include <QMap>
#include <QSize>
#include <QString>

#include <optional>

namespace ugurugu::serializer_detail
{

enum class RasterAssetRegistrationStatus
{
    Registered,
    Reused,
    Invalid,
    PixelLimit,
    DecodedBudget,
    PayloadBudget
};

struct RasterAssetRegistrationResult
{
    RasterAssetRegistrationStatus status =
        RasterAssetRegistrationStatus::Invalid;
    QString id;
};

struct RasterAssetEntry
{
    QString id;
    QSize size;
    QByteArray compressedRgba;
    quint64 decodedBytes = 0;
};

std::optional<quint64> rasterDecodedByteCount(const QSize &size);
QImage canonicalRasterImage(const QImage &source);
QString rasterContentId(const QImage &canonical);
std::optional<RasterAsset> rasterAssetFromImage(
    const QImage &source, RasterAssetRegistrationStatus *status = nullptr);
std::optional<QImage> decodeRasterAsset(const RasterAsset &asset);

class RasterAssetTable final
{
public:
    explicit RasterAssetTable(
        quint64 maximumDecodedBytes =
            DocumentLimits::maximumDistinctRasterDecodedBytes,
        qint64 maximumPayloadBytes =
            DocumentLimits::maximumDistinctRasterPayloadBytes);

    RasterAssetRegistrationResult registerImage(const QImage &source);
    RasterAssetRegistrationResult registerPayload(const QString &expectedId,
        const QSize &size,
        const QByteArray &compressedRgba);
    std::optional<QImage> decode(const QString &id) const;

    const QMap<QString, RasterAssetEntry> &entries() const;
    quint64 decodedBytes() const;
    qint64 payloadBytes() const;

private:
    RasterAssetRegistrationResult registerCanonical(
        const QImage &canonical, const QString &id, QByteArray compressedRgba);

    QMap<QString, RasterAssetEntry> m_entries;
    quint64 m_decodedBytes = 0;
    qint64 m_payloadBytes = 0;
    quint64 m_maximumDecodedBytes = 0;
    qint64 m_maximumPayloadBytes = 0;
};

}

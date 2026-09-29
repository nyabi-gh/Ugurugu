// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "io/serializer/RasterAssetTable.hpp"

#include "io/serializer/BoundedCompression.hpp"

#include <QColorSpace>
#include <QCryptographicHash>

#include <algorithm>
#include <cstring>
#include <limits>

namespace ugurugu::serializer_detail
{

namespace
{

constexpr int bytesPerPixel = 4;
constexpr int compressionLevel = 6;

QByteArray canonicalRasterBytes(const QImage &canonical)
{
    if (canonical.isNull() || canonical.format() != QImage::Format_RGBA8888)
    {
        return {};
    }
    const std::optional<quint64> byteCount =
        rasterDecodedByteCount(canonical.size());
    if (!byteCount
        || *byteCount > static_cast<quint64>(std::numeric_limits<int>::max()))
    {
        return {};
    }
    QByteArray bytes(static_cast<qsizetype>(*byteCount), '\0');
    const qsizetype rowBytes =
        static_cast<qsizetype>(canonical.width()) * bytesPerPixel;
    for (int y = 0; y < canonical.height(); ++y)
    {
        std::memcpy(bytes.data() + static_cast<qsizetype>(y) * rowBytes,
            canonical.constScanLine(y),
            static_cast<std::size_t>(rowBytes));
    }
    return bytes;
}

std::optional<QByteArray> uncompressRaster(
    const QSize &size, const QByteArray &compressed)
{
    const std::optional<quint64> expectedBytes = rasterDecodedByteCount(size);
    if (!expectedBytes || compressed.size() < 4
        || *expectedBytes > std::numeric_limits<quint32>::max())
    {
        return std::nullopt;
    }
    return uncompressQtPayload(compressed, *expectedBytes);
}

QImage imageFromCanonicalBytes(const QSize &size, const QByteArray &bytes)
{
    const int rowBytes = size.width() * bytesPerPixel;
    const QImage view(reinterpret_cast<const uchar *>(bytes.constData()),
        size.width(),
        size.height(),
        rowBytes,
        QImage::Format_RGBA8888);
    QImage image = view.copy();
    image.setColorSpace(QColorSpace::SRgb);
    return image;
}

}

std::optional<quint64> rasterDecodedByteCount(const QSize &size)
{
    if (size.width() <= 0 || size.height() <= 0)
    {
        return std::nullopt;
    }
    const quint64 width = static_cast<quint64>(size.width());
    const quint64 height = static_cast<quint64>(size.height());
    if (width > DocumentLimits::maximumRasterAssetPixels / height)
    {
        return std::nullopt;
    }
    const quint64 pixels = width * height;
    if (pixels > DocumentLimits::maximumRasterAssetPixels
        || pixels > std::numeric_limits<quint64>::max() / bytesPerPixel)
    {
        return std::nullopt;
    }
    return pixels * bytesPerPixel;
}

QImage canonicalRasterImage(const QImage &source)
{
    if (source.isNull() || !rasterDecodedByteCount(source.size()))
    {
        return {};
    }
    QImage canonical;
    if (source.colorSpace().isValid()
        && source.colorSpace() != QColorSpace(QColorSpace::SRgb))
    {
        canonical = source.convertedToColorSpace(
            QColorSpace::SRgb, QImage::Format_RGBA8888);
    }
    else
    {
        canonical = source.convertToFormat(QImage::Format_RGBA8888);
        canonical.setColorSpace(QColorSpace::SRgb);
    }
    return canonical;
}

QString rasterContentId(const QImage &canonical)
{
    if (canonical.isNull() || canonical.format() != QImage::Format_RGBA8888
        || !rasterDecodedByteCount(canonical.size()))
    {
        return {};
    }
    QCryptographicHash hash(QCryptographicHash::Sha256);
    // This tag, dimension framing and tightly packed top-to-bottom straight
    // RGBA byte order define prototype identity. Schema 11 must preserve this
    // framing or publish a new version tag.
    hash.addData(QByteArrayLiteral("WWP_RASTER_RGBA8_V1\n"));
    hash.addData(QByteArray::number(canonical.width()));
    hash.addData(QByteArrayLiteral("x"));
    hash.addData(QByteArray::number(canonical.height()));
    hash.addData(QByteArrayLiteral("\n"));
    const qsizetype rowBytes =
        static_cast<qsizetype>(canonical.width()) * bytesPerPixel;
    for (int y = 0; y < canonical.height(); ++y)
    {
        hash.addData(QByteArrayView(
            reinterpret_cast<const char *>(canonical.constScanLine(y)),
            rowBytes));
    }
    return QString::fromLatin1(hash.result().toHex());
}

std::optional<RasterAsset> rasterAssetFromImage(
    const QImage &source, RasterAssetRegistrationStatus *status)
{
    RasterAssetTable table;
    const RasterAssetRegistrationResult result = table.registerImage(source);
    if (status)
    {
        *status = result.status;
    }
    if (result.status != RasterAssetRegistrationStatus::Registered
        && result.status != RasterAssetRegistrationStatus::Reused)
    {
        return std::nullopt;
    }
    const RasterAssetEntry &entry = table.entries().value(result.id);
    return RasterAsset{entry.id, entry.size, entry.compressedRgba};
}

std::optional<QImage> decodeRasterAsset(const RasterAsset &asset)
{
    RasterAssetTable table;
    const RasterAssetRegistrationResult result =
        table.registerPayload(asset.id, asset.size, asset.compressedRgba);
    if (result.status != RasterAssetRegistrationStatus::Registered
        && result.status != RasterAssetRegistrationStatus::Reused)
    {
        return std::nullopt;
    }
    return table.decode(asset.id);
}

RasterAssetTable::RasterAssetTable(
    quint64 maximumDecodedBytes, qint64 maximumPayloadBytes)
    : m_maximumDecodedBytes(std::min(maximumDecodedBytes,
          DocumentLimits::maximumDistinctRasterDecodedBytes))
    , m_maximumPayloadBytes(std::clamp(maximumPayloadBytes,
          0LL,
          DocumentLimits::maximumDistinctRasterPayloadBytes))
{
}

RasterAssetRegistrationResult RasterAssetTable::registerImage(
    const QImage &source)
{
    if (source.isNull())
    {
        return {RasterAssetRegistrationStatus::Invalid, {}};
    }
    const std::optional<quint64> decoded =
        rasterDecodedByteCount(source.size());
    if (!decoded)
    {
        return {RasterAssetRegistrationStatus::PixelLimit, {}};
    }
    const QImage canonical = canonicalRasterImage(source);
    if (canonical.isNull())
    {
        return {RasterAssetRegistrationStatus::Invalid, {}};
    }
    const QString id = rasterContentId(canonical);
    if (id.isEmpty())
    {
        return {RasterAssetRegistrationStatus::Invalid, {}};
    }
    if (m_entries.contains(id))
    {
        return {RasterAssetRegistrationStatus::Reused, id};
    }
    if (*decoded > m_maximumDecodedBytes
        || m_decodedBytes > m_maximumDecodedBytes - *decoded)
    {
        return {RasterAssetRegistrationStatus::DecodedBudget, {}};
    }
    const QByteArray bytes = canonicalRasterBytes(canonical);
    const QByteArray compressed = qCompress(bytes, compressionLevel);
    if (compressed.isEmpty())
    {
        return {RasterAssetRegistrationStatus::Invalid, {}};
    }
    return registerCanonical(canonical, id, compressed);
}

RasterAssetRegistrationResult RasterAssetTable::registerPayload(
    const QString &expectedId,
    const QSize &size,
    const QByteArray &compressedRgba)
{
    if (expectedId.isEmpty())
    {
        return {RasterAssetRegistrationStatus::Invalid, {}};
    }
    const std::optional<quint64> decoded = rasterDecodedByteCount(size);
    if (!decoded)
    {
        return {RasterAssetRegistrationStatus::PixelLimit, {}};
    }
    const auto existing = m_entries.constFind(expectedId);
    if (existing != m_entries.cend())
    {
        if (existing->size != size
            || existing->compressedRgba != compressedRgba)
        {
            return {RasterAssetRegistrationStatus::Invalid, {}};
        }
        return {RasterAssetRegistrationStatus::Reused, expectedId};
    }
    if (*decoded > m_maximumDecodedBytes
        || m_decodedBytes > m_maximumDecodedBytes - *decoded)
    {
        return {RasterAssetRegistrationStatus::DecodedBudget, {}};
    }
    if (compressedRgba.isEmpty()
        || compressedRgba.size() > m_maximumPayloadBytes
        || m_payloadBytes > m_maximumPayloadBytes - compressedRgba.size())
    {
        return {RasterAssetRegistrationStatus::PayloadBudget, {}};
    }
    const std::optional<QByteArray> bytes =
        uncompressRaster(size, compressedRgba);
    if (!bytes)
    {
        return {RasterAssetRegistrationStatus::Invalid, {}};
    }
    const QImage canonical = imageFromCanonicalBytes(size, *bytes);
    if (canonical.isNull() || rasterContentId(canonical) != expectedId)
    {
        return {RasterAssetRegistrationStatus::Invalid, {}};
    }
    return registerCanonical(canonical, expectedId, compressedRgba);
}

std::optional<QImage> RasterAssetTable::decode(const QString &id) const
{
    const auto entry = m_entries.constFind(id);
    if (entry == m_entries.cend())
    {
        return std::nullopt;
    }
    const std::optional<QByteArray> bytes =
        uncompressRaster(entry->size, entry->compressedRgba);
    if (!bytes)
    {
        return std::nullopt;
    }
    QImage image = imageFromCanonicalBytes(entry->size, *bytes);
    if (image.isNull() || rasterContentId(image) != id)
    {
        return std::nullopt;
    }
    return image;
}

const QMap<QString, RasterAssetEntry> &RasterAssetTable::entries() const
{
    return m_entries;
}

quint64 RasterAssetTable::decodedBytes() const
{
    return m_decodedBytes;
}

qint64 RasterAssetTable::payloadBytes() const
{
    return m_payloadBytes;
}

RasterAssetRegistrationResult RasterAssetTable::registerCanonical(
    const QImage &canonical, const QString &id, QByteArray compressedRgba)
{
    const std::optional<quint64> decoded =
        rasterDecodedByteCount(canonical.size());
    if (!decoded)
    {
        return {RasterAssetRegistrationStatus::PixelLimit, {}};
    }
    if (*decoded > m_maximumDecodedBytes
        || m_decodedBytes > m_maximumDecodedBytes - *decoded)
    {
        return {RasterAssetRegistrationStatus::DecodedBudget, {}};
    }
    if (compressedRgba.isEmpty()
        || compressedRgba.size() > m_maximumPayloadBytes
        || m_payloadBytes > m_maximumPayloadBytes - compressedRgba.size())
    {
        return {RasterAssetRegistrationStatus::PayloadBudget, {}};
    }
    RasterAssetEntry entry;
    entry.id = id;
    entry.size = canonical.size();
    entry.compressedRgba = std::move(compressedRgba);
    entry.decodedBytes = *decoded;
    m_decodedBytes += entry.decodedBytes;
    m_payloadBytes += entry.compressedRgba.size();
    m_entries.insert(id, entry);
    return {RasterAssetRegistrationStatus::Registered, id};
}

}

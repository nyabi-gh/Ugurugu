// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "io/serializer/MaskAssetTable.hpp"

#include <QCryptographicHash>
#include <QJsonArray>

#include <cstring>
#include <limits>

namespace ugurugu
{
namespace serializer_detail
{

QByteArray canonicalMaskBytes(const QImage &mask)
{
    if (mask.isNull() || mask.format() != QImage::Format_Grayscale8)
    {
        return {};
    }
    const qint64 byteCount = static_cast<qint64>(mask.width()) * mask.height();
    if (byteCount <= 0 || byteCount > std::numeric_limits<int>::max())
    {
        return {};
    }
    QByteArray bytes(static_cast<qsizetype>(byteCount), '\0');
    for (int y = 0; y < mask.height(); ++y)
    {
        std::memcpy(bytes.data() + static_cast<qsizetype>(y) * mask.width(),
            mask.constScanLine(y),
            static_cast<std::size_t>(mask.width()));
    }
    return bytes;
}

QString maskContentId(int width, int height, const QByteArray &canonicalBytes)
{
    QCryptographicHash hash(QCryptographicHash::Sha256);
    hash.addData(QByteArray::number(width));
    hash.addData(QByteArrayLiteral("x"));
    hash.addData(QByteArray::number(height));
    hash.addData(QByteArrayLiteral(":"));
    hash.addData(canonicalBytes);
    return QString::fromLatin1(hash.result().toHex());
}

qint64 base64Size(qsizetype byteCount)
{
    if (byteCount < 0)
    {
        return -1;
    }
    return 4LL * ((static_cast<qint64>(byteCount) + 2LL) / 3LL);
}

QString payloadCacheKey(bool binary, const QString &contentId)
{
    return QStringLiteral("%1:%2:qcompress-6:%3")
        .arg(serializationFormatGeneration)
        .arg(binary ? QStringLiteral("binary") : QStringLiteral("clip"))
        .arg(contentId);
}

QJsonObject serializedClipMaskToJson(const SerializedClipMask &entry)
{
    QJsonObject object;
    object.insert(QStringLiteral("id"), entry.id);
    object.insert(QStringLiteral("width"), entry.image.width());
    object.insert(QStringLiteral("height"), entry.image.height());
    object.insert(QStringLiteral("data"),
        QString::fromLatin1(entry.compressedData.toBase64()));
    return object;
}

qint64 serializedClipMaskSize(
    const QString &id, const QImage &image, qsizetype compressedBytes)
{
    SerializedClipMask skeleton;
    skeleton.id = id;
    skeleton.image = image;
    const qint64 skeletonBytes =
        QJsonDocument(serializedClipMaskToJson(skeleton))
            .toJson(QJsonDocument::Compact)
            .size();
    const qint64 encodedBytes = base64Size(compressedBytes);
    return encodedBytes < 0 ? -1 : skeletonBytes + encodedBytes;
}

QHash<qint64, ClipAssetMeta> clipAssetsByIdentity(const PreparedPlan *base)
{
    QHash<qint64, ClipAssetMeta> result;
    if (!base)
    {
        return result;
    }
    for (auto identity = base->clipIdsByIdentity.cbegin();
        identity != base->clipIdsByIdentity.cend();
        ++identity)
    {
        const auto asset = base->clipAssets.constFind(identity.value());
        if (asset != base->clipAssets.cend())
        {
            result.insert(identity.key(), asset.value());
        }
    }
    return result;
}

QString binaryMaskContentId(const PackedMaskRegion &region)
{
    QCryptographicHash hash(QCryptographicHash::Sha256);
    hash.addData(QByteArrayLiteral("binary-mask:"));
    for (const int value : {region.canvasSize.width(),
             region.canvasSize.height(),
             region.bounds.x(),
             region.bounds.y(),
             region.bounds.width(),
             region.bounds.height()})
    {
        hash.addData(QByteArray::number(value));
        hash.addData(QByteArrayLiteral(":"));
    }
    hash.addData(region.packedMask);
    return QString::fromLatin1(hash.result().toHex());
}

QJsonObject serializedBinaryMaskToJson(const SerializedBinaryMask &entry)
{
    const PackedMaskRegion &region = entry.region;
    QJsonObject object;
    object.insert(QStringLiteral("id"), entry.id);
    object.insert(QStringLiteral("canvas"),
        QJsonArray{region.canvasSize.width(), region.canvasSize.height()});
    object.insert(QStringLiteral("bounds"),
        QJsonArray{region.bounds.x(),
            region.bounds.y(),
            region.bounds.width(),
            region.bounds.height()});
    object.insert(QStringLiteral("data"),
        QString::fromLatin1(entry.compressedData.toBase64()));
    return object;
}

qint64 serializedBinaryMaskSize(const QString &id,
    const PackedMaskRegion &region,
    qsizetype compressedBytes)
{
    SerializedBinaryMask skeleton;
    skeleton.id = id;
    skeleton.region = region;
    const qint64 skeletonBytes =
        QJsonDocument(serializedBinaryMaskToJson(skeleton))
            .toJson(QJsonDocument::Compact)
            .size();
    const qint64 encodedBytes = base64Size(compressedBytes);
    return encodedBytes < 0 ? -1 : skeletonBytes + encodedBytes;
}

QString binaryMaskIdentity(const PackedMaskRegion &region)
{
    return QStringLiteral("%1:%2:%3:%4:%5:%6:%7:%8")
        .arg(reinterpret_cast<quintptr>(region.packedMask.constData()), 0, 16)
        .arg(region.packedMask.size())
        .arg(region.canvasSize.width())
        .arg(region.canvasSize.height())
        .arg(region.bounds.x())
        .arg(region.bounds.y())
        .arg(region.bounds.width())
        .arg(region.bounds.height());
}

QHash<QString, BinaryAssetMeta> binaryAssetsByIdentity(const PreparedPlan *base)
{
    QHash<QString, BinaryAssetMeta> result;
    if (!base)
    {
        return result;
    }
    for (auto identity = base->binaryIdsByIdentity.cbegin();
        identity != base->binaryIdsByIdentity.cend();
        ++identity)
    {
        const auto asset = base->binaryAssets.constFind(identity.value());
        if (asset != base->binaryAssets.cend())
        {
            result.insert(identity.key(), asset.value());
        }
    }
    return result;
}

PackedMaskRegion pixelSelectionMaskRegion(const PixelSelectionOp &operation)
{
    return PackedMaskRegion{
        operation.canvasSize, operation.sourceBounds, operation.packedMask};
}

std::optional<PackedMaskRegion> strokeBinaryMaskRegion(const Stroke &stroke)
{
    if (stroke.pixelSelectionOp)
    {
        return pixelSelectionMaskRegion(*stroke.pixelSelectionOp);
    }
    return stroke.fillCoverage;
}

}

}

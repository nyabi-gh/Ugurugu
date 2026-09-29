// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#pragma once

#include "document/Document.hpp"
#include "document/SelectionOperation.hpp"
#include "io/serializer/SerializerSchema.hpp"

#include <QByteArray>
#include <QHash>
#include <QImage>
#include <QJsonObject>
#include <QMap>
#include <QString>

namespace ugurugu
{
namespace serializer_detail
{

// Deduplicated mask payload store shared by both serialization directions.
//
// Clip masks are keyed by pixel content and selection masks by packed region
// content, so two strokes carrying identical masks resolve to one written
// asset. `tooLarge` and `invalid` mark a table the caller must not serialize:
// building stops early, so a flagged table is incomplete rather than merely
// oversized.

struct SerializedClipMask
{
    QString id;
    QImage image;
    QByteArray compressedData;
    qsizetype compressedBytes = 0;
    qint64 serializedBytes = 0;
};

struct ClipMaskTable
{
    QHash<qint64, QString> idByCacheKey;
    QMap<QString, SerializedClipMask> entries;
    qint64 serializedEntryBytes = 0;
    bool tooLarge = false;
    bool invalid = false;
};

struct SerializedBinaryMask
{
    QString id;
    PackedMaskRegion region;
    QByteArray compressedData;
    qsizetype compressedBytes = 0;
    qint64 serializedBytes = 0;
};

struct BinaryMaskTable
{
    QHash<QString, QString> idByIdentity;
    QMap<QString, SerializedBinaryMask> entries;
    qint64 serializedEntryBytes = 0;
    bool tooLarge = false;
    bool invalid = false;
};

QByteArray canonicalMaskBytes(const QImage &mask);

QString maskContentId(int width, int height, const QByteArray &canonicalBytes);

qint64 base64Size(qsizetype byteCount);

QString payloadCacheKey(bool binary, const QString &contentId);

QJsonObject serializedClipMaskToJson(const SerializedClipMask &entry);

qint64 serializedClipMaskSize(
    const QString &id, const QImage &image, qsizetype compressedBytes);

QHash<qint64, ClipAssetMeta> clipAssetsByIdentity(const PreparedPlan *base);

QString binaryMaskContentId(const PackedMaskRegion &region);

QJsonObject serializedBinaryMaskToJson(const SerializedBinaryMask &entry);

qint64 serializedBinaryMaskSize(const QString &id,
    const PackedMaskRegion &region,
    qsizetype compressedBytes);

QString binaryMaskIdentity(const PackedMaskRegion &region);

QHash<QString, BinaryAssetMeta> binaryAssetsByIdentity(
    const PreparedPlan *base);

PackedMaskRegion pixelSelectionMaskRegion(const PixelSelectionOp &operation);

std::optional<PackedMaskRegion> strokeBinaryMaskRegion(const Stroke &stroke);

// Templated on the cache so this header does not need the private
// SerializationCache::Impl definition. `Cache` must provide payload(),
// storePayload() and a `statistics` member.

template <typename Cache>
QByteArray compressedPayload(Cache &cache,
    bool binary,
    const QString &contentId,
    const QByteArray &source)
{
    const QString key = payloadCacheKey(binary, contentId);
    QByteArray compressed = cache.payload(key);
    if (!compressed.isEmpty())
    {
        return compressed;
    }
    if (binary)
    {
        ++cache.statistics.binaryMaskCompressions;
    }
    else
    {
        ++cache.statistics.clipMaskCompressions;
    }
    compressed = qCompress(source, 6);
    cache.storePayload(key, compressed);
    return compressed;
}

template <typename Cache>
QString registerClipMask(const QImage &mask,
    ClipMaskTable &table,
    const QHash<qint64, ClipAssetMeta> &baseAssets,
    Cache &cache,
    qint64 maximumBytes,
    bool requirePayload)
{
    if (mask.isNull())
    {
        return {};
    }
    const qint64 cacheKey = mask.cacheKey();
    const auto cachedId = table.idByCacheKey.constFind(cacheKey);
    if (cachedId != table.idByCacheKey.cend())
    {
        return cachedId.value();
    }

    const auto baseAsset = baseAssets.constFind(cacheKey);
    if (baseAsset != baseAssets.cend() && baseAsset->image.size() == mask.size()
        && baseAsset->image.format() == mask.format())
    {
        const auto existing = table.entries.constFind(baseAsset->id);
        if (existing != table.entries.cend())
        {
            table.idByCacheKey.insert(cacheKey, baseAsset->id);
            return baseAsset->id;
        }
        SerializedClipMask entry;
        entry.id = baseAsset->id;
        entry.image = mask;
        entry.serializedBytes = baseAsset->serializedEntryBytes;
        entry.compressedBytes = baseAsset->compressedBytes;
        if (requirePayload)
        {
            const QString key = payloadCacheKey(false, entry.id);
            entry.compressedData = cache.payload(key);
            if (entry.compressedData.isEmpty())
            {
                const QByteArray bytes = canonicalMaskBytes(mask);
                if (bytes.isEmpty())
                {
                    table.invalid = true;
                    return {};
                }
                ++cache.statistics.clipMaskCompressions;
                entry.compressedData = qCompress(bytes, 6);
                cache.storePayload(key, entry.compressedData);
            }
            if (entry.compressedData.size() != baseAsset->compressedBytes
                || serializedClipMaskSize(
                       entry.id, entry.image, entry.compressedData.size())
                       != entry.serializedBytes)
            {
                table.invalid = true;
                return {};
            }
        }
        if (entry.serializedBytes < 0 || entry.serializedBytes > maximumBytes
            || table.serializedEntryBytes
                   > maximumBytes - entry.serializedBytes)
        {
            table.tooLarge = true;
            return {};
        }
        table.serializedEntryBytes += entry.serializedBytes;
        table.idByCacheKey.insert(cacheKey, entry.id);
        table.entries.insert(entry.id, entry);
        return baseAsset->id;
    }

    const QByteArray bytes = canonicalMaskBytes(mask);
    if (bytes.isEmpty())
    {
        table.invalid = true;
        return {};
    }
    ++cache.statistics.clipMaskContentHashes;
    QString id = maskContentId(mask.width(), mask.height(), bytes);
    const auto existing = table.entries.constFind(id);
    if (existing != table.entries.cend())
    {
        if (existing->image.size() != mask.size() || existing->image != mask)
        {
            table.invalid = true;
            return {};
        }
        table.idByCacheKey.insert(cacheKey, id);
        return id;
    }

    const QByteArray compressed = compressedPayload(cache, false, id, bytes);
    if (compressed.isEmpty())
    {
        table.invalid = true;
        return {};
    }
    SerializedClipMask entry;
    entry.id = id;
    entry.image = mask;
    entry.serializedBytes = serializedClipMaskSize(id, mask, compressed.size());
    entry.compressedBytes = compressed.size();
    if (requirePayload)
    {
        entry.compressedData = compressed;
    }
    const qint64 entryBytes = entry.serializedBytes;
    if (entryBytes > maximumBytes || entryBytes < 0
        || table.serializedEntryBytes > maximumBytes - entryBytes)
    {
        table.tooLarge = true;
        return {};
    }
    table.serializedEntryBytes += entryBytes;
    table.idByCacheKey.insert(cacheKey, id);
    table.entries.insert(id, entry);
    return id;
}

template <typename Cache>
ClipMaskTable buildClipMaskTable(const Document &document,
    const PreparedPlan *base,
    Cache &cache,
    qint64 maximumBytes,
    bool requirePayload)
{
    ClipMaskTable table;
    const QHash<qint64, ClipAssetMeta> baseAssets = clipAssetsByIdentity(base);
    for (const Layer &layer : document.layers)
    {
        for (const Stroke &stroke : layer.strokes)
        {
            if ((!stroke.clipMask.isNull()
                    && registerClipMask(stroke.clipMask,
                        table,
                        baseAssets,
                        cache,
                        maximumBytes,
                        requirePayload)
                        .isEmpty())
                || (!stroke.fillMask.isNull()
                    && registerClipMask(stroke.fillMask,
                        table,
                        baseAssets,
                        cache,
                        maximumBytes,
                        requirePayload)
                        .isEmpty()))
            {
                return table;
            }
        }
    }
    return table;
}

template <typename Cache>
QString registerBinaryMask(const PackedMaskRegion &region,
    BinaryMaskTable &table,
    const QHash<QString, BinaryAssetMeta> &baseAssets,
    Cache &cache,
    qint64 maximumBytes,
    bool requirePayload)
{
    if (!isValidPackedMaskRegion(region))
    {
        table.invalid = true;
        return {};
    }
    const QString identity = binaryMaskIdentity(region);
    const auto knownIdentity = table.idByIdentity.constFind(identity);
    if (knownIdentity != table.idByIdentity.cend())
    {
        return knownIdentity.value();
    }

    const auto baseAsset = baseAssets.constFind(identity);
    if (baseAsset != baseAssets.cend())
    {
        const auto existing = table.entries.constFind(baseAsset->id);
        if (existing != table.entries.cend())
        {
            table.idByIdentity.insert(identity, baseAsset->id);
            return baseAsset->id;
        }
        SerializedBinaryMask entry;
        entry.id = baseAsset->id;
        entry.region = region;
        entry.serializedBytes = baseAsset->serializedEntryBytes;
        entry.compressedBytes = baseAsset->compressedBytes;
        if (requirePayload)
        {
            entry.compressedData =
                compressedPayload(cache, true, entry.id, region.packedMask);
            if (entry.compressedData.size() != baseAsset->compressedBytes
                || serializedBinaryMaskSize(
                       entry.id, entry.region, entry.compressedData.size())
                       != entry.serializedBytes)
            {
                table.invalid = true;
                return {};
            }
        }
        if (entry.serializedBytes < 0 || entry.serializedBytes > maximumBytes
            || table.serializedEntryBytes
                   > maximumBytes - entry.serializedBytes)
        {
            table.tooLarge = true;
            return {};
        }
        table.serializedEntryBytes += entry.serializedBytes;
        table.idByIdentity.insert(identity, entry.id);
        table.entries.insert(entry.id, entry);
        return baseAsset->id;
    }

    ++cache.statistics.binaryMaskContentHashes;
    QString id = binaryMaskContentId(region);
    const auto existing = table.entries.constFind(id);
    if (existing != table.entries.cend())
    {
        if (existing->region != region)
        {
            table.invalid = true;
            return {};
        }
        table.idByIdentity.insert(identity, id);
        return id;
    }
    const QByteArray compressed =
        compressedPayload(cache, true, id, region.packedMask);
    if (compressed.isEmpty())
    {
        table.invalid = true;
        return {};
    }
    SerializedBinaryMask entry;
    entry.id = id;
    entry.region = region;
    entry.serializedBytes =
        serializedBinaryMaskSize(id, region, compressed.size());
    entry.compressedBytes = compressed.size();
    if (requirePayload)
    {
        entry.compressedData = compressed;
    }
    const qint64 entryBytes = entry.serializedBytes;
    if (entryBytes > maximumBytes || entryBytes < 0
        || table.serializedEntryBytes > maximumBytes - entryBytes)
    {
        table.tooLarge = true;
        return {};
    }
    table.serializedEntryBytes += entryBytes;
    table.idByIdentity.insert(identity, id);
    table.entries.insert(id, entry);
    return id;
}

template <typename Cache>
BinaryMaskTable buildBinaryMaskTable(const Document &document,
    const PreparedPlan *base,
    Cache &cache,
    qint64 maximumBytes,
    bool requirePayload)
{
    BinaryMaskTable table;
    const QHash<QString, BinaryAssetMeta> baseAssets =
        binaryAssetsByIdentity(base);
    for (const Layer &layer : document.layers)
    {
        for (const Stroke &stroke : layer.strokes)
        {
            const std::optional<PackedMaskRegion> region =
                strokeBinaryMaskRegion(stroke);
            if (region
                && registerBinaryMask(*region,
                    table,
                    baseAssets,
                    cache,
                    maximumBytes,
                    requirePayload)
                    .isEmpty())
            {
                return table;
            }
        }
    }
    return table;
}

}

}

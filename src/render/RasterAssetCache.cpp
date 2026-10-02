// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "render/RasterAssetCache.hpp"

#include "app/MemoryBudget.hpp"
#include "io/serializer/RasterAssetTable.hpp"
#include "render/ImageAffineTransformer.hpp"

#include <QCache>
#include <QMutex>
#include <QMutexLocker>
#include <QSet>
#include <QWaitCondition>

#include <algorithm>

namespace ugurugu
{

namespace
{

QMutex &cacheMutex()
{
    static QMutex mutex;
    return mutex;
}

QCache<QString, QImage> &imageCache()
{
    static QCache<QString, QImage> cache;
    static const bool configured = []()
    {
        cache.setMaxCost(
            static_cast<int>(MemoryBudget::rasterDecodeCacheBytes / 1024));
        return true;
    }();
    static_cast<void>(configured);
    return cache;
}

QString transformedCacheKey(const QString &assetId,
    const QSize &targetSize,
    const QTransform &transform,
    SamplingMode sampling)
{
    QString key = QStringLiteral("render:%1:%2x%3:%4")
                      .arg(assetId)
                      .arg(targetSize.width())
                      .arg(targetSize.height())
                      .arg(static_cast<int>(sampling));
    for (qreal value : {transform.m11(),
             transform.m12(),
             transform.m21(),
             transform.m22(),
             transform.dx(),
             transform.dy()})
    {
        key += QLatin1Char(':');
        key += QString::number(value, 'g', 17);
    }
    return key;
}

int cacheCost(const QImage &image)
{
    return static_cast<int>(
        std::max<qsizetype>(1, (image.sizeInBytes() + 1023) / 1024));
}

QSet<QString> &inFlightKeys()
{
    static QSet<QString> keys;
    return keys;
}

QWaitCondition &inFlightSettled()
{
    static QWaitCondition condition;
    return condition;
}

// Every frame worker meets the same image operations at the same moment, so
// a miss used to decode or transform one asset once per worker. The first
// thread to miss computes it; the others wait for its result.
template <typename Compute>
QImage cachedOrComputed(const QString &key, Compute compute)
{
    QMutexLocker locker(&cacheMutex());
    for (;;)
    {
        if (const QImage *cached = imageCache().object(key))
        {
            return *cached;
        }
        if (!inFlightKeys().contains(key))
        {
            break;
        }
        inFlightSettled().wait(&cacheMutex());
    }
    inFlightKeys().insert(key);
    locker.unlock();

    const QImage image = compute();

    locker.relock();
    if (!image.isNull())
    {
        imageCache().insert(key, new QImage(image), cacheCost(image));
    }
    inFlightKeys().remove(key);
    inFlightSettled().wakeAll();
    return image;
}

}

QImage RasterAssetCache::image(const Document &document, const QString &assetId)
{
    return cachedOrComputed(QStringLiteral("source:") + assetId,
        [&document, &assetId]() -> QImage
        {
            const auto asset = document.rasterAssets.constFind(assetId);
            if (asset == document.rasterAssets.cend())
            {
                return {};
            }
            const std::optional<QImage> canonical =
                serializer_detail::decodeRasterAsset(*asset);
            if (!canonical)
            {
                return {};
            }
            return canonical->convertToFormat(
                QImage::Format_ARGB32_Premultiplied);
        });
}

QImage RasterAssetCache::transformedImage(const Document &document,
    const QString &assetId,
    const QSize &targetSize,
    const QTransform &transform,
    SamplingMode sampling)
{
    const ImageOp operation{assetId, transform, sampling};
    if (!targetSize.isValid() || !isValidImageOp(operation))
    {
        return {};
    }
    return cachedOrComputed(
        transformedCacheKey(assetId, targetSize, transform, sampling),
        [&]() -> QImage
        {
            const QImage source = image(document, assetId);
            if (source.isNull())
            {
                return {};
            }
            QImage transformed(targetSize, QImage::Format_ARGB32_Premultiplied);
            if (transformed.isNull())
            {
                return {};
            }
            transformed.fill(Qt::transparent);
            if (!ImageAffineTransformer::compositeSourceOver(transformed,
                    QRect(QPoint(), targetSize),
                    source,
                    QRect(QPoint(), source.size()),
                    transform,
                    sampling))
            {
                return {};
            }
            return transformed;
        });
}

void RasterAssetCache::clear()
{
    const QMutexLocker locker(&cacheMutex());
    imageCache().clear();
}

}

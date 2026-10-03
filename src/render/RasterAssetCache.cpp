// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "render/RasterAssetCache.hpp"

#include "app/MemoryBudget.hpp"
#include "io/serializer/RasterAssetTable.hpp"
#include "render/ImageAffineTransformer.hpp"
#include "render/engine/ComputedImageCache.hpp"

namespace ugurugu
{

namespace
{

// Every frame worker meets the same image operations at the same moment, so
// one shared cache keeps a miss from decoding or transforming once per worker.
render_detail::ComputedImageCache &imageCache()
{
    static render_detail::ComputedImageCache cache(
        MemoryBudget::rasterDecodeCacheBytes);
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

}

QImage RasterAssetCache::image(const Document &document, const QString &assetId)
{
    return imageCache().imageOrCompute(QStringLiteral("source:") + assetId,
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
    return imageCache().imageOrCompute(
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
    imageCache().clear();
}

}

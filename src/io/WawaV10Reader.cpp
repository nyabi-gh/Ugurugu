// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "io/WawaV10Reader.hpp"

#include "document/DocumentLimits.hpp"
#include "io/serializer/RasterAssetTable.hpp"

#include <QBuffer>
#include <QHash>
#include <QImageReader>
#include <QStringDecoder>
#include <QtEndian>

#include <algorithm>
#include <bit>
#include <cmath>
#include <limits>

namespace ugurugu
{

namespace
{

constexpr int nativeVersion = 10;
constexpr int maximumNativeLayers = 10;
constexpr qsizetype maximumEncodedLayerNameBytes = 4096;

void setError(QString *error, const QString &message)
{
    if (error)
    {
        *error = message;
    }
}

// Native v10 follows .NET BinaryReader: little-endian primitives and UTF-8
// strings prefixed by a 7-bit encoded byte count.
class BinaryCursor final
{
public:
    explicit BinaryCursor(const QByteArray &data)
        : m_data(data)
    {
    }

    std::optional<quint8> readByte()
    {
        const std::optional<QByteArrayView> bytes = readBytes(1);
        return bytes ? std::optional<quint8>(static_cast<quint8>((*bytes)[0]))
                     : std::nullopt;
    }

    std::optional<bool> readBoolean()
    {
        const std::optional<quint8> value = readByte();
        if (!value || (*value != 0 && *value != 1))
        {
            return std::nullopt;
        }
        return *value != 0;
    }

    std::optional<qint32> readInt32()
    {
        const std::optional<QByteArrayView> bytes = readBytes(4);
        if (!bytes)
        {
            return std::nullopt;
        }
        return static_cast<qint32>(qFromLittleEndian<quint32>(
            reinterpret_cast<const uchar *>(bytes->data())));
    }

    std::optional<float> readSingle()
    {
        const std::optional<qint32> bits = readInt32();
        if (!bits)
        {
            return std::nullopt;
        }
        return std::bit_cast<float>(static_cast<quint32>(*bits));
    }

    std::optional<QString> readString(qsizetype maximumBytes)
    {
        quint32 length = 0;
        int shift = 0;
        for (int index = 0; index < 5; ++index)
        {
            const std::optional<quint8> byte = readByte();
            if (!byte)
            {
                return std::nullopt;
            }
            length |= static_cast<quint32>(*byte & 0x7fU) << shift;
            if ((*byte & 0x80U) == 0)
            {
                if (length > static_cast<quint32>(maximumBytes))
                {
                    return std::nullopt;
                }
                const std::optional<QByteArrayView> bytes = readBytes(length);
                if (!bytes)
                {
                    return std::nullopt;
                }
                QStringDecoder decoder(QStringDecoder::Utf8);
                const QString decoded = decoder.decode(*bytes);
                return decoder.hasError() ? std::nullopt
                                          : std::optional<QString>(decoded);
            }
            shift += 7;
        }
        return std::nullopt;
    }

    std::optional<QByteArrayView> readBytes(qsizetype length)
    {
        if (length < 0 || m_offset > m_data.size()
            || length > m_data.size() - m_offset)
        {
            return std::nullopt;
        }
        const QByteArrayView result(m_data.constData() + m_offset, length);
        m_offset += length;
        return result;
    }

    bool atEnd() const
    {
        return m_offset == m_data.size();
    }

private:
    QByteArrayView m_data;
    qsizetype m_offset = 0;
};

QColor colorFromArgb(qint32 value)
{
    const quint32 argb = static_cast<quint32>(value);
    return QColor(static_cast<int>((argb >> 16U) & 0xffU),
        static_cast<int>((argb >> 8U) & 0xffU),
        static_cast<int>(argb & 0xffU),
        static_cast<int>((argb >> 24U) & 0xffU));
}

bool validCanvas(const QSize &size)
{
    return size.width() >= DocumentLimits::minimumCanvasEdge
           && size.height() >= DocumentLimits::minimumCanvasEdge
           && size.width() <= DocumentLimits::maximumCanvasEdge
           && size.height() <= DocumentLimits::maximumCanvasEdge;
}

bool validPoint(const QPointF &point)
{
    return std::isfinite(point.x()) && std::isfinite(point.y())
           && std::abs(point.x())
                  <= DocumentLimits::maximumStoredCoordinateMagnitude
           && std::abs(point.y())
                  <= DocumentLimits::maximumStoredCoordinateMagnitude;
}

bool hasVisiblePixels(const QImage &image)
{
    for (int y = 0; y < image.height(); ++y)
    {
        for (int x = 0; x < image.width(); ++x)
        {
            if (qAlpha(image.pixel(x, y)) != 0)
            {
                return true;
            }
        }
    }
    return false;
}

std::optional<QImage> readLayerImage(BinaryCursor &cursor,
    const QSize &size,
    quint64 &remainingDecodedImageBytes,
    QHash<QString, QImage> &decodedImages,
    QImage &transparentImage)
{
    const std::optional<qint32> encodedLength = cursor.readInt32();
    if (!encodedLength || *encodedLength <= 0
        || *encodedLength > DocumentLimits::maximumProjectBytes)
    {
        return std::nullopt;
    }
    const std::optional<QByteArrayView> encoded =
        cursor.readBytes(*encodedLength);
    if (!encoded)
    {
        return std::nullopt;
    }
    const QByteArray bytes(encoded->data(), encoded->size());
    QBuffer buffer;
    buffer.setData(bytes);
    if (!buffer.open(QIODevice::ReadOnly))
    {
        return std::nullopt;
    }
    QImageReader reader(&buffer);
    reader.setDecideFormatFromContent(true);
    const QSize decodedSize = reader.size();
    const std::optional<quint64> decodedBytes =
        serializer_detail::rasterDecodedByteCount(decodedSize);
    if (reader.format().toLower() != QByteArrayLiteral("png")
        || decodedSize != size || !decodedBytes)
    {
        return std::nullopt;
    }
    const QImage decoded = reader.read();
    if (decoded.isNull() || decoded.size() != size)
    {
        return std::nullopt;
    }
    QImage canonical = serializer_detail::canonicalRasterImage(decoded);
    if (canonical.isNull())
    {
        return std::nullopt;
    }
    if (!hasVisiblePixels(canonical))
    {
        if (transparentImage.isNull())
        {
            canonical.fill(Qt::transparent);
            transparentImage = canonical;
        }
        return transparentImage;
    }
    const QString contentId = serializer_detail::rasterContentId(canonical);
    if (contentId.isEmpty())
    {
        return std::nullopt;
    }
    const auto existing = decodedImages.constFind(contentId);
    if (existing != decodedImages.cend())
    {
        return *existing;
    }
    if (*decodedBytes > remainingDecodedImageBytes)
    {
        return std::nullopt;
    }
    remainingDecodedImageBytes -= *decodedBytes;
    decodedImages.insert(contentId, canonical);
    return canonical;
}

std::optional<QVector<QPointF>> readPoints(
    BinaryCursor &cursor, qsizetype &remainingPoints)
{
    const std::optional<qint32> count = cursor.readInt32();
    if (!count || *count < 0 || *count > remainingPoints
        || *count > DocumentLimits::maximumPointsPerStroke)
    {
        return std::nullopt;
    }
    QVector<QPointF> points;
    points.reserve(*count);
    for (int index = 0; index < *count; ++index)
    {
        const std::optional<float> x = cursor.readSingle();
        const std::optional<float> y = cursor.readSingle();
        if (!x || !y)
        {
            return std::nullopt;
        }
        const QPointF point(*x, *y);
        if (!validPoint(point))
        {
            return std::nullopt;
        }
        points.append(point);
    }
    remainingPoints -= *count;
    return points;
}

std::optional<QVector<WawaStroke>> readStrokes(BinaryCursor &cursor,
    qsizetype &remainingStrokes,
    qsizetype &remainingPoints)
{
    const std::optional<qint32> count = cursor.readInt32();
    if (!count || *count < 0 || *count > remainingStrokes)
    {
        return std::nullopt;
    }
    QVector<WawaStroke> strokes;
    strokes.reserve(*count);
    for (int index = 0; index < *count; ++index)
    {
        const std::optional<qint32> color = cursor.readInt32();
        const std::optional<qint32> size = cursor.readInt32();
        const std::optional<qint32> opacity = cursor.readInt32();
        const std::optional<bool> airbrush = cursor.readBoolean();
        const std::optional<qint32> seed = cursor.readInt32();
        const std::optional<qint32> order = cursor.readInt32();
        if (!color || !size || !opacity || !airbrush || !seed || !order
            || *size <= 0 || *size > 4096 || *opacity < 0 || *opacity > 100
            || *order < 0)
        {
            return std::nullopt;
        }
        std::optional<QVector<QPointF>> points =
            readPoints(cursor, remainingPoints);
        if (!points)
        {
            return std::nullopt;
        }
        strokes.append({colorFromArgb(*color),
            *size,
            *opacity,
            *airbrush,
            *seed,
            *order,
            std::move(*points)});
    }
    remainingStrokes -= *count;
    return strokes;
}

std::optional<QVector<WawaFill>> readFills(BinaryCursor &cursor,
    qsizetype &remainingStrokes,
    qsizetype &remainingPoints)
{
    const std::optional<qint32> count = cursor.readInt32();
    if (!count || *count < 0 || *count > remainingStrokes)
    {
        return std::nullopt;
    }
    QVector<WawaFill> fills;
    fills.reserve(*count);
    for (int index = 0; index < *count; ++index)
    {
        const std::optional<qint32> color = cursor.readInt32();
        const std::optional<qint32> opacity = cursor.readInt32();
        const std::optional<qint32> seed = cursor.readInt32();
        const std::optional<qint32> order = cursor.readInt32();
        if (!color || !opacity || !seed || !order || *opacity < 0
            || *opacity > 100 || *order < 0)
        {
            return std::nullopt;
        }
        std::optional<QVector<QPointF>> points =
            readPoints(cursor, remainingPoints);
        if (!points)
        {
            return std::nullopt;
        }
        fills.append({colorFromArgb(*color),
            *opacity,
            *seed,
            *order,
            std::move(*points)});
    }
    remainingStrokes -= *count;
    return fills;
}

}

std::optional<WawaProject> WawaV10Reader::read(
    const QByteArray &data, QString *error)
{
    return read(data, DocumentLimits::maximumDistinctRasterDecodedBytes, error);
}

std::optional<WawaProject> WawaV10Reader::read(
    const QByteArray &data, quint64 maximumDecodedImageBytes, QString *error)
{
    if (data.size() > DocumentLimits::maximumProjectBytes)
    {
        setError(error, QStringLiteral("The .wawa project is too large."));
        return std::nullopt;
    }
    if (data.trimmed().startsWith('{'))
    {
        setError(error,
            QStringLiteral("Web JSON .wawa projects are not supported."));
        return std::nullopt;
    }

    BinaryCursor cursor(data);
    const std::optional<QString> magic = cursor.readString(4);
    const std::optional<qint32> version = cursor.readInt32();
    if (!magic || *magic != QStringLiteral("WAWA") || !version)
    {
        setError(error, QStringLiteral("This is not a native .wawa project."));
        return std::nullopt;
    }
    if (*version != nativeVersion)
    {
        setError(error,
            QStringLiteral("Only native .wawa version 10 is supported."));
        return std::nullopt;
    }

    const std::optional<qint32> width = cursor.readInt32();
    const std::optional<qint32> height = cursor.readInt32();
    if (!width || !height || !validCanvas(QSize(*width, *height)))
    {
        setError(error, QStringLiteral("The .wawa canvas size is invalid."));
        return std::nullopt;
    }

    WawaProject project;
    project.canvasSize = QSize(*width, *height);
    const std::optional<qint32> activeLayer = cursor.readInt32();
    const std::optional<qint32> brushSize = cursor.readInt32();
    const std::optional<qint32> opacity = cursor.readInt32();
    const std::optional<qint32> tolerance = cursor.readInt32();
    const std::optional<qint32> wobbleAmount = cursor.readInt32();
    const std::optional<qint32> wobbleSpeed = cursor.readInt32();
    const std::optional<qint32> wobbleDetail = cursor.readInt32();
    const std::optional<qint32> wobbleMode = cursor.readInt32();
    const std::optional<qint32> wobbleHoldFrames = cursor.readInt32();
    const std::optional<qint32> wobbleStepSpeed = cursor.readInt32();
    const std::optional<qint32> wobbleRandomness = cursor.readInt32();
    const std::optional<bool> linkedWiggle = cursor.readBoolean();
    const std::optional<bool> brokenLine = cursor.readBoolean();
    const std::optional<qint32> breakAmount = cursor.readInt32();
    const std::optional<qint32> breakRange = cursor.readInt32();
    const std::optional<bool> wobbleEraser = cursor.readBoolean();
    const std::optional<qint32> brushColor = cursor.readInt32();
    const std::optional<qint32> backgroundColor = cursor.readInt32();
    const std::optional<bool> bucketUseLayerAlpha = cursor.readBoolean();
    const std::optional<bool> bucketAntialias = cursor.readBoolean();
    const std::optional<qint32> layerCount = cursor.readInt32();
    if (!activeLayer || !brushSize || !opacity || !tolerance || !wobbleAmount
        || !wobbleSpeed || !wobbleDetail || !wobbleMode || !wobbleHoldFrames
        || !wobbleStepSpeed || !wobbleRandomness || !linkedWiggle || !brokenLine
        || !breakAmount || !breakRange || !wobbleEraser || !brushColor
        || !backgroundColor || !bucketUseLayerAlpha || !bucketAntialias
        || !layerCount || *layerCount < 1 || *layerCount > maximumNativeLayers
        || *activeLayer < 0 || *activeLayer >= *layerCount)
    {
        setError(error, QStringLiteral("The .wawa settings are invalid."));
        return std::nullopt;
    }
    project.settings = {*activeLayer,
        *brushSize,
        *opacity,
        *tolerance,
        *wobbleAmount,
        *wobbleSpeed,
        *wobbleDetail,
        *wobbleMode,
        *wobbleHoldFrames,
        *wobbleStepSpeed,
        *wobbleRandomness,
        *linkedWiggle,
        *brokenLine,
        *breakAmount,
        *breakRange,
        *wobbleEraser,
        colorFromArgb(*brushColor),
        colorFromArgb(*backgroundColor),
        *bucketUseLayerAlpha,
        *bucketAntialias};

    qsizetype remainingStrokes = DocumentLimits::maximumTotalStrokes;
    qsizetype remainingPoints = DocumentLimits::maximumTotalPoints;
    quint64 remainingDecodedImageBytes = std::min(maximumDecodedImageBytes,
        DocumentLimits::maximumDistinctRasterDecodedBytes);
    QHash<QString, QImage> decodedImages;
    QImage transparentImage;
    project.layers.reserve(*layerCount);
    for (int index = 0; index < *layerCount; ++index)
    {
        const std::optional<QString> name =
            cursor.readString(maximumEncodedLayerNameBytes);
        const std::optional<bool> visible = cursor.readBoolean();
        std::optional<QImage> image = readLayerImage(cursor,
            project.canvasSize,
            remainingDecodedImageBytes,
            decodedImages,
            transparentImage);
        std::optional<QVector<WawaStroke>> paint =
            readStrokes(cursor, remainingStrokes, remainingPoints);
        std::optional<QVector<WawaStroke>> erasers =
            readStrokes(cursor, remainingStrokes, remainingPoints);
        std::optional<QVector<WawaFill>> fills =
            readFills(cursor, remainingStrokes, remainingPoints);
        if (!name || !visible || !image || !paint || !erasers || !fills)
        {
            setError(error, QStringLiteral("The .wawa layer data is invalid."));
            return std::nullopt;
        }
        project.layers.append({*name,
            *visible,
            std::move(*image),
            std::move(*paint),
            std::move(*erasers),
            std::move(*fills)});
    }
    if (!cursor.atEnd())
    {
        setError(error,
            QStringLiteral("The .wawa project has unexpected trailing data."));
        return std::nullopt;
    }
    return project;
}

}

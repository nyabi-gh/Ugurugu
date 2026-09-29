// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include <QBuffer>
#include <QCoreApplication>
#include <QCryptographicHash>
#include <QElapsedTimer>
#include <QImage>
#include <QImageWriter>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QTemporaryFile>
#include <QTextStream>

#include <algorithm>
#include <array>
#include <optional>

namespace
{

constexpr int imageEdge = 4096;
constexpr int bytesPerPixel = 4;
constexpr int compressionLevel = 6;
constexpr qreal bytesPerMiB = 1024.0 * 1024.0;

enum class Encoding
{
    Deflate,
    Png
};

struct Asset
{
    QString id;
    QSize size;
    QByteArray payload;
};

quint32 nextRandom(quint32 &state)
{
    state ^= state << 13U;
    state ^= state >> 17U;
    state ^= state << 5U;
    return state;
}

QImage generatedPhoto(quint32 seed)
{
    QImage image(QSize(imageEdge, imageEdge), QImage::Format_RGBA8888);
    if (image.isNull())
    {
        return {};
    }
    quint32 random = seed;
    for (int y = 0; y < image.height(); ++y)
    {
        auto *row = reinterpret_cast<quint8 *>(image.scanLine(y));
        for (int x = 0; x < image.width(); ++x)
        {
            const quint32 noise = nextRandom(random);
            const int redNoise = static_cast<int>(noise & 0x1fU) - 16;
            const int greenNoise = static_cast<int>((noise >> 5U) & 0x1fU) - 16;
            const int blueNoise = static_cast<int>((noise >> 10U) & 0x1fU) - 16;
            const int offset = x * bytesPerPixel;
            row[offset] = static_cast<quint8>(
                std::clamp(24 + (x * 176) / (imageEdge - 1)
                               + (y * 24) / (imageEdge - 1) + redNoise,
                    0,
                    255));
            row[offset + 1] = static_cast<quint8>(
                std::clamp(36 + (x * 38) / (imageEdge - 1)
                               + (y * 158) / (imageEdge - 1) + greenNoise,
                    0,
                    255));
            row[offset + 2] = static_cast<quint8>(
                std::clamp(52 + (x * 92) / (imageEdge - 1)
                               + (y * 76) / (imageEdge - 1) + blueNoise,
                    0,
                    255));
            row[offset + 3] = 255;
        }
    }
    return image;
}

QString contentId(const QImage &image)
{
    QCryptographicHash hash(QCryptographicHash::Sha256);
    hash.addData(QByteArrayLiteral("WWP_RASTER_RGBA8_V1\n"));
    hash.addData(QByteArray::number(image.width()));
    hash.addData(QByteArrayLiteral("x"));
    hash.addData(QByteArray::number(image.height()));
    hash.addData(QByteArrayLiteral("\n"));
    const qsizetype rowBytes =
        static_cast<qsizetype>(image.width()) * bytesPerPixel;
    for (int y = 0; y < image.height(); ++y)
    {
        hash.addData(QByteArrayView(
            reinterpret_cast<const char *>(image.constScanLine(y)), rowBytes));
    }
    return QString::fromLatin1(hash.result().toHex());
}

QByteArray deflatePayload(const QImage &image)
{
    const qsizetype rowBytes =
        static_cast<qsizetype>(image.width()) * bytesPerPixel;
    if (image.bytesPerLine() != rowBytes)
    {
        return {};
    }
    const QByteArray bytes = QByteArray::fromRawData(
        reinterpret_cast<const char *>(image.constBits()), image.sizeInBytes());
    return qCompress(bytes, compressionLevel);
}

QByteArray pngPayload(const QImage &image)
{
    QByteArray payload;
    QBuffer buffer(&payload);
    if (!buffer.open(QIODevice::WriteOnly))
    {
        return {};
    }
    QImageWriter writer(&buffer, QByteArrayLiteral("png"));
    writer.setCompression(compressionLevel);
    if (!writer.write(image))
    {
        return {};
    }
    return payload;
}

QByteArray encode(const QImage &image, Encoding encoding)
{
    return encoding == Encoding::Deflate ? deflatePayload(image)
                                         : pngPayload(image);
}

QByteArray serialize(const QVector<Asset> &assets, Encoding encoding)
{
    QJsonArray entries;
    for (const Asset &asset : assets)
    {
        QJsonObject object;
        object.insert(QStringLiteral("id"), asset.id);
        object.insert(QStringLiteral("width"), asset.size.width());
        object.insert(QStringLiteral("height"), asset.size.height());
        object.insert(QStringLiteral("format"), QStringLiteral("rgba8"));
        object.insert(QStringLiteral("encoding"),
            encoding == Encoding::Deflate ? QStringLiteral("qcompress-6")
                                          : QStringLiteral("png-6"));
        object.insert(QStringLiteral("data"),
            QString::fromLatin1(asset.payload.toBase64()));
        entries.append(object);
    }
    QJsonObject root;
    root.insert(QStringLiteral("rasterAssets"), entries);
    return QJsonDocument(root).toJson(QJsonDocument::Compact);
}

bool writeTemporary(const QByteArray &data)
{
    QTemporaryFile file;
    return file.open() && file.write(data) == data.size() && file.flush();
}

std::optional<Encoding> parseEncoding(const QString &value)
{
    if (value == QStringLiteral("deflate"))
    {
        return Encoding::Deflate;
    }
    if (value == QStringLiteral("png"))
    {
        return Encoding::Png;
    }
    return std::nullopt;
}

}

int main(int argc, char *argv[])
{
    QCoreApplication application(argc, argv);
    QTextStream output(stdout);
    const QStringList arguments = application.arguments();
    if (arguments.size() != 3)
    {
        output << "usage: ugurugu_raster_asset_probe <deflate|png> "
                  "<1|3|5>\n";
        return 2;
    }
    const std::optional<Encoding> encoding = parseEncoding(arguments[1]);
    bool validCount = false;
    const int assetCount = arguments[2].toInt(&validCount);
    constexpr std::array supportedCounts = {1, 3, 5};
    if (!encoding || !validCount
        || std::ranges::find(supportedCounts, assetCount)
               == supportedCounts.end())
    {
        output << "invalid arguments\n";
        return 2;
    }

    QElapsedTimer totalTimer;
    totalTimer.start();
    QElapsedTimer encodeTimer;
    encodeTimer.start();
    QVector<Asset> assets;
    assets.reserve(assetCount);
    quint64 rawBytes = 0;
    quint64 payloadBytes = 0;
    for (int index = 0; index < assetCount; ++index)
    {
        QImage image = generatedPhoto(
            0x6d2b79f5U + static_cast<quint32>(index) * 0x9e3779b9U);
        if (image.isNull())
        {
            output << "image allocation failed\n";
            return 1;
        }
        QByteArray payload = encode(image, *encoding);
        if (payload.isEmpty())
        {
            output << "encoding failed\n";
            return 1;
        }
        rawBytes += image.sizeInBytes();
        payloadBytes += payload.size();
        assets.append(
            Asset{contentId(image), image.size(), std::move(payload)});
    }
    const qint64 encodeMilliseconds = encodeTimer.elapsed();

    QElapsedTimer serializeTimer;
    serializeTimer.start();
    QByteArray json = serialize(assets, *encoding);
    const qint64 serializeMilliseconds = serializeTimer.elapsed();
    if (json.isEmpty())
    {
        output << "serialization failed\n";
        return 1;
    }
    QElapsedTimer writeTimer;
    writeTimer.start();
    if (!writeTemporary(json))
    {
        output << "temporary write failed\n";
        return 1;
    }
    const qint64 writeMilliseconds = writeTimer.elapsed();
    const quint64 jsonBytes = json.size();
    json.clear();
    json.squeeze();

    QElapsedTimer repeatTimer;
    repeatTimer.start();
    json = serialize(assets, *encoding);
    const qint64 repeatMilliseconds = repeatTimer.elapsed();

    output << "encoding=" << arguments[1] << " assets=" << assetCount
           << " rawMiB=" << static_cast<qreal>(rawBytes) / bytesPerMiB
           << " payloadMiB=" << static_cast<qreal>(payloadBytes) / bytesPerMiB
           << " jsonMiB=" << static_cast<qreal>(jsonBytes) / bytesPerMiB
           << " encodeMs=" << encodeMilliseconds
           << " serializeMs=" << serializeMilliseconds
           << " repeatSerializeMs=" << repeatMilliseconds
           << " writeMs=" << writeMilliseconds
           << " totalMs=" << totalTimer.elapsed() << '\n';
    return 0;
}

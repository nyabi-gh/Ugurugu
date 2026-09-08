// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "document/FrozenFillMask.hpp"

#include <QByteArray>
#include <QElapsedTimer>
#include <QJsonArray>
#include <QJsonDocument>
#include <QTextStream>

#include <algorithm>
#include <cmath>
#include <numbers>

namespace
{

constexpr int canvasEdge = 4096;
constexpr int pointCount = 512;
constexpr int frameCount = 60;
constexpr qreal displacement = 4.0;

QVector<QPointF> representativePolygon()
{
    QVector<QPointF> polygon;
    polygon.reserve(pointCount);
    const qreal center = canvasEdge / 2.0;
    for (int index = 0; index < pointCount; ++index)
    {
        const qreal angle = 2.0 * std::numbers::pi * index / pointCount;
        const qreal radius = canvasEdge
                             * (0.32 + 0.04 * std::sin(angle * 7.0)
                                 + 0.025 * std::cos(angle * 13.0));
        polygon.append(QPointF(center + std::cos(angle) * radius,
            center + std::sin(angle) * radius));
    }
    return polygon;
}

QVector<QPointF> displacedPolygon(const QVector<QPointF> &source, int frame)
{
    QVector<QPointF> polygon;
    polygon.reserve(source.size());
    const QPointF center(canvasEdge / 2.0, canvasEdge / 2.0);
    const qreal phase = 2.0 * std::numbers::pi * frame / frameCount;
    for (int index = 0; index < source.size(); ++index)
    {
        const qreal offset =
            displacement * std::sin(phase + index * 0.61803398875);
        const QPointF direction = source[index] - center;
        const qreal length = std::hypot(direction.x(), direction.y());
        polygon.append(
            source[index] + direction / std::max(length, 1.0) * offset);
    }
    return polygon;
}

QByteArray canonicalBytes(const QImage &mask)
{
    const qsizetype rowBytes = mask.width();
    QByteArray bytes(rowBytes * static_cast<qsizetype>(mask.height()), '\0');
    for (int y = 0; y < mask.height(); ++y)
    {
        std::copy_n(mask.constScanLine(y),
            rowBytes,
            reinterpret_cast<uchar *>(
                bytes.data() + rowBytes * static_cast<qsizetype>(y)));
    }
    return bytes;
}

qsizetype polygonJsonBytes(const QVector<QPointF> &polygon)
{
    QJsonArray points;
    for (const QPointF &point : polygon)
    {
        points.append(QJsonArray{point.x(), point.y()});
    }
    return QJsonDocument(points).toJson(QJsonDocument::Compact).size();
}

}

int main()
{
    const QVector<QPointF> polygon = representativePolygon();

    QElapsedTimer timer;
    timer.start();
    const std::optional<QImage> frozen = ugurugu::FrozenFillMask::fromPolygon(
        QSize(canvasEdge, canvasEdge), polygon);
    const qint64 frozenRasterMilliseconds = timer.elapsed();
    if (!frozen)
    {
        return 1;
    }

    const QByteArray canonical = canonicalBytes(*frozen);
    timer.restart();
    const QByteArray compressed = qCompress(canonical, 6);
    const qint64 compressionMilliseconds = timer.elapsed();
    const auto packed = ugurugu::FrozenFillMask::packedFromPolygon(
        QSize(canvasEdge, canvasEdge), polygon);
    if (!packed)
    {
        return 1;
    }
    const QByteArray compressedPacked = qCompress(packed->packedMask, 6);

    timer.restart();
    quint64 rasterChecksum = 0;
    for (int frame = 0; frame < frameCount; ++frame)
    {
        const auto mask = ugurugu::FrozenFillMask::fromPolygon(
            QSize(canvasEdge, canvasEdge), displacedPolygon(polygon, frame));
        if (!mask)
        {
            return 1;
        }
        rasterChecksum += mask->constScanLine(canvasEdge / 2)[canvasEdge / 2];
    }
    const qint64 proceduralRasterMilliseconds = timer.elapsed();

    QTextStream output(stdout);
    output << "canvas=" << canvasEdge << 'x' << canvasEdge << '\n';
    output << "points=" << pointCount << '\n';
    output << "frames=" << frameCount << '\n';
    output << "frozenDecodedBytes=" << frozen->sizeInBytes() << '\n';
    output << "frozenCompressedBytes=" << compressed.size() << '\n';
    output << "frozenBase64Bytes=" << 4 * ((compressed.size() + 2) / 3) << '\n';
    output << "packedBounds=" << packed->bounds.width() << 'x'
           << packed->bounds.height() << '\n';
    output << "packedDecodedBytes=" << packed->packedMask.size() << '\n';
    output << "packedCompressedBytes=" << compressedPacked.size() << '\n';
    output << "packedBase64Bytes=" << 4 * ((compressedPacked.size() + 2) / 3)
           << '\n';
    output << "polygonJsonBytes=" << polygonJsonBytes(polygon) << '\n';
    output << "frozenRasterMilliseconds=" << frozenRasterMilliseconds << '\n';
    output << "compressionMilliseconds=" << compressionMilliseconds << '\n';
    output << "procedural60RasterMilliseconds=" << proceduralRasterMilliseconds
           << '\n';
    output << "rasterChecksum=" << rasterChecksum << '\n';
    return 0;
}

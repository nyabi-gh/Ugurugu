// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "render/FloodFillMask.hpp"

#include <algorithm>
#include <cstdlib>

namespace ugurugu::FloodFillMask
{

QImage fromImage(const QImage &image,
    const QPoint &seed,
    Comparison comparison,
    int tolerance)
{
    if (image.isNull() || image.format() != QImage::Format_ARGB32_Premultiplied
        || !image.rect().contains(seed) || tolerance < 0 || tolerance > 255)
    {
        return {};
    }
    const QRgb target = qUnpremultiply(image.pixel(seed));
    const Comparison effectiveComparison =
        comparison == Comparison::Color && tolerance == 0
            ? Comparison::AlphaBoundary
            : comparison;
    const auto blocked = [&image, effectiveComparison, tolerance, target](
                             int x, int y)
    {
        const auto *line =
            reinterpret_cast<const QRgb *>(image.constScanLine(y));
        if (effectiveComparison == Comparison::AlphaBoundary)
        {
            return qAlpha(line[x]) >= 128;
        }
        const QRgb pixel = qUnpremultiply(line[x]);
        return std::max({std::abs(qRed(pixel) - qRed(target)),
                   std::abs(qGreen(pixel) - qGreen(target)),
                   std::abs(qBlue(pixel) - qBlue(target)),
                   std::abs(qAlpha(pixel) - qAlpha(target))})
               > tolerance;
    };
    if (blocked(seed.x(), seed.y()))
    {
        return {};
    }

    QImage mask(image.size(), QImage::Format_Grayscale8);
    if (mask.isNull())
    {
        return {};
    }
    mask.fill(0);
    QVector<QPoint> pending{seed};
    while (!pending.isEmpty())
    {
        const QPoint point = pending.takeLast();
        const int y = point.y();
        uchar *maskLine = mask.scanLine(y);
        if (maskLine[point.x()] || blocked(point.x(), y))
        {
            continue;
        }
        int left = point.x();
        while (left > 0 && !maskLine[left - 1] && !blocked(left - 1, y))
        {
            --left;
        }
        int right = point.x();
        while (right < image.width() - 1 && !maskLine[right + 1]
               && !blocked(right + 1, y))
        {
            ++right;
        }
        std::fill(maskLine + left, maskLine + right + 1, uchar(255));
        for (const int neighborY : {y - 1, y + 1})
        {
            if (neighborY < 0 || neighborY >= image.height())
            {
                continue;
            }
            const uchar *neighborMask = mask.constScanLine(neighborY);
            for (int x = left; x <= right; ++x)
            {
                if (!neighborMask[x] && !blocked(x, neighborY))
                {
                    pending.append(QPoint(x, neighborY));
                    while (x < right && !neighborMask[x + 1]
                           && !blocked(x + 1, neighborY))
                    {
                        ++x;
                    }
                }
            }
        }
    }
    return mask;
}

}

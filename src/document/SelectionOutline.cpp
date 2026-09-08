// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "document/SelectionOutline.hpp"

#include <QHash>

namespace ugurugu
{

namespace
{

quint64 encodedPoint(int x, int y)
{
    return (static_cast<quint64>(static_cast<quint32>(x)) << 32U)
           | static_cast<quint32>(y);
}

QPointF decodedPoint(quint64 point)
{
    return QPointF(
        static_cast<quint32>(point >> 32U), static_cast<quint32>(point));
}

}

QVector<QPolygonF> selectionOutline(const QImage &mask)
{
    if (mask.isNull() || mask.format() != QImage::Format_Grayscale8)
    {
        return {};
    }

    QHash<quint64, QVector<quint64>> edges;
    const auto inside = [&mask](int x, int y)
    {
        return x >= 0 && y >= 0 && x < mask.width() && y < mask.height()
               && mask.constScanLine(y)[x] >= 128;
    };
    const auto addEdge = [&edges](int x1, int y1, int x2, int y2)
    {
        edges[encodedPoint(x1, y1)].append(encodedPoint(x2, y2));
    };

    for (int y = 0; y < mask.height(); ++y)
    {
        const uchar *line = mask.constScanLine(y);
        for (int x = 0; x < mask.width(); ++x)
        {
            if (line[x] < 128)
            {
                continue;
            }
            if (!inside(x, y - 1))
            {
                addEdge(x, y, x + 1, y);
            }
            if (!inside(x + 1, y))
            {
                addEdge(x + 1, y, x + 1, y + 1);
            }
            if (!inside(x, y + 1))
            {
                addEdge(x + 1, y + 1, x, y + 1);
            }
            if (!inside(x - 1, y))
            {
                addEdge(x, y + 1, x, y);
            }
        }
    }

    QVector<QPolygonF> contours;
    while (!edges.isEmpty())
    {
        auto first = edges.begin();
        const quint64 start = first.key();
        quint64 current = start;
        QPolygonF contour;
        contour.append(decodedPoint(start));

        do
        {
            auto edge = edges.find(current);
            if (edge == edges.end())
            {
                break;
            }
            const quint64 next = edge.value().takeLast();
            if (edge.value().isEmpty())
            {
                edges.erase(edge);
            }
            contour.append(decodedPoint(next));
            current = next;
        } while (current != start);

        contours.append(std::move(contour));
    }
    return contours;
}

}

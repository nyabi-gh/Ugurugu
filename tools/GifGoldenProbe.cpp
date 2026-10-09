// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

// Writes 2.2.13's GIFs of fixed test frames, the golden files the Rust GIF
// writer is compared with (crates/ugu-io/src/gif.rs, docs/rust/m5-plan.md
// M5-8). The frames come from integer formulas the Rust tests repeat.
//
// Usage: ugurugu_gif_golden_probe <opaque|transparent|few|large> <out.gif>

#include "io/GifWriter.hpp"

#include <QImage>
#include <QString>
#include <QTextStream>
#include <QVector>

namespace
{

struct Case
{
    int width;
    int height;
    QVector<int> delays;
};

int channel(const QString &name, int frame, int x, int y, int which)
{
    if (name == QStringLiteral("few"))
    {
        const int values[] = {
            (x / 16) * 80 % 256, (y / 16) * 90 % 256, frame * 60 % 256, 255};
        return values[which];
    }
    const int values[] = {
        (x * 7 + y * 3 + frame * 40 + ((x * y) % 13) * 5) % 256,
        (x * 2 + y * 9 + frame * 17 + ((x ^ y) & 31)) % 256,
        (x * y + frame * 91) % 256,
        name == QStringLiteral("transparent")
            ? ((x + y * 2 + frame * 5) % 5) * 60 + 15
            : 255};
    return values[which];
}

}

int main(int argc, char **argv)
{
    QTextStream err(stderr);
    if (argc != 3)
    {
        err << "usage: ugurugu_gif_golden_probe "
               "<opaque|transparent|few|large> <out.gif>\n";
        return 2;
    }
    const QString name = QString::fromLocal8Bit(argv[1]);
    Case shape{64, 48, {4, 4, 5}};
    if (name == QStringLiteral("large"))
    {
        shape = {300, 200, {7, 6}};
    }
    else if (name != QStringLiteral("opaque")
             && name != QStringLiteral("transparent")
             && name != QStringLiteral("few"))
    {
        err << "unknown case\n";
        return 2;
    }
    QVector<QImage> frames;
    for (int frame = 0; frame < shape.delays.size(); ++frame)
    {
        QImage image(shape.width, shape.height, QImage::Format_ARGB32);
        for (int y = 0; y < shape.height; ++y)
        {
            auto *row = reinterpret_cast<QRgb *>(image.scanLine(y));
            for (int x = 0; x < shape.width; ++x)
            {
                row[x] = qRgba(channel(name, frame, x, y, 0),
                    channel(name, frame, x, y, 1),
                    channel(name, frame, x, y, 2),
                    channel(name, frame, x, y, 3));
            }
        }
        frames.append(image);
    }
    QString error;
    if (!ugurugu::GifWriter::write(
            QString::fromLocal8Bit(argv[2]), frames, shape.delays, &error))
    {
        err << error << "\n";
        return 1;
    }
    return 0;
}

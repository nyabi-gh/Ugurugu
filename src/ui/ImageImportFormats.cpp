// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "ui/ImageImportFormats.hpp"

#include <QImageReader>

namespace ugurugu::ImageImportFormats
{

QStringList suffixes()
{
    const QList<QByteArray> readable = QImageReader::supportedImageFormats();
    QStringList offered;
    for (const char *suffix :
        {"png", "jpg", "jpeg", "webp", "bmp", "gif", "tif", "tiff"})
    {
        if (readable.contains(QByteArray(suffix)))
        {
            offered.append(QString::fromLatin1(suffix));
        }
    }
    return offered;
}

QString nameFilterPattern()
{
    QStringList patterns;
    for (const QString &suffix : suffixes())
    {
        patterns.append(QStringLiteral("*.") + suffix);
    }
    return patterns.join(QLatin1Char(' '));
}

}

// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#pragma once

#include <QCoreApplication>
#include <QImage>
#include <QString>
#include <QVector>

#include <functional>

namespace ugurugu
{

class GifWriter final
{
    Q_DECLARE_TR_FUNCTIONS(ugurugu::GifWriter)

public:
    static bool write(const QString &path,
        const QVector<QImage> &frames,
        int delayCentiseconds,
        QString *error = nullptr,
        const std::function<bool()> &isCanceled = {});

    static bool write(const QString &path,
        const QVector<QImage> &frames,
        const QVector<int> &delaysCentiseconds,
        QString *error = nullptr,
        const std::function<bool()> &isCanceled = {});
};

}

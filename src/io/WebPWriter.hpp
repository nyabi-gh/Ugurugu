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

class WebPWriter final
{
    Q_DECLARE_TR_FUNCTIONS(ugurugu::WebPWriter)

public:
    static bool write(const QString &filePath,
        const QVector<QImage> &frames,
        const QVector<int> &durationsMilliseconds,
        QString *error = nullptr,
        const std::function<bool()> &isCanceled = {});
};

}

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
    // Produces frame `index`, or a null image when it cannot. Each frame is
    // requested once, in order, and encoded before the next is requested.
    using FrameSource = std::function<QImage(int index)>;

    static bool write(const QString &filePath,
        int frameCount,
        const FrameSource &frameSource,
        const QVector<int> &durationsMilliseconds,
        QString *error = nullptr,
        const std::function<bool()> &isCanceled = {});

    static bool write(const QString &filePath,
        const QVector<QImage> &frames,
        const QVector<int> &durationsMilliseconds,
        QString *error = nullptr,
        const std::function<bool()> &isCanceled = {});
};

}

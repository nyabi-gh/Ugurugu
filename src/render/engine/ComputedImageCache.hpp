// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#pragma once

#include <QCache>
#include <QImage>
#include <QMutex>
#include <QSet>
#include <QString>
#include <QWaitCondition>

#include <functional>

namespace ugurugu
{
namespace render_detail
{

// Cost-bounded image cache whose concurrent misses on one key compute the
// image once: the first thread to miss computes it, the others wait for it.
// A null result is passed through and not kept.
class ComputedImageCache final
{
public:
    explicit ComputedImageCache(qint64 budgetBytes);

    QImage imageOrCompute(
        const QString &key, const std::function<QImage()> &compute);
    void clear();

private:
    QMutex m_mutex;
    QWaitCondition m_settled;
    QCache<QString, QImage> m_images;
    QSet<QString> m_inFlight;
};

}

}

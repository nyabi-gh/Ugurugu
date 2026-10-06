// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "render/engine/ComputedImageCache.hpp"

#include <QMutexLocker>
#include <QScopeGuard>

#include <algorithm>

namespace ugurugu
{
namespace render_detail
{

namespace
{

int cacheCost(const QImage &image)
{
    return static_cast<int>(
        std::max<qsizetype>(1, (image.sizeInBytes() + 1023) / 1024));
}

}

ComputedImageCache::ComputedImageCache(qint64 budgetBytes)
{
    m_images.setMaxCost(static_cast<int>(budgetBytes / 1024));
}

QImage ComputedImageCache::imageOrCompute(
    const QString &key, const std::function<QImage()> &compute)
{
    QMutexLocker locker(&m_mutex);
    for (;;)
    {
        if (const QImage *cached = m_images.object(key))
        {
            return *cached;
        }
        if (!m_inFlight.contains(key))
        {
            break;
        }
        m_settled.wait(&m_mutex);
    }
    m_inFlight.insert(key);
    locker.unlock();
    [[maybe_unused]] const auto releaseClaim = qScopeGuard(
        [&mutex = m_mutex, &inFlight = m_inFlight, &settled = m_settled, &key]()
        {
            const QMutexLocker locked(&mutex);
            inFlight.remove(key);
            settled.wakeAll();
        });

    const QImage image = compute();
    if (!image.isNull())
    {
        const QMutexLocker publish(&m_mutex);
        m_images.insert(key, new QImage(image), cacheCost(image));
    }
    return image;
}

void ComputedImageCache::clear()
{
    const QMutexLocker locker(&m_mutex);
    m_images.clear();
}

}

}

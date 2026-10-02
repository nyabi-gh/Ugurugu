// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "render/engine/StaticLayerCache.hpp"

#include "app/MemoryBudget.hpp"
#include "render/engine/RenderCancellation.hpp"

#include <QHash>
#include <QMutex>
#include <QMutexLocker>
#include <QWaitCondition>

#include <algorithm>

namespace ugurugu
{
namespace render_detail
{

size_t qHash(const StaticLayerCache::Key &key, size_t seed = 0) noexcept
{
    return qHashMulti(seed,
        key.layerId,
        key.outputSize.width(),
        key.outputSize.height(),
        key.documentSize.width(),
        key.documentSize.height(),
        key.initialCanvasSize.width(),
        key.initialCanvasSize.height(),
        key.displayScale);
}

namespace
{

struct Entry
{
    QVector<Stroke> strokes;
    QMap<QString, RasterAsset> rasterAssets;
    QImage image;
    quint64 owner = 0;
    quint64 lastUse = 0;
    bool ready = false;
};

struct State
{
    QMutex mutex;
    QWaitCondition settled;
    QHash<StaticLayerCache::Key, Entry> entries;
    qint64 budget = MemoryBudget::staticLayerCacheBytes;
    qint64 resident = 0;
    quint64 clock = 0;
};

State &state()
{
    static State instance;
    return instance;
}

bool sameSource(
    const Entry &entry, const Document &document, const Layer &layer)
{
    return entry.strokes.isSharedWith(layer.strokes)
           && entry.rasterAssets.isSharedWith(document.rasterAssets);
}

void erase(State &cache, QHash<StaticLayerCache::Key, Entry>::iterator entry)
{
    if (entry->ready)
    {
        cache.resident -= entry->image.sizeInBytes();
    }
    cache.entries.erase(entry);
}

void evictToBudget(State &cache)
{
    while (cache.resident > cache.budget)
    {
        auto oldest = cache.entries.end();
        for (auto entry = cache.entries.begin(); entry != cache.entries.end();
            ++entry)
        {
            if (entry->ready
                && (oldest == cache.entries.end()
                    || entry->lastUse < oldest->lastUse))
            {
                oldest = entry;
            }
        }
        if (oldest == cache.entries.end())
        {
            return;
        }
        erase(cache, oldest);
    }
}

}

bool isLayerFrameInvariant(const Document &document, const Layer &layer)
{
    if (document.animationFrames <= 1)
    {
        return true;
    }
    if (document.motion.brokenLine)
    {
        return false;
    }
    return std::none_of(layer.strokes.cbegin(),
        layer.strokes.cend(),
        [&document](const Stroke &stroke)
        {
            return document.wobbleAmount * stroke.brush.wobbleScale != 0.0
                   || (stroke.brush.animatedJitter
                       && stroke.brush.wobbleScale > 0.0);
        });
}

QImage StaticLayerCache::raster(const Document &document,
    const Layer &layer,
    const Key &key,
    const std::function<QImage()> &render,
    const std::atomic_bool *cancellation)
{
    State &cache = state();
    QMutexLocker locker(&cache.mutex);
    if (cache.budget <= 0)
    {
        locker.unlock();
        return render();
    }

    auto entry = cache.entries.find(key);
    if (entry != cache.entries.end() && sameSource(*entry, document, layer))
    {
        // Another thread is rendering this very raster; waiting costs less
        // than rendering it a second time.
        while (entry != cache.entries.end() && !entry->ready
               && sameSource(*entry, document, layer))
        {
            if (isRenderCancelled(cancellation))
            {
                return {};
            }
            cache.settled.wait(&cache.mutex, 20);
            entry = cache.entries.find(key);
        }
        if (entry != cache.entries.end() && entry->ready
            && sameSource(*entry, document, layer))
        {
            entry->lastUse = ++cache.clock;
            return entry->image;
        }
        // The first render failed or was superseded; render uncached rather
        // than contend for the slot again.
        locker.unlock();
        return render();
    }

    if (entry != cache.entries.end())
    {
        erase(cache, entry);
    }
    const quint64 owner = ++cache.clock;
    Entry pending;
    pending.strokes = layer.strokes;
    pending.rasterAssets = document.rasterAssets;
    pending.owner = owner;
    cache.entries.insert(key, std::move(pending));
    locker.unlock();

    QImage image = render();

    locker.relock();
    entry = cache.entries.find(key);
    if (entry != cache.entries.end() && entry->owner == owner)
    {
        if (image.isNull() || image.sizeInBytes() > cache.budget)
        {
            erase(cache, entry);
        }
        else
        {
            entry->image = image;
            entry->ready = true;
            entry->lastUse = ++cache.clock;
            cache.resident += image.sizeInBytes();
            evictToBudget(cache);
        }
    }
    cache.settled.wakeAll();
    return image;
}

void StaticLayerCache::setBudget(qint64 bytes)
{
    State &cache = state();
    const QMutexLocker locker(&cache.mutex);
    cache.budget = std::max<qint64>(0, bytes);
    evictToBudget(cache);
}

qint64 StaticLayerCache::budget()
{
    State &cache = state();
    const QMutexLocker locker(&cache.mutex);
    return cache.budget;
}

qint64 StaticLayerCache::residentBytes()
{
    State &cache = state();
    const QMutexLocker locker(&cache.mutex);
    return cache.resident;
}

void StaticLayerCache::clear()
{
    State &cache = state();
    const QMutexLocker locker(&cache.mutex);
    // Pending entries stay so their renders still settle their waiters.
    for (auto entry = cache.entries.begin(); entry != cache.entries.end();)
    {
        if (entry->ready)
        {
            cache.resident -= entry->image.sizeInBytes();
            entry = cache.entries.erase(entry);
        }
        else
        {
            ++entry;
        }
    }
}

}

}

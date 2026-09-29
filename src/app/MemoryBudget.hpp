// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#pragma once

#include <QtTypes>

namespace ugurugu
{

// The large in-memory subsystems share one documented process budget. Export
// clears preview, serialization and decoded raster caches before allocating
// its working set. Project serialization clears decoded raster images before
// constructing JSON while retaining compressed raster payloads.
struct MemoryBudget final
{
    static constexpr qint64 residentTargetBytes = 4096LL * 1024LL * 1024LL;
    static constexpr qint64 historyResidentBytes = 192LL * 1024LL * 1024LL;
    static constexpr qint64 serializationCacheBytes = 64LL * 1024LL * 1024LL;
    static constexpr qint64 rasterAssetEncodedBytes = 72LL * 1024LL * 1024LL;
    static constexpr qint64 rasterDecodeCacheBytes = 128LL * 1024LL * 1024LL;
    static constexpr qint64 projectSerializationWorkingBytes =
        512LL * 1024LL * 1024LL;
    static constexpr quint64 animationExportWorkingBytes =
        512ULL * 1024ULL * 1024ULL;

    // The preview frame cache is the one budget worth spending real memory on:
    // every frame it cannot hold is re-rendered on the GUI thread the next time
    // playback reaches it. A fixed ceiling either starved capable machines or
    // pushed small ones into swap, so the ceiling follows installed memory and
    // is clamped to the range below.
    static constexpr int minimumPreviewCacheKiB = 128 * 1024;
    static constexpr int maximumPreviewCacheKiB = 2048 * 1024;
    static constexpr qint64 previewCacheMemoryShare = 16;

    // Installed physical memory in bytes, or 0 when the platform cannot report
    // it. Read once and cached; hot-plugged memory is not worth tracking.
    static qint64 installedPhysicalBytes();
    static int previewCacheKiB();
};

static_assert(
    MemoryBudget::historyResidentBytes + MemoryBudget::rasterAssetEncodedBytes
        + static_cast<qint64>(MemoryBudget::animationExportWorkingBytes)
    <= MemoryBudget::residentTargetBytes);
static_assert(
    MemoryBudget::historyResidentBytes + MemoryBudget::serializationCacheBytes
        + MemoryBudget::rasterAssetEncodedBytes
        + MemoryBudget::rasterDecodeCacheBytes
        + static_cast<qint64>(MemoryBudget::maximumPreviewCacheKiB) * 1024LL
    <= MemoryBudget::residentTargetBytes);
static_assert(
    MemoryBudget::historyResidentBytes + MemoryBudget::serializationCacheBytes
        + MemoryBudget::rasterAssetEncodedBytes
        + MemoryBudget::projectSerializationWorkingBytes
        + static_cast<qint64>(MemoryBudget::maximumPreviewCacheKiB) * 1024LL
    <= MemoryBudget::residentTargetBytes);
static_assert(MemoryBudget::minimumPreviewCacheKiB
              <= MemoryBudget::maximumPreviewCacheKiB);

}

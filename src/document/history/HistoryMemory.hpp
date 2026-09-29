// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#pragma once

#include "document/Document.hpp"

#include <QByteArray>
#include <QHash>
#include <QImage>
#include <QString>
#include <QVector>

#include <limits>
#include <optional>

namespace ugurugu
{
namespace history
{

// History entries share payload backings with the live document and with each
// other, so a backing must be charged once per distinct address rather than
// once per reference. Callers accumulate into one footprint per measured
// scope; `addOwned` covers storage the entry allocates for itself and
// `addShared` deduplicates by backing identity. Every accumulator saturates
// instead of overflowing, because the caller compares the total against a
// budget and a wrapped total would read as "within budget".
struct MemoryFootprint
{
    qint64 ownedBytes = 0;
    QHash<QString, qint64> sharedBackings;

    void addOwned(qint64 bytes)
    {
        if (bytes <= 0)
        {
            return;
        }
        if (ownedBytes > std::numeric_limits<qint64>::max() - bytes)
        {
            ownedBytes = std::numeric_limits<qint64>::max();
            return;
        }
        ownedBytes += bytes;
    }

    void addShared(const QString &kind, quint64 identity, qint64 bytes)
    {
        if (identity == 0 || bytes <= 0)
        {
            return;
        }
        const QString key =
            QStringLiteral("%1:%2").arg(kind).arg(identity, 0, 16);
        const auto existing = sharedBackings.constFind(key);
        if (existing == sharedBackings.cend() || *existing < bytes)
        {
            sharedBackings.insert(key, bytes);
        }
    }

    qint64 totalBytes() const
    {
        qint64 total = ownedBytes;
        for (const qint64 bytes : sharedBackings)
        {
            if (total > std::numeric_limits<qint64>::max() - bytes)
            {
                return std::numeric_limits<qint64>::max();
            }
            total += bytes;
        }
        return total;
    }
};

// Identity is the backing address, so an accounted container must outlive the
// footprint it was charged to; otherwise a later allocation can reuse the
// address and collide with the stale entry.
template <typename T>
void accountVectorStorage(
    MemoryFootprint &footprint, const QVector<T> &items, const QString &kind)
{
    if (items.capacity() <= 0 || !items.constData())
    {
        return;
    }
    footprint.addShared(kind,
        reinterpret_cast<quintptr>(items.constData()),
        static_cast<qint64>(items.capacity()) * static_cast<qint64>(sizeof(T)));
}

void accountByteStorage(MemoryFootprint &footprint, const QByteArray &bytes);

void accountImageStorage(MemoryFootprint &footprint, const QImage &image);

void accountPackedMask(
    MemoryFootprint &footprint, const std::optional<PackedMaskRegion> &mask);

}

}

// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "render/DeterministicNoise.hpp"

#include <cmath>
#include <limits>

namespace ugurugu::DeterministicNoise
{

namespace
{

quint64 mixHash(quint64 value)
{
    value += 0x9e3779b97f4a7c15ULL;
    value = (value ^ (value >> 30U)) * 0xbf58476d1ce4e5b9ULL;
    value = (value ^ (value >> 27U)) * 0x94d049bb133111ebULL;
    return value ^ (value >> 31U);
}

}

qreal signedValue(quint64 seed, int frame, qint64 index, quint64 channel)
{
    quint64 value = seed;
    value ^= mixHash((static_cast<quint64>(static_cast<qint64>(frame)) + 1ULL)
                     * 0x517cc1b727220a95ULL);
    value ^= mixHash(
        (static_cast<quint64>(index) + 4099ULL) * 0x6eed0e9da4d94a4fULL);
    value ^= channel;
    const quint64 result = mixHash(value);
    const qreal unit =
        static_cast<qreal>(result >> 11U) / static_cast<qreal>(1ULL << 53U);
    return unit * 2.0 - 1.0;
}

qreal smoothValue(quint64 seed, int frame, qreal coordinate, quint64 channel)
{
    if (!std::isfinite(coordinate))
    {
        return 0.0;
    }
    const qreal leftCoordinate = std::floor(coordinate);
    const long double extendedLeft = static_cast<long double>(leftCoordinate);
    if (extendedLeft
            < static_cast<long double>(std::numeric_limits<qint64>::min())
        || extendedLeft
               >= static_cast<long double>(std::numeric_limits<qint64>::max()))
    {
        return 0.0;
    }
    const qint64 left = static_cast<qint64>(leftCoordinate);
    const qreal fraction = coordinate - leftCoordinate;
    const qreal blend = fraction * fraction * (3.0 - 2.0 * fraction);
    const qreal a = signedValue(seed, frame, left, channel);
    const qreal b = signedValue(seed, frame, left + 1, channel);
    return a + (b - a) * blend;
}

qreal unitValue(quint64 seed, int frame, qint64 index, quint64 channel)
{
    return (signedValue(seed, frame, index, channel) + 1.0) * 0.5;
}

}

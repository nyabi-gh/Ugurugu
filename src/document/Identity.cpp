// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "document/Identity.hpp"

#include <QByteArrayView>
#include <QRandomGenerator>
#include <QtEndian>

#include <array>
#include <atomic>
#include <mutex>

namespace ugurugu::Identity
{
namespace
{

constexpr quint64 idStreamSalt = 0x6a09e667f3bcc909ULL;

std::atomic<bool> deterministic{false};
std::mutex stateMutex;
quint64 idState = 0;
quint64 seedState = 0;

quint64 splitMix64(quint64 &state)
{
    state += 0x9e3779b97f4a7c15ULL;
    quint64 value = state;
    value = (value ^ (value >> 30)) * 0xbf58476d1ce4e5b9ULL;
    value = (value ^ (value >> 27)) * 0x94d049bb133111ebULL;
    return value ^ (value >> 31);
}

}

QUuid newId()
{
    if (!deterministic.load(std::memory_order_acquire))
    {
        return QUuid::createUuid();
    }
    std::array<uchar, 16> bytes{};
    {
        const std::lock_guard lock(stateMutex);
        qToBigEndian(splitMix64(idState), bytes.data());
        qToBigEndian(splitMix64(idState), bytes.data() + 8);
    }
    bytes[6] = uchar((bytes[6] & 0x0f) | 0x40);
    bytes[8] = uchar((bytes[8] & 0x3f) | 0x80);
    return QUuid::fromRfc4122(QByteArrayView(bytes.data(), bytes.size()));
}

quint64 newSeed()
{
    if (!deterministic.load(std::memory_order_acquire))
    {
        return QRandomGenerator::global()->generate64();
    }
    const std::lock_guard lock(stateMutex);
    return splitMix64(seedState);
}

DeterministicScope::DeterministicScope(quint64 seed)
{
    const std::lock_guard lock(stateMutex);
    m_wasDeterministic = deterministic.load(std::memory_order_relaxed);
    m_previousIdState = idState;
    m_previousSeedState = seedState;
    idState = seed ^ idStreamSalt;
    seedState = seed;
    deterministic.store(true, std::memory_order_release);
}

DeterministicScope::~DeterministicScope()
{
    const std::lock_guard lock(stateMutex);
    idState = m_previousIdState;
    seedState = m_previousSeedState;
    deterministic.store(m_wasDeterministic, std::memory_order_release);
}

}

// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#pragma once

#include <QUuid>
#include <QtGlobal>

namespace ugurugu::Identity
{

// Every new layer and stroke id and every new stroke seed comes from here.
QUuid newId();
quint64 newSeed();

// Replaces both random sources with splitmix64 sequences derived from one
// seed while it lives, so the reference exporter and tests build the same
// document from the same recipe on every platform. Scopes nest; ending one
// restores the sources it replaced.
class DeterministicScope final
{
public:
    explicit DeterministicScope(quint64 seed);
    ~DeterministicScope();

    DeterministicScope(const DeterministicScope &) = delete;
    DeterministicScope &operator=(const DeterministicScope &) = delete;

private:
    bool m_wasDeterministic = false;
    quint64 m_previousIdState = 0;
    quint64 m_previousSeedState = 0;
};

}

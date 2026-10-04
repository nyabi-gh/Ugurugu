// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "support/ProcessMemory.hpp"

#if defined(Q_OS_WIN)
#ifndef WIN32_LEAN_AND_MEAN
#define WIN32_LEAN_AND_MEAN
#endif
#include <windows.h>
// psapi.h uses the types windows.h declares.
#include <psapi.h>
#else
#include <sys/resource.h>
#endif

namespace ugurugu
{

qint64 peakResidentBytes()
{
#if defined(Q_OS_WIN)
    PROCESS_MEMORY_COUNTERS counters{};
    if (!K32GetProcessMemoryInfo(
            GetCurrentProcess(), &counters, sizeof(counters)))
    {
        return 0;
    }
    return static_cast<qint64>(counters.PeakWorkingSetSize);
#else
    rusage usage{};
    if (getrusage(RUSAGE_SELF, &usage) != 0)
    {
        return 0;
    }
#if defined(Q_OS_DARWIN)
    return static_cast<qint64>(usage.ru_maxrss);
#else
    return static_cast<qint64>(usage.ru_maxrss) * 1024;
#endif
#endif
}

}

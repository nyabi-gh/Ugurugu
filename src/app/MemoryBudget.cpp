// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "app/MemoryBudget.hpp"

#include <QtGlobal>

#ifdef Q_OS_WIN
// std::clamp below would otherwise resolve against the windows.h macros.
#ifndef NOMINMAX
#define NOMINMAX
#endif
#ifndef WIN32_LEAN_AND_MEAN
#define WIN32_LEAN_AND_MEAN
#endif
#include <windows.h>
#elif defined(Q_OS_MACOS)
#include <sys/sysctl.h>
#include <sys/types.h>
#endif

#include <algorithm>

namespace ugurugu
{

namespace
{

qint64 readInstalledPhysicalBytes()
{
#ifdef Q_OS_WIN
    MEMORYSTATUSEX status{};
    status.dwLength = sizeof(status);
    if (GlobalMemoryStatusEx(&status) == 0)
    {
        return 0;
    }
    return static_cast<qint64>(status.ullTotalPhys);
#elif defined(Q_OS_MACOS)
    quint64 total = 0;
    size_t size = sizeof(total);
    if (sysctlbyname("hw.memsize", &total, &size, nullptr, 0) != 0
        || size != sizeof(total))
    {
        return 0;
    }
    return static_cast<qint64>(total);
#else
    return 0;
#endif
}

}

qint64 MemoryBudget::installedPhysicalBytes()
{
    static const qint64 bytes = readInstalledPhysicalBytes();
    return bytes;
}

int MemoryBudget::previewCacheKiB()
{
    const qint64 installed = installedPhysicalBytes();
    if (installed <= 0)
    {
        return minimumPreviewCacheKiB;
    }
    const qint64 share = installed / previewCacheMemoryShare / 1024LL;
    return static_cast<int>(std::clamp<qint64>(
        share, minimumPreviewCacheKiB, maximumPreviewCacheKiB));
}

}

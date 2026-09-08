// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "app/UpdateCheckPolicy.hpp"

namespace ugurugu
{

bool UpdateCheckPolicy::isAutomaticCheckDue(
    const QDateTime &lastCheck, const QDateTime &now)
{
    if (!now.isValid())
    {
        return false;
    }
    if (!lastCheck.isValid() || lastCheck > now)
    {
        return true;
    }
    return lastCheck.secsTo(now) >= automaticCheckIntervalSeconds;
}

}

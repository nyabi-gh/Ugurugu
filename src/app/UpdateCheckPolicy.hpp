// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#pragma once

#include <QDateTime>

namespace ugurugu
{

class UpdateCheckPolicy final
{
public:
    // Four opportunities a day, so a fix reaches a user who keeps the app
    // open all day without turning every launch into a request.
    static constexpr qint64 automaticCheckIntervalSeconds = 6LL * 60 * 60;

    static bool isAutomaticCheckDue(
        const QDateTime &lastCheck, const QDateTime &now);
};

}

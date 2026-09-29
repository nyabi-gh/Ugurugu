// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#pragma once

#include <QString>

namespace ugurugu
{

class Logging final
{
public:
    static void initialize();
    static void shutdown();
    static QString logFilePath();
};

}

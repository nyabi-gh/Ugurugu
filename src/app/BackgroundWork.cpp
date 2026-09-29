// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "app/BackgroundWork.hpp"

#include <QThreadPool>

namespace ugurugu
{

void joinDetachedBackgroundWork()
{
    QThreadPool::globalInstance()->waitForDone();
}

}

// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#pragma once

#include <QFutureWatcher>

namespace ugurugu
{

// QtConcurrent stores an exception thrown by a pool task and rethrows it from
// result(). Watchers are read inside finished handlers, so an escaping
// exception would unwind through the event dispatcher, past every handler that
// could report it, and terminate the process. The tasks read this way all
// signal failure through their result already - a null image, an evaluation
// that did not succeed - so a thrown one reports the same default.
template <typename T> T watchedFutureResult(QFutureWatcher<T> &watcher)
{
    try
    {
        return watcher.result();
    }
    catch (...)
    {
        return T();
    }
}

}

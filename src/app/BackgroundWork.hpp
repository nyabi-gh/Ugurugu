// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#pragma once

namespace ugurugu
{

// Waits for the detached tasks the UI leaves on the global thread pool - layer
// thumbnails, selection visibility - to finish. They outlive the widgets that
// started them and render through caches kept in function-local statics, which
// are destroyed on the way out of main while the global pool's own destructor
// runs later still. Call this once the last window is gone and before any
// process-wide teardown.
void joinDetachedBackgroundWork();

}

// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#pragma once

#include <QtGlobal>

namespace ugurugu
{

// The most memory the process has held resident so far. It only grows, so
// a rise across a step bounds what that step allocated and touched.
qint64 peakResidentBytes();

}

// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#pragma once

#include "document/Document.hpp"

namespace ugurugu
{
namespace DocumentBudget
{

// Each of these saturates one step past its DocumentLimits ceiling instead of
// counting the whole document, so a caller may only compare the result against
// that limit — the value is not a usable total once the limit is exceeded.

qsizetype totalPointCount(const Document &document);

qsizetype totalStrokeCount(const Document &document);

// Masks are counted once per distinct backing, matching what serialization
// writes after deduplication rather than the sum over strokes.
quint64 distinctClipMaskBytes(const Document &document);

}

}

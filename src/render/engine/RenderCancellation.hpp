// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#pragma once

#include <atomic>

namespace ugurugu
{
namespace render_detail
{

// Cancellation is cooperative and read-only: render entry points observe the
// flag at layer and framebuffer-operation boundaries, never per primitive, so
// the uncancelled hot path stays unchanged. A render that observes the flag
// set returns a null or invalid result, which the caller must discard.

inline bool isRenderCancelled(const std::atomic_bool *cancellation)
{
    return cancellation && cancellation->load(std::memory_order_relaxed);
}

}

}

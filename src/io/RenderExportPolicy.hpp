// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#pragma once

#include "document/Document.hpp"

namespace ugurugu
{

struct RenderExportMemoryEstimate final
{
    bool valid = false;
    long double hierarchyTransientBytes = 0.0L;
    long double workingBytes = 0.0L;
};

class RenderExportPolicy final
{
public:
    static RenderExportMemoryEstimate staticImage(const Document &document);
    static RenderExportMemoryEstimate animatedGif(const Document &document);
    static RenderExportMemoryEstimate animatedGifAtSize(
        const Document &document, const QSize &outputSize);
    static bool staticImageFitsMemoryBudget(const Document &document);
    static bool animatedGifFitsMemoryBudget(const Document &document);
    static bool animatedGifFitsMemoryBudget(
        const Document &document, const QSize &outputSize);
};

}

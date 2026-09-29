// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#pragma once

#include "render/RenderEngine.hpp"

namespace ugurugu
{

class StrokeCoverageRenderer final
{
public:
    static RenderEngine::StrokeCoveragePlan prepare(
        const Document &document, const Layer &layer);
    static QRect conservativeBounds(const Document &document,
        const Layer &layer,
        int strokeIndex,
        const RenderEngine::StrokeCoveragePlan &plan);
    static RenderEngine::StrokeCoverageRegion render(const Document &document,
        const Layer &layer,
        int strokeIndex,
        int frameIndex,
        const QRect &outputBounds,
        const RenderEngine::StrokeCoveragePlan &plan,
        RenderEngine::StrokeCoverageStats *stats);
};

}

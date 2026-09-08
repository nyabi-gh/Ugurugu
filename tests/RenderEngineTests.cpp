// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "TestSuites.hpp"
#include "support/RenderTestSuites.hpp"

namespace ugurugu
{

int runRenderEngineTests(int argc, char **argv)
{
    int result = 0;
    result |= runBrokenLineModelTests(argc, argv);
    result |= runClassicStrokeMotionTests(argc, argv);
    result |= runRenderPreviewTests(argc, argv);
    result |= runWobbleAnimationTests(argc, argv);
    result |= runLayerCompositionTests(argc, argv);
    result |= runStrokeCoverageTests(argc, argv);
    result |= runLayerSplitPreviewTests(argc, argv);
    result |= runLegacyRenderGoldenTests(argc, argv);
    result |= runFrozenFillMaskTests(argc, argv);
    result |= runMotionTimeModelTests(argc, argv);
    result |= runStrokeRenderingTests(argc, argv);
    result |= runSelectionPreviewTests(argc, argv);
    result |= runBrushRenderingTests(argc, argv);
    return result;
}

}

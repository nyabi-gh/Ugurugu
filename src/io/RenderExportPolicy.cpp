// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "io/RenderExportPolicy.hpp"

#include "app/MemoryBudget.hpp"
#include "render/LayerCompositionPlan.hpp"

#include <algorithm>

namespace ugurugu
{

namespace
{

bool fitsMemoryBudget(const RenderExportMemoryEstimate &estimate)
{
    return estimate.valid && estimate.workingBytes > 0.0L
           && estimate.workingBytes <= static_cast<long double>(
                  MemoryBudget::animationExportWorkingBytes);
}

}

RenderExportMemoryEstimate RenderExportPolicy::staticImage(
    const Document &document)
{
    RenderExportMemoryEstimate estimate;
    const LayerCompositionPlan plan = LayerCompositionPlan::build(document);
    const LayerCompositionMemoryEstimate hierarchy =
        plan.memoryEstimate(document.size);
    if (!hierarchy.valid)
    {
        return estimate;
    }
    const long double frameBytes =
        static_cast<long double>(hierarchy.bytesPerSurface);
    const int nativePaintSurfaces = plan.peakPaintLayerSurfaceCount();
    const long double maximumPaintSurfaceBytes = std::max(frameBytes,
        static_cast<long double>(plan.maximumPaintLayerBytesPerSurface()));
    estimate.valid = true;
    estimate.hierarchyTransientBytes =
        static_cast<long double>(hierarchy.peakBytes);
    estimate.workingBytes =
        estimate.hierarchyTransientBytes + frameBytes
        + maximumPaintSurfaceBytes
              * LayerCompositionPlan::paintOperationScratchSurfaceCount;
    if (nativePaintSurfaces > 0)
    {
        estimate.workingBytes +=
            (maximumPaintSurfaceBytes - frameBytes) * nativePaintSurfaces;
    }
    return estimate;
}

RenderExportMemoryEstimate RenderExportPolicy::animatedGif(
    const Document &document)
{
    return animatedGifAtSize(document, document.size);
}

RenderExportMemoryEstimate RenderExportPolicy::animatedGifAtSize(
    const Document &document, const QSize &outputSize)
{
    RenderExportMemoryEstimate estimate;
    if (outputSize.isEmpty())
    {
        return estimate;
    }
    const LayerCompositionPlan plan = LayerCompositionPlan::build(document);
    const LayerCompositionMemoryEstimate hierarchy =
        plan.memoryEstimate(outputSize);
    if (!hierarchy.valid || document.animationFrames <= 0)
    {
        return estimate;
    }
    const long double frameBytes =
        static_cast<long double>(hierarchy.bytesPerSurface);
    const long double frameCount =
        static_cast<long double>(document.animationFrames);
    const long double encoderBytes = frameBytes * frameCount * 3.0L;
    const int nativePaintSurfaces = plan.peakPaintLayerSurfaceCount();
    const long double maximumPaintSurfaceBytes = std::max(frameBytes,
        static_cast<long double>(plan.maximumPaintLayerBytesPerSurface()));
    estimate.hierarchyTransientBytes =
        static_cast<long double>(hierarchy.peakBytes);
    long double renderBytes =
        estimate.hierarchyTransientBytes + frameBytes * (frameCount - 1.0L);
    if (outputSize != document.size)
    {
        if (nativePaintSurfaces > 0)
        {
            renderBytes +=
                maximumPaintSurfaceBytes
                * static_cast<long double>(
                    nativePaintSurfaces
                    + LayerCompositionPlan::paintOperationScratchSurfaceCount);
        }
    }
    else
    {
        renderBytes +=
            maximumPaintSurfaceBytes
            * LayerCompositionPlan::paintOperationScratchSurfaceCount;
        if (nativePaintSurfaces > 0)
        {
            renderBytes +=
                (maximumPaintSurfaceBytes - frameBytes) * nativePaintSurfaces;
        }
    }
    estimate.valid = true;
    estimate.workingBytes = std::max(encoderBytes, renderBytes);
    return estimate;
}

bool RenderExportPolicy::staticImageFitsMemoryBudget(const Document &document)
{
    return fitsMemoryBudget(staticImage(document));
}

bool RenderExportPolicy::animatedGifFitsMemoryBudget(const Document &document)
{
    return fitsMemoryBudget(animatedGif(document));
}

bool RenderExportPolicy::animatedGifFitsMemoryBudget(
    const Document &document, const QSize &outputSize)
{
    return fitsMemoryBudget(animatedGifAtSize(document, outputSize));
}

}

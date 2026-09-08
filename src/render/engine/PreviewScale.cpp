// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "render/engine/PreviewScale.hpp"

namespace ugurugu
{
namespace render_detail
{

bool isNonUpscaledDisplaySize(const QSize &nativeSize, const QSize &outputSize)
{
    return nativeSize.isValid() && outputSize.isValid()
           && outputSize.width() <= nativeSize.width()
           && outputSize.height() <= nativeSize.height()
           && outputSize != nativeSize;
}

bool canReplayAtDisplayScale(const Document &document, const QSize &outputSize)
{
    return isNonUpscaledDisplaySize(document.size, outputSize);
}

void notePreviewImage(
    RenderEngine::ScaledRenderStats *stats, const QImage &image)
{
    if (!stats || image.isNull())
    {
        return;
    }
    const quint64 bytes = static_cast<quint64>(image.sizeInBytes());
    if (bytes > stats->largestIntermediateImageBytes)
    {
        stats->largestIntermediateImageBytes = bytes;
        stats->largestIntermediateImageSize = image.size();
    }
}

}

}

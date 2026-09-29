// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#pragma once

#include "document/Document.hpp"
#include "render/engine/PreviewScale.hpp"
#include "render/engine/RenderCancellation.hpp"

#include <QImage>
#include <QRect>
#include <QSize>

namespace ugurugu
{
namespace render_detail
{

// Replays framebuffer operations directly against a preview-sized layer,
// instead of rendering natively and scaling afterwards.
//
// The saving is the point: a 4K document previewed at window size never
// allocates a native surface. What makes it correct is conjugating each
// operation by the scale mapping (D * T * D^-1) and resampling the selection
// mask into display space, so the operation reads the same content it would
// have read natively. Every entry point re-checks that the framebuffer
// matches the mapping and returns false rather than replaying against a
// surface it was not built for.

struct DisplaySelectionMask
{
    QRect bounds;
    QImage mask;
};

bool buildDisplaySelectionMask(DisplaySelectionMask &result,
    const PixelSelectionOp &operation,
    const PreviewScaleMapping &mapping,
    const QSize &nativeCanvasSize,
    const QSize &displayCanvasSize,
    RenderEngine::ScaledRenderStats *stats);

bool applyPixelSelectionOperationAtDisplayScale(QImage &layerImage,
    const PixelSelectionOp &operation,
    const PreviewScaleMapping &mapping,
    const QSize &nativeCanvasSize,
    RenderEngine::ScaledRenderStats *stats);

bool applyReframeOperationAtDisplayScale(QImage &layerImage,
    const ReframeOp &operation,
    const PreviewScaleMapping &mapping,
    const QSize &nativeCanvasSize,
    RenderEngine::ScaledRenderStats *stats);

bool renderLayerOperationsAtDisplayScale(QImage &layerImage,
    const Document &document,
    const QVector<Stroke> &operations,
    int normalizedFrame,
    int frameCount,
    const QSize &initialCanvasSize,
    const PreviewScaleMapping &mapping,
    RenderEngine::ScaledRenderStats *stats,
    const std::atomic_bool *cancellation = nullptr);

}

}

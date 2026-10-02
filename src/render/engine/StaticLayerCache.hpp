// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#pragma once

#include "document/Document.hpp"

#include <QImage>

#include <atomic>
#include <functional>

namespace ugurugu
{
namespace render_detail
{

// True when every frame of the layer rasterizes to the same pixels: no stroke
// wobbles, no broken-line visibility cycle and no frame-seeded brush jitter.
// `document` is the layer's own document from documentForLayer().
bool isLayerFrameInvariant(const Document &document, const Layer &layer);

// Process-wide store of frame-invariant layer rasters, shared by every frame,
// worker and document snapshot. An entry is reused only while the layer still
// shares its stroke list and the document its raster assets with the snapshot
// the raster came from; implicit sharing guarantees identical content then,
// so the cache needs no invalidation from the outside. Concurrent misses on
// one layer wait for the first render instead of repeating it.
class StaticLayerCache final
{
public:
    struct Key
    {
        QUuid layerId;
        QSize outputSize;
        QSize documentSize;
        QSize initialCanvasSize;
        bool displayScale = false;
        // Frame-invariant is not wobble-free: a single-frame document still
        // displaces its strokes, so the motion inputs stay part of the key.
        int frameCount = 1;
        qreal wobbleAmount = 0.0;
        MotionSettings motion;

        bool operator==(const Key &) const = default;
    };

    // Returns the cached raster for `layer`, or runs `render` and keeps its
    // result. A null image from `render` is passed through and not kept.
    static QImage raster(const Document &document,
        const Layer &layer,
        const Key &key,
        const std::function<QImage()> &render,
        const std::atomic_bool *cancellation);

    static void setBudget(qint64 bytes);
    static qint64 budget();
    static qint64 residentBytes();
    static void clear();
};

}

}

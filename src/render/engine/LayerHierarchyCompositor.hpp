// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#pragma once

#include "document/Document.hpp"
#include "render/LayerCompositionPlan.hpp"
#include "render/engine/PreviewScale.hpp"
#include "render/engine/RenderCancellation.hpp"

#include <QImage>
#include <QPainter>

namespace ugurugu
{
namespace render_detail
{

// Composites the layer tree into one frame.
//
// Groups and clip-to-below need their members drawn onto a shared surface
// before the group's own opacity and blend mode apply, so the compositor
// walks a LayerCompositionPlan and borrows surfaces from a pool rather than
// compositing straight onto the output. Surface reuse is what keeps peak
// memory bounded on deep hierarchies, and the reuse counters in
// ScaledRenderStats are how that stays observable.

QPainter::CompositionMode compositionMode(LayerBlendMode mode);

void prepareLayerComposition(
    QPainter &painter, LayerBlendMode mode, qreal opacity);

// Renders the whole frame. `renderPaintLayer` produces one paint layer's
// framebuffer; it is a template parameter so a caller can substitute a cached
// or partially replayed layer without this file knowing how.

template <typename RenderPaintLayer>
QImage renderLayerHierarchy(const Document &document,
    const QSize &outputSize,
    const QColor &background,
    RenderPaintLayer renderPaintLayer,
    RenderEngine::ScaledRenderStats *stats,
    const std::atomic_bool *cancellation = nullptr)
{
    const LayerCompositionPlan plan = LayerCompositionPlan::build(document);
    if (!plan.isValid())
    {
        return {};
    }

    class SurfacePool final
    {
    public:
        SurfacePool(
            const QSize &size, RenderEngine::ScaledRenderStats *renderStats)
            : m_size(size)
            , m_stats(renderStats)
        {
        }

        QImage acquire(const QColor &fill)
        {
            QImage image;
            if (!m_free.isEmpty())
            {
                image = m_free.takeLast();
                if (m_stats)
                {
                    ++m_stats->hierarchySurfaceReuses;
                }
            }
            else
            {
                image = QImage(m_size, QImage::Format_ARGB32_Premultiplied);
                if (image.isNull())
                {
                    return {};
                }
                ++m_residentSurfaces;
                if (m_stats)
                {
                    ++m_stats->hierarchySurfaceAllocations;
                }
                notePeak();
            }
            image.fill(fill);
            notePreviewImage(m_stats, image);
            return image;
        }

        void trackExternal(const QImage &image)
        {
            if (image.isNull())
            {
                return;
            }
            ++m_residentSurfaces;
            if (m_stats)
            {
                ++m_stats->hierarchySurfaceAllocations;
            }
            notePreviewImage(m_stats, image);
            notePeak();
        }

        void recycle(QImage image)
        {
            if (image.isNull())
            {
                return;
            }
            if (m_free.isEmpty() && image.size() == m_size
                && image.format() == QImage::Format_ARGB32_Premultiplied
                && image.isDetached())
            {
                m_free.append(std::move(image));
                return;
            }
            --m_residentSurfaces;
        }

        void discardFreeSurfaces()
        {
            m_residentSurfaces -= m_free.size();
            m_free.clear();
        }

    private:
        void notePeak()
        {
            if (!m_stats)
            {
                return;
            }
            m_stats->hierarchyPeakSurfaceCount = std::max(
                m_stats->hierarchyPeakSurfaceCount, m_residentSurfaces);
            const quint64 bytes = static_cast<quint64>(m_residentSurfaces)
                                  * static_cast<quint64>(m_size.width())
                                  * static_cast<quint64>(m_size.height())
                                  * sizeof(quint32);
            m_stats->hierarchyPeakSurfaceBytes =
                std::max(m_stats->hierarchyPeakSurfaceBytes, bytes);
            m_stats->maximumEstimatedWorkingSetBytes =
                std::max(m_stats->maximumEstimatedWorkingSetBytes, bytes);
        }

        QSize m_size;
        RenderEngine::ScaledRenderStats *m_stats = nullptr;
        QVector<QImage> m_free;
        int m_residentSurfaces = 0;
    };

    struct CompositionFrame final
    {
        QImage result;
        QImage clippingBase;
        qreal clippingBaseOpacity = 0.0;
    };

    if (stats)
    {
        stats->hierarchyPlannedPeakSurfaceCount = plan.peakSurfaceCount();
    }
    SurfacePool surfacePool(outputSize, stats);
    QVector<CompositionFrame> frames;
    frames.reserve(document.layers.size() + 1);
    QImage root = surfacePool.acquire(background);
    if (root.isNull())
    {
        return {};
    }
    frames.append({std::move(root), {}, 0.0});

    const auto clearClippingBase = [&](CompositionFrame &frame)
    {
        surfacePool.recycle(std::move(frame.clippingBase));
        frame.clippingBaseOpacity = 0.0;
    };
    const auto composeLayer =
        [&](CompositionFrame &frame, const Layer &layer, QImage layerImage)
    {
        if (layer.clipToLayerBelow)
        {
            QPainter clipper(&layerImage);
            clipper.setCompositionMode(QPainter::CompositionMode_DestinationIn);
            clipper.setOpacity(frame.clippingBaseOpacity);
            clipper.drawImage(QPoint(0, 0), frame.clippingBase);
            clipper.end();
        }

        if (layer.clipToLayerBelow)
        {
            QPainter compositor(&frame.result);
            compositor.setRenderHint(QPainter::Antialiasing, false);
            prepareLayerComposition(compositor, layer.blendMode, layer.opacity);
            compositor.drawImage(QPoint(0, 0), layerImage);
            compositor.end();
            surfacePool.recycle(std::move(layerImage));
            return;
        }

        frame.clippingBase = std::move(layerImage);
        frame.clippingBaseOpacity = std::clamp(layer.opacity, 0.0, 1.0);
        QPainter compositor(&frame.result);
        compositor.setRenderHint(QPainter::Antialiasing, false);
        prepareLayerComposition(compositor, layer.blendMode, layer.opacity);
        compositor.drawImage(QPoint(0, 0), frame.clippingBase);
        compositor.end();
    };

    const QVector<LayerCompositionPlan::Operation> &operations =
        plan.operations();
    for (int operationIndex = 0; operationIndex < operations.size();)
    {
        if (isRenderCancelled(cancellation))
        {
            return {};
        }
        const LayerCompositionPlan::Operation &operation =
            operations[operationIndex];
        const Layer &layer = document.layers[operation.layerIndex];
        if (operation.type == LayerCompositionPlan::OperationType::EndGroup)
        {
            CompositionFrame child = frames.takeLast();
            clearClippingBase(child);
            QImage groupImage = std::move(child.result);
            composeLayer(frames.last(), layer, std::move(groupImage));
            ++operationIndex;
            continue;
        }

        CompositionFrame &frame = frames.last();
        if (!layer.visible || layer.opacity <= 0.0)
        {
            if (!layer.clipToLayerBelow)
            {
                clearClippingBase(frame);
            }
            operationIndex =
                operation.type
                        == LayerCompositionPlan::OperationType::BeginGroup
                    ? operation.matchingOperationIndex + 1
                    : operationIndex + 1;
            continue;
        }
        if (layer.clipToLayerBelow
            && (frame.clippingBase.isNull()
                || frame.clippingBaseOpacity <= 0.0))
        {
            operationIndex =
                operation.type
                        == LayerCompositionPlan::OperationType::BeginGroup
                    ? operation.matchingOperationIndex + 1
                    : operationIndex + 1;
            continue;
        }
        if (!layer.clipToLayerBelow)
        {
            clearClippingBase(frame);
        }

        if (operation.type == LayerCompositionPlan::OperationType::BeginGroup)
        {
            QImage group = surfacePool.acquire(Qt::transparent);
            if (group.isNull())
            {
                return {};
            }
            frames.append({std::move(group), {}, 0.0});
            ++operationIndex;
            continue;
        }

        surfacePool.discardFreeSurfaces();
        QImage layerImage = renderPaintLayer(layer);
        if (layerImage.isNull())
        {
            return {};
        }
        surfacePool.trackExternal(layerImage);
        composeLayer(frame, layer, std::move(layerImage));
        ++operationIndex;
    }

    if (frames.size() != 1)
    {
        return {};
    }
    clearClippingBase(frames.last());
    return std::move(frames.last().result);
}

QImage renderAtDisplayScale(const Document &document,
    int frameIndex,
    const QSize &outputSize,
    RenderEngine::ScaledRenderStats *stats,
    const std::atomic_bool *cancellation = nullptr);

QImage renderAtSize(const Document &document,
    int frameIndex,
    const QSize &outputSize,
    const std::atomic_bool *cancellation = nullptr);

// Renders outputRegion of the frame the two above produce whole, without
// paying for the pixels outside it. Layers whose strokes cannot be replayed
// against a partial framebuffer still render whole and are cropped, so the
// saving depends on the document; the result is identical either way. Null
// when outputSize is neither the document size nor a display-scale replay,
// because the native-exact resampler reads outside the region.
QImage renderRegion(const Document &document,
    int frameIndex,
    const QSize &outputSize,
    const QRect &outputRegion,
    RenderEngine::ScaledRenderStats *stats,
    const std::atomic_bool *cancellation = nullptr);

}

}

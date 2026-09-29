// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "render/LayerCompositionPlan.hpp"

#include "document/DocumentOperations.hpp"
#include "document/LayerHierarchy.hpp"

#include <QHash>
#include <QtMath>

#include <algorithm>
#include <limits>

namespace ugurugu
{

namespace
{

struct PlanBuildFrame final
{
    int groupLayerIndex = -1;
    qsizetype nextChild = 0;
    int beginOperationIndex = -1;
};

struct SurfaceEstimateFrame final
{
    bool hasClippingBase = false;
};

struct SurfaceEstimateResult final
{
    int peakSurfaceCount = 0;
    int peakPaintLayerSurfaceCount = 0;
    quint64 maximumPaintLayerBytesPerSurface = 0;
    QVector<QSize> paintLayerSurfaceSizes;
};

SurfaceEstimateResult estimatedSurfaceCounts(const Document &document,
    const QVector<LayerCompositionPlan::Operation> &ops)
{
    QVector<SurfaceEstimateFrame> frames;
    frames.append(SurfaceEstimateFrame{});
    int residentSurfaces = 1;
    int freeSurfaces = 0;
    int peakSurfaces = residentSurfaces;
    int peakPaintLayerSurfaces = 0;
    quint64 maximumPaintLayerBytesPerSurface = 0;
    QVector<QSize> paintLayerSurfaceSizes;

    const auto notePeak = [&]()
    {
        peakSurfaces = std::max(peakSurfaces, residentSurfaces);
    };
    const auto recycleSurface = [&]()
    {
        if (freeSurfaces == 0)
        {
            freeSurfaces = 1;
        }
        else
        {
            --residentSurfaces;
        }
    };
    const auto clearClippingBase = [&]()
    {
        SurfaceEstimateFrame &frame = frames.last();
        if (frame.hasClippingBase)
        {
            recycleSurface();
            frame.hasClippingBase = false;
        }
    };
    const auto discardFreeSurfaces = [&]()
    {
        residentSurfaces -= freeSurfaces;
        freeSurfaces = 0;
    };
    const auto acquirePooledSurface = [&]()
    {
        if (freeSurfaces > 0)
        {
            --freeSurfaces;
        }
        else
        {
            ++residentSurfaces;
            notePeak();
        }
    };

    for (int operationIndex = 0; operationIndex < ops.size();)
    {
        const LayerCompositionPlan::Operation &operation = ops[operationIndex];
        const Layer &layer = document.layers[operation.layerIndex];
        if (operation.type == LayerCompositionPlan::OperationType::EndGroup)
        {
            const SurfaceEstimateFrame child = frames.takeLast();
            if (child.hasClippingBase)
            {
                recycleSurface();
            }
            if (layer.clipToLayerBelow)
            {
                recycleSurface();
            }
            else
            {
                frames.last().hasClippingBase = true;
            }
            ++operationIndex;
            continue;
        }

        SurfaceEstimateFrame &frame = frames.last();
        if (!layer.visible || layer.opacity <= 0.0)
        {
            if (!layer.clipToLayerBelow)
            {
                clearClippingBase();
            }
            operationIndex =
                operation.type
                        == LayerCompositionPlan::OperationType::BeginGroup
                    ? operation.matchingOperationIndex + 1
                    : operationIndex + 1;
            continue;
        }
        if (layer.clipToLayerBelow && !frame.hasClippingBase)
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
            clearClippingBase();
        }

        if (operation.type == LayerCompositionPlan::OperationType::BeginGroup)
        {
            acquirePooledSurface();
            frames.append(SurfaceEstimateFrame{});
            ++operationIndex;
            continue;
        }

        discardFreeSurfaces();
        ++residentSurfaces;
        notePeak();
        // Composite-boundary sections hold their own surfaces until the last
        // boundary flattens them, so a merged layer briefly needs one surface
        // per boundary instead of one.
        int boundaries = 0;
        for (const Stroke &stroke : layer.strokes)
        {
            if (stroke.mode == StrokeMode::CompositeBoundary)
            {
                ++boundaries;
            }
        }
        if (!layer.strokes.isEmpty())
        {
            peakPaintLayerSurfaces =
                std::max(peakPaintLayerSurfaces, std::max(1, boundaries));
            const auto noteSurfaceSize = [&](const QSize &size)
            {
                if (size.isEmpty())
                {
                    return;
                }
                const quint64 bytes = static_cast<quint64>(size.width())
                                      * static_cast<quint64>(size.height())
                                      * sizeof(quint32);
                maximumPaintLayerBytesPerSurface =
                    std::max(maximumPaintLayerBytesPerSurface, bytes);
                paintLayerSurfaceSizes.append(size);
            };
            noteSurfaceSize(layer.initialCanvasSize.isValid()
                                ? layer.initialCanvasSize
                                : DocumentOperations::initialCanvasSize(
                                      layer.strokes, document.size));
            for (const Stroke &stroke : layer.strokes)
            {
                if (stroke.reframeOp)
                {
                    noteSurfaceSize(stroke.reframeOp->targetSize);
                }
            }
        }
        if (boundaries > 1)
        {
            residentSurfaces += boundaries - 1;
            notePeak();
            residentSurfaces -= boundaries - 1;
        }
        if (layer.clipToLayerBelow)
        {
            recycleSurface();
        }
        else
        {
            frame.hasClippingBase = true;
        }
        ++operationIndex;
    }

    if (frames.size() != 1)
    {
        return {};
    }
    if (frames.last().hasClippingBase)
    {
        recycleSurface();
    }
    return {peakSurfaces,
        peakPaintLayerSurfaces,
        maximumPaintLayerBytesPerSurface,
        paintLayerSurfaceSizes};
}

}

LayerCompositionPlan LayerCompositionPlan::build(const Document &document)
{
    LayerCompositionPlan plan;
    if (!analyzeLayerHierarchy(document).isValid())
    {
        return plan;
    }

    const int layerCount = static_cast<int>(document.layers.size());
    QHash<QUuid, int> layerIndexes;
    layerIndexes.reserve(layerCount);
    for (int index = 0; index < layerCount; ++index)
    {
        const Layer &layer = document.layers[index];
        if (!isValidLayerKind(layer.kind)
            || (layer.kind == LayerKind::Group
                && (layer.clipToLayerBelow || layer.reference
                    || !layer.strokes.isEmpty())))
        {
            return plan;
        }
        layerIndexes.insert(layer.id, index);
    }

    QVector<QVector<int>> children(layerCount + 1);
    const int rootIndex = layerCount;
    for (int index = 0; index < layerCount; ++index)
    {
        const QUuid parentId = document.layers[index].parentGroupId;
        const int parentIndex =
            parentId.isNull() ? rootIndex : layerIndexes.value(parentId, -1);
        if (parentIndex < 0)
        {
            return plan;
        }
        children[parentIndex].append(index);
    }

    plan.m_operations.reserve(static_cast<qsizetype>(layerCount) * 2);
    QVector<PlanBuildFrame> stack;
    stack.append(PlanBuildFrame{rootIndex, 0, -1});
    int visitedLayers = 0;
    while (!stack.isEmpty())
    {
        PlanBuildFrame &frame = stack.last();
        const QVector<int> &siblings = children[frame.groupLayerIndex];
        if (frame.nextChild >= siblings.size())
        {
            if (frame.beginOperationIndex >= 0)
            {
                const int endIndex = static_cast<int>(plan.m_operations.size());
                const int layerIndex =
                    plan.m_operations[frame.beginOperationIndex].layerIndex;
                plan.m_operations.append(Operation{OperationType::EndGroup,
                    layerIndex,
                    frame.beginOperationIndex});
                plan.m_operations[frame.beginOperationIndex]
                    .matchingOperationIndex = endIndex;
            }
            stack.removeLast();
            continue;
        }

        const int layerIndex = siblings[frame.nextChild++];
        ++visitedLayers;
        if (document.layers[layerIndex].kind == LayerKind::Group)
        {
            const int beginIndex = static_cast<int>(plan.m_operations.size());
            plan.m_operations.append(
                Operation{OperationType::BeginGroup, layerIndex, -1});
            stack.append(PlanBuildFrame{layerIndex, 0, beginIndex});
        }
        else
        {
            plan.m_operations.append(
                Operation{OperationType::PaintLayer, layerIndex, -1});
        }
    }

    if (visitedLayers != layerCount)
    {
        plan.m_operations.clear();
        return plan;
    }
    const SurfaceEstimateResult surfaceEstimate =
        estimatedSurfaceCounts(document, plan.m_operations);
    plan.m_peakSurfaceCount = surfaceEstimate.peakSurfaceCount;
    plan.m_peakPaintLayerSurfaceCount =
        surfaceEstimate.peakPaintLayerSurfaceCount;
    plan.m_maximumPaintLayerBytesPerSurface =
        surfaceEstimate.maximumPaintLayerBytesPerSurface;
    plan.m_paintLayerSurfaceSizes = surfaceEstimate.paintLayerSurfaceSizes;
    plan.m_valid = plan.m_peakSurfaceCount > 0;
    return plan;
}

bool LayerCompositionPlan::isValid() const
{
    return m_valid;
}

const QVector<LayerCompositionPlan::Operation> &
LayerCompositionPlan::operations() const
{
    return m_operations;
}

int LayerCompositionPlan::peakSurfaceCount() const
{
    return m_valid ? m_peakSurfaceCount : 0;
}

int LayerCompositionPlan::peakPaintLayerSurfaceCount() const
{
    return m_valid ? m_peakPaintLayerSurfaceCount : 0;
}

quint64 LayerCompositionPlan::maximumPaintLayerBytesPerSurface() const
{
    return m_valid ? m_maximumPaintLayerBytesPerSurface : 0;
}

quint64 LayerCompositionPlan::maximumPaintLayerBytesPerSurfaceAtSize(
    const QSize &documentSize, const QSize &outputSize) const
{
    if (!m_valid || documentSize.isEmpty() || outputSize.isEmpty())
    {
        return 0;
    }
    if (outputSize.width() > documentSize.width()
        || outputSize.height() > documentSize.height())
    {
        return 0;
    }
    const qreal horizontalScale =
        static_cast<qreal>(outputSize.width()) / documentSize.width();
    const qreal verticalScale =
        static_cast<qreal>(outputSize.height()) / documentSize.height();
    quint64 maximumBytes = 0;
    for (const QSize &nativeSize : m_paintLayerSurfaceSizes)
    {
        const quint64 width = static_cast<quint64>(
            std::max(1, qRound(nativeSize.width() * horizontalScale)));
        const quint64 height = static_cast<quint64>(
            std::max(1, qRound(nativeSize.height() * verticalScale)));
        maximumBytes = std::max(maximumBytes, width * height * sizeof(quint32));
    }
    return maximumBytes;
}

LayerCompositionMemoryEstimate LayerCompositionPlan::memoryEstimate(
    const QSize &surfaceSize) const
{
    LayerCompositionMemoryEstimate estimate;
    if (!m_valid || surfaceSize.isEmpty() || m_peakSurfaceCount <= 0)
    {
        return estimate;
    }

    constexpr quint64 bytesPerPixel = sizeof(quint32);
    const quint64 width = static_cast<quint64>(surfaceSize.width());
    const quint64 height = static_cast<quint64>(surfaceSize.height());
    const quint64 maximum = std::numeric_limits<quint64>::max();
    if (width > maximum / bytesPerPixel
        || height > maximum / (width * bytesPerPixel))
    {
        return estimate;
    }
    const quint64 bytesPerSurface = width * height * bytesPerPixel;
    if (static_cast<quint64>(m_peakSurfaceCount) > maximum / bytesPerSurface)
    {
        return estimate;
    }

    estimate.valid = true;
    estimate.peakSurfaceCount = m_peakSurfaceCount;
    estimate.bytesPerSurface = bytesPerSurface;
    estimate.peakBytes =
        static_cast<quint64>(m_peakSurfaceCount) * bytesPerSurface;
    return estimate;
}

}

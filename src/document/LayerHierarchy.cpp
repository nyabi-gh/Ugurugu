// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "document/LayerHierarchy.hpp"

#include "document/DocumentLimits.hpp"

#include <algorithm>

namespace ugurugu
{

bool LayerHierarchyAnalysis::isValid() const
{
    return m_issue == LayerHierarchyIssue::None;
}

LayerHierarchyIssue LayerHierarchyAnalysis::issue() const
{
    return m_issue;
}

int LayerHierarchyAnalysis::issueLayerIndex() const
{
    return m_issueLayerIndex;
}

int LayerHierarchyAnalysis::maximumDepth() const
{
    return m_maximumDepth;
}

const QVector<int> &LayerHierarchyAnalysis::depths() const
{
    return m_depths;
}

const QVector<int> &LayerHierarchyAnalysis::subtreeHeights() const
{
    return m_subtreeHeights;
}

int LayerHierarchyAnalysis::depth(const QUuid &layerId) const
{
    const auto index = m_layerIndexes.constFind(layerId);
    return isValid() && index != m_layerIndexes.cend() ? m_depths[*index] : -1;
}

int LayerHierarchyAnalysis::subtreeHeight(const QUuid &layerId) const
{
    const auto index = m_layerIndexes.constFind(layerId);
    return isValid() && index != m_layerIndexes.cend()
               ? m_subtreeHeights[*index]
               : -1;
}

bool LayerHierarchyAnalysis::isDescendantOf(
    const QUuid &layerId, const QUuid &ancestorLayerId) const
{
    const auto layer = m_layerIndexes.constFind(layerId);
    const auto ancestor = m_layerIndexes.constFind(ancestorLayerId);
    if (!isValid() || layer == m_layerIndexes.cend()
        || ancestor == m_layerIndexes.cend() || *layer == *ancestor)
    {
        return false;
    }
    return m_traversalBegins[*ancestor] < m_traversalBegins[*layer]
           && m_traversalEnds[*layer] <= m_traversalEnds[*ancestor];
}

LayerHierarchyAnalysis analyzeLayerHierarchy(const Document &document)
{
    LayerHierarchyAnalysis analysis;
    const int layerCount = static_cast<int>(document.layers.size());
    analysis.m_layerIndexes.reserve(layerCount);
    analysis.m_depths.fill(-1, layerCount);
    analysis.m_subtreeHeights.fill(0, layerCount);
    analysis.m_traversalBegins.fill(-1, layerCount);
    analysis.m_traversalEnds.fill(-1, layerCount);

    const auto reject = [&analysis](LayerHierarchyIssue issue, int index)
    {
        analysis.m_issue = issue;
        analysis.m_issueLayerIndex = index;
        return analysis;
    };

    for (int index = 0; index < layerCount; ++index)
    {
        const QUuid id = document.layers[index].id;
        if (id.isNull())
        {
            return reject(LayerHierarchyIssue::InvalidLayerId, index);
        }
        if (analysis.m_layerIndexes.contains(id))
        {
            return reject(LayerHierarchyIssue::DuplicateLayerId, index);
        }
        analysis.m_layerIndexes.insert(id, index);
    }

    QVector<QVector<int>> children(layerCount);
    QVector<int> parentIndexes(layerCount, -1);
    QVector<int> roots;
    roots.reserve(layerCount);
    for (int index = 0; index < layerCount; ++index)
    {
        const Layer &layer = document.layers[index];
        if (layer.parentGroupId.isNull())
        {
            roots.append(index);
            continue;
        }
        if (layer.parentGroupId == layer.id)
        {
            return reject(LayerHierarchyIssue::SelfParent, index);
        }
        const auto parent =
            analysis.m_layerIndexes.constFind(layer.parentGroupId);
        if (parent == analysis.m_layerIndexes.cend())
        {
            return reject(LayerHierarchyIssue::MissingParent, index);
        }
        if (document.layers[*parent].kind != LayerKind::Group)
        {
            return reject(LayerHierarchyIssue::ParentNotGroup, index);
        }
        parentIndexes[index] = *parent;
        children[*parent].append(index);
    }

    struct TraversalFrame
    {
        int layerIndex = -1;
        int nextChild = 0;
    };

    int traversalPosition = 0;
    QVector<TraversalFrame> stack;
    stack.reserve(layerCount);
    for (const int root : roots)
    {
        analysis.m_depths[root] = 0;
        analysis.m_traversalBegins[root] = traversalPosition++;
        stack.append({root, 0});
        while (!stack.isEmpty())
        {
            TraversalFrame &frame = stack.last();
            if (frame.nextChild < children[frame.layerIndex].size())
            {
                const int child = children[frame.layerIndex][frame.nextChild++];
                const int depth = analysis.m_depths[frame.layerIndex] + 1;
                analysis.m_depths[child] = depth;
                analysis.m_maximumDepth =
                    std::max(analysis.m_maximumDepth, depth);
                analysis.m_traversalBegins[child] = traversalPosition++;
                stack.append({child, 0});
                continue;
            }

            const int completed = frame.layerIndex;
            analysis.m_traversalEnds[completed] = traversalPosition;
            stack.removeLast();
            if (!stack.isEmpty())
            {
                const int parent = stack.last().layerIndex;
                analysis.m_subtreeHeights[parent] =
                    std::max(analysis.m_subtreeHeights[parent],
                        analysis.m_subtreeHeights[completed] + 1);
            }
        }
    }

    if (traversalPosition != layerCount)
    {
        const auto firstUnvisited =
            std::find(analysis.m_depths.cbegin(), analysis.m_depths.cend(), -1);
        int cycleIndex = static_cast<int>(
            std::distance(analysis.m_depths.cbegin(), firstUnvisited));
        for (int step = 0; step < layerCount; ++step)
        {
            cycleIndex = parentIndexes[cycleIndex];
        }
        return reject(LayerHierarchyIssue::Cycle, cycleIndex);
    }
    return analysis;
}

bool isLayerHierarchyDepthChangeAllowed(const LayerHierarchyAnalysis &current,
    const LayerHierarchyAnalysis &candidate)
{
    if (!current.isValid() || !candidate.isValid())
    {
        return false;
    }
    const int maximumAllowedDepth =
        current.maximumDepth() <= DocumentLimits::maximumLayerDepth
            ? DocumentLimits::maximumLayerDepth
            : current.maximumDepth();
    return candidate.maximumDepth() <= maximumAllowedDepth;
}

}

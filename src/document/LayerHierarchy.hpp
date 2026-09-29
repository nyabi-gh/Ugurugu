// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#pragma once

#include "document/Document.hpp"

#include <QHash>
#include <QVector>

namespace ugurugu
{

enum class LayerHierarchyIssue
{
    None,
    InvalidLayerId,
    DuplicateLayerId,
    MissingParent,
    ParentNotGroup,
    SelfParent,
    Cycle
};

class LayerHierarchyAnalysis final
{
public:
    bool isValid() const;
    LayerHierarchyIssue issue() const;
    int issueLayerIndex() const;
    int maximumDepth() const;
    const QVector<int> &depths() const;
    const QVector<int> &subtreeHeights() const;
    int depth(const QUuid &layerId) const;
    int subtreeHeight(const QUuid &layerId) const;
    bool isDescendantOf(
        const QUuid &layerId, const QUuid &ancestorLayerId) const;

private:
    LayerHierarchyIssue m_issue = LayerHierarchyIssue::None;
    int m_issueLayerIndex = -1;
    int m_maximumDepth = 0;
    QHash<QUuid, int> m_layerIndexes;
    QVector<int> m_depths;
    QVector<int> m_subtreeHeights;
    QVector<int> m_traversalBegins;
    QVector<int> m_traversalEnds;

    friend LayerHierarchyAnalysis analyzeLayerHierarchy(const Document &);
};

LayerHierarchyAnalysis analyzeLayerHierarchy(const Document &document);
bool isLayerHierarchyDepthChangeAllowed(const LayerHierarchyAnalysis &current,
    const LayerHierarchyAnalysis &candidate);

}

// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "render/LayerThumbnailRenderer.hpp"

#include "document/LayerHierarchy.hpp"
#include "render/RenderEngine.hpp"

#include <algorithm>

namespace ugurugu
{

QSize LayerThumbnailRenderer::renderSize(
    const QSize &documentSize, qreal devicePixelRatio)
{
    if (!documentSize.isValid() || devicePixelRatio <= 0.0)
    {
        return {};
    }
    const QSize deviceTarget(qRound(targetSize.width() * devicePixelRatio),
        qRound(targetSize.height() * devicePixelRatio));
    const QSize scaled = documentSize.scaled(deviceTarget, Qt::KeepAspectRatio);
    return QSize(std::max(1, scaled.width()), std::max(1, scaled.height()));
}

QImage LayerThumbnailRenderer::renderImage(
    const Document &document, const Layer &layer, qreal devicePixelRatio)
{
    const LayerHierarchyAnalysis hierarchy = analyzeLayerHierarchy(document);
    if (!hierarchy.isValid())
    {
        return {};
    }
    Document single = document;
    single.wobbleAmount = 0.0;
    single.layers.removeIf(
        [&hierarchy, &layer](const Layer &candidate)
        {
            return candidate.id != layer.id
                   && !hierarchy.isDescendantOf(candidate.id, layer.id);
        });
    if (Layer *root = single.layer(layer.id))
    {
        root->visible = true;
        root->opacity = 1.0;
        root->blendMode = LayerBlendMode::Normal;
        root->parentGroupId = QUuid();
        root->clipToLayerBelow = false;
    }

    return RenderEngine::renderScaled(
        single, 0, renderSize(single.size, devicePixelRatio));
}

}

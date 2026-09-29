// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#pragma once

#include "document/Document.hpp"
#include "render/StrokeRenderer.hpp"

#include <QHash>
#include <QImage>
#include <QPainterPath>

namespace ugurugu
{

class IncrementalStrokeRenderer final
{
public:
    struct Patch
    {
        QRect bounds;
        QImage layerImage;
    };

    struct Update
    {
        QVector<Patch> patches;
        quint64 sourcePointsProcessed = 0;
        quint64 tilesRendered = 0;
        quint64 pixelsRendered = 0;
        quint64 primitiveInstancesRendered = 0;
        quint64 cachedTileBytes = 0;
        bool geometryRebuilt = false;
        bool valid = false;
    };

    // Redraws only the tiles the appended points touch. The result must stay
    // pixel-identical to rendering the whole stroke from scratch, which is
    // what the coverage regression tests assert; anything that changes how a
    // primitive rasterizes has to invalidate the affected tiles rather than
    // blend over them. A different base layer or clip mask discards the cache,
    // both detected by QImage::cacheKey.
    Update update(const QImage &baseLayer,
        const Document &document,
        const Stroke &stroke,
        int frameIndex,
        const QSize &outputSize);
    // Requires layerImage to match the cached output size exactly and to be
    // ARGB32_Premultiplied; tiles are copied over it, not composited.
    bool applyTo(QImage &layerImage) const;
    quint64 cachedTileBytes() const;
    void clear();

private:
    static constexpr int tileEdge = 256;
    static constexpr qsizetype checkpointPrimitiveInterval = 64;

    struct TileCheckpoint
    {
        QImage image;
        int throughPrimitiveExclusive = 0;
    };

    QVector<QPoint> tilesForBounds(
        const QRectF &bounds, const Document &document) const;
    QRect tileBounds(const QPoint &tile) const;
    void removePrimitiveIndexesFrom(qsizetype first, QSet<QPoint> &dirtyTiles);
    void addPrimitiveIndexesFrom(const Stroke &stroke,
        const StrokeRenderer::PreparedStroke &prepared,
        qsizetype first,
        const Document &document,
        QSet<QPoint> &dirtyTiles);
    void advanceTileCheckpoint(const QImage &baseLayer,
        const Document &document,
        const Stroke &stroke,
        const StrokeRenderer::PreparedStroke &prepared,
        const QPoint &tile,
        qsizetype stablePrimitiveExclusive,
        quint64 &primitiveInstancesRendered);
    bool paintTilePrimitives(QImage &image,
        const QRect &bounds,
        const Document &document,
        const Stroke &stroke,
        const StrokeRenderer::PreparedStroke &prepared,
        const QVector<int> &primitiveIndexes);
    QImage renderTile(const QImage &baseLayer,
        const Document &document,
        const Stroke &stroke,
        const StrokeRenderer::PreparedStroke &prepared,
        const QPoint &tile,
        quint64 &primitiveInstancesRendered);

    StrokeRenderer::IncrementalGeometry m_geometry;
    QHash<QPoint, QVector<int>> m_primitivesByTile;
    QVector<QVector<QPoint>> m_tilesByPrimitive;
    QHash<QPoint, QImage> m_layerTiles;
    QHash<QPoint, TileCheckpoint> m_tileCheckpoints;
    QPainterPath m_clipPath;
    qint64 m_clipMaskKey = 0;
    qint64 m_baseLayerKey = 0;
    QSize m_documentSize;
    QSize m_outputSize;
};

}

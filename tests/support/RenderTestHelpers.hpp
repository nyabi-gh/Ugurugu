// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#pragma once

#include "brush/BrushPreset.hpp"
#include "document/DocumentController.hpp"
#include "document/DocumentLimits.hpp"
#include "document/SelectionOperation.hpp"
#include "document/SelectionVisibility.hpp"
#include "io/DocumentSerializer.hpp"
#include "render/ImageAffineTransformer.hpp"
#include "render/ImageResampler.hpp"
#include "render/IncrementalStrokeRenderer.hpp"
#include "render/PreviewRenderPolicy.hpp"
#include "render/RenderEngine.hpp"

#include <QElapsedTimer>
#include <QPainter>
#include <QRandomGenerator>
#include <QSet>
#include <QTransform>
#include <QtTest>

#include <algorithm>
#include <array>
#include <bit>
#include <cmath>
#include <functional>
#include <limits>
#include <numbers>

namespace ugurugu
{

inline Stroke makeStroke(StrokeMode mode,
    const QColor &color,
    qreal width,
    quint64 seed,
    const QVector<QPointF> &positions)
{
    Stroke stroke;
    stroke.mode = mode;
    stroke.color = color;
    stroke.width = width;
    stroke.seed = seed;
    for (const QPointF &position : positions)
    {
        stroke.points.append({position, 1.0});
    }
    return stroke;
}

inline Document animatedDocument()
{
    Document document = Document::createDefault(QSize(96, 72));
    document.animationFrames = 12;
    document.wobbleAmount = 6.0;
    document.layers.first().strokes.append(makeStroke(StrokeMode::Paint,
        QColor(25, 40, 70),
        9.0,
        0x123456789abcdef0ULL,
        {QPointF(8.0, 52.0),
            QPointF(24.0, 18.0),
            QPointF(48.0, 46.0),
            QPointF(72.0, 14.0),
            QPointF(88.0, 48.0)}));
    return document;
}

inline QImage rectangularMask(const QSize &size, const QRect &rect)
{
    QImage mask(size, QImage::Format_Grayscale8);
    mask.fill(0);
    const QRect clipped = rect.intersected(mask.rect());
    for (int y = clipped.top(); y <= clipped.bottom(); ++y)
    {
        std::fill(mask.scanLine(y) + clipped.left(),
            mask.scanLine(y) + clipped.right() + 1,
            255);
    }
    return mask;
}

inline QPainter::CompositionMode referenceCompositionMode(LayerBlendMode mode)
{
    switch (mode)
    {
    case LayerBlendMode::Multiply:
        return QPainter::CompositionMode_Multiply;
    case LayerBlendMode::Screen:
        return QPainter::CompositionMode_Screen;
    case LayerBlendMode::Overlay:
        return QPainter::CompositionMode_Overlay;
    case LayerBlendMode::Normal:
        return QPainter::CompositionMode_SourceOver;
    }
    return QPainter::CompositionMode_SourceOver;
}

inline QImage referenceHierarchyComposition(const Document &document,
    const QHash<QUuid, QImage> &paintLayers,
    const QSize &outputSize)
{
    std::function<QImage(const QUuid &, bool)> composeStack;
    composeStack = [&](const QUuid &parentGroupId, bool root)
    {
        QImage result(outputSize, QImage::Format_ARGB32_Premultiplied);
        if (result.isNull())
        {
            return QImage();
        }
        result.fill(root ? document.background : QColor(Qt::transparent));

        QImage clippingBase;
        qreal clippingBaseOpacity = 0.0;
        for (const Layer &layer : document.layers)
        {
            if (layer.parentGroupId != parentGroupId)
            {
                continue;
            }
            if (!layer.visible || layer.opacity <= 0.0)
            {
                if (!layer.clipToLayerBelow)
                {
                    clippingBase =
                        QImage(outputSize, QImage::Format_ARGB32_Premultiplied);
                    if (!clippingBase.isNull())
                    {
                        clippingBase.fill(Qt::transparent);
                    }
                    clippingBaseOpacity = 0.0;
                }
                continue;
            }

            QImage layerImage = layer.kind == LayerKind::Group
                                    ? composeStack(layer.id, false)
                                    : paintLayers.value(layer.id);
            if (layerImage.isNull())
            {
                return QImage();
            }
            if (layer.clipToLayerBelow)
            {
                if (clippingBase.isNull() || clippingBaseOpacity <= 0.0)
                {
                    continue;
                }
                QPainter clipper(&layerImage);
                clipper.setCompositionMode(
                    QPainter::CompositionMode_DestinationIn);
                clipper.setOpacity(clippingBaseOpacity);
                clipper.drawImage(QPoint(), clippingBase);
                clipper.end();
            }
            else
            {
                clippingBase = layerImage;
                clippingBaseOpacity = std::clamp(layer.opacity, 0.0, 1.0);
            }

            QPainter painter(&result);
            painter.setCompositionMode(
                referenceCompositionMode(layer.blendMode));
            painter.setOpacity(std::clamp(layer.opacity, 0.0, 1.0));
            painter.drawImage(QPoint(), layerImage);
            painter.end();
        }
        return result;
    };
    return composeStack({}, true);
}

inline QImage patternedSurface(
    const QSize &size, const QColor &first, const QColor &second)
{
    QImage image(size, QImage::Format_ARGB32_Premultiplied);
    if (image.isNull())
    {
        return {};
    }
    for (int y = 0; y < size.height(); ++y)
    {
        for (int x = 0; x < size.width(); ++x)
        {
            image.setPixelColor(x, y, (x + y) % 2 == 0 ? first : second);
        }
    }
    return image;
}

inline Document adversarialClippedHierarchy(const QSize &size, int groupCount)
{
    Document document = Document::createDefault(size);
    document.background = QColor(24, 32, 48);
    document.layers.clear();
    document.activeLayerId = QUuid();

    QUuid parentGroupId;
    for (int depth = 0; depth <= groupCount; ++depth)
    {
        Layer base;
        base.name = QStringLiteral("Base %1").arg(depth);
        base.parentGroupId = parentGroupId;
        base.initialCanvasSize = size;
        document.layers.append(base);
        if (document.activeLayerId.isNull())
        {
            document.activeLayerId = base.id;
        }
        if (depth == groupCount)
        {
            Layer clipped;
            clipped.name = QStringLiteral("Clipped");
            clipped.parentGroupId = parentGroupId;
            clipped.clipToLayerBelow = true;
            clipped.initialCanvasSize = size;
            document.layers.append(clipped);
            break;
        }

        Layer group;
        group.name = QStringLiteral("Group %1").arg(depth);
        group.kind = LayerKind::Group;
        group.parentGroupId = parentGroupId;
        group.initialCanvasSize = size;
        document.layers.append(group);
        parentGroupId = group.id;
    }
    return document;
}

inline QImage activeLayerPixels(const Document &document, int frameIndex = 0)
{
    QImage layerImage;
    if (document.layers.isEmpty()
        || !RenderEngine::renderStrokesOnLayer(layerImage,
            document,
            document.layers.first().strokes,
            frameIndex,
            document.size))
    {
        return {};
    }
    return layerImage;
}

inline QImage rasterSelectionResult(const QImage &before,
    const QImage &selection,
    const QPoint &delta,
    bool cutSource)
{
    if (before.isNull() || selection.size() != before.size()
        || selection.format() != QImage::Format_Grayscale8)
    {
        return {};
    }

    QImage payload(before.size(), QImage::Format_ARGB32_Premultiplied);
    payload.fill(Qt::transparent);
    QImage result = before;
    for (int y = 0; y < before.height(); ++y)
    {
        const QRgb *source =
            reinterpret_cast<const QRgb *>(before.constScanLine(y));
        QRgb *payloadLine = reinterpret_cast<QRgb *>(payload.scanLine(y));
        QRgb *resultLine = reinterpret_cast<QRgb *>(result.scanLine(y));
        const uchar *selectionLine = selection.constScanLine(y);
        for (int x = 0; x < before.width(); ++x)
        {
            if (selectionLine[x] < 128)
            {
                continue;
            }
            payloadLine[x] = source[x];
            if (cutSource)
            {
                resultLine[x] = 0;
            }
        }
    }

    QPainter painter(&result);
    painter.setCompositionMode(QPainter::CompositionMode_SourceOver);
    painter.drawImage(delta, payload);
    painter.end();
    return result;
}

inline QImage rasterSelectionTransformResult(const QImage &before,
    const QImage &selection,
    const QTransform &transform,
    bool cutSource,
    bool smooth = false)
{
    if (before.isNull() || selection.size() != before.size()
        || selection.format() != QImage::Format_Grayscale8
        || !transform.isInvertible())
    {
        return {};
    }

    QImage payload(before.size(), QImage::Format_ARGB32_Premultiplied);
    payload.fill(Qt::transparent);
    QImage result = before;
    for (int y = 0; y < before.height(); ++y)
    {
        const QRgb *source =
            reinterpret_cast<const QRgb *>(before.constScanLine(y));
        QRgb *payloadLine = reinterpret_cast<QRgb *>(payload.scanLine(y));
        QRgb *resultLine = reinterpret_cast<QRgb *>(result.scanLine(y));
        const uchar *selectionLine = selection.constScanLine(y);
        for (int x = 0; x < before.width(); ++x)
        {
            if (selectionLine[x] < 128)
            {
                continue;
            }
            payloadLine[x] = source[x];
            if (cutSource)
            {
                resultLine[x] = 0;
            }
        }
    }

    QPainter painter(&result);
    painter.setCompositionMode(QPainter::CompositionMode_SourceOver);
    painter.setRenderHint(QPainter::SmoothPixmapTransform, smooth);
    painter.setTransform(transform);
    painter.drawImage(QPointF(), payload);
    painter.end();
    return result;
}

inline QImage transformedSelectionMask(
    const QImage &selection, const QTransform &transform)
{
    if (selection.isNull() || selection.format() != QImage::Format_Grayscale8
        || !transform.isInvertible())
    {
        return {};
    }
    QImage transformed(selection.size(), QImage::Format_Grayscale8);
    if (transformed.isNull())
    {
        return {};
    }
    transformed.fill(0);
    QPainter painter(&transformed);
    painter.setRenderHint(QPainter::SmoothPixmapTransform, false);
    painter.setTransform(transform);
    painter.drawImage(QPointF(), selection);
    painter.end();
    return transformed;
}

inline QImage renderAdditionalStrokes(QImage base,
    const Document &document,
    const QVector<Stroke> &strokes,
    int frameIndex)
{
    if (!RenderEngine::renderStrokesOnLayer(
            base, document, strokes, frameIndex, base.size()))
    {
        return {};
    }
    return base;
}

inline QImage resizedRasterResult(
    const QImage &source, const QSize &targetSize, bool smooth)
{
    return ImageResampler::resample(source,
        targetSize,
        smooth ? SamplingMode::Smooth : SamplingMode::Nearest);
}

inline QImage qtResizedRasterResult(
    const QImage &source, const QSize &targetSize, bool smooth)
{
    QImage target(targetSize, QImage::Format_ARGB32_Premultiplied);
    if (source.isNull() || !targetSize.isValid() || target.isNull())
    {
        return {};
    }
    target.fill(Qt::transparent);
    QPainter painter(&target);
    painter.setRenderHint(QPainter::SmoothPixmapTransform, smooth);
    painter.drawImage(QRectF(QPointF(), QSizeF(targetSize)),
        source,
        QRectF(QPointF(), QSizeF(source.size())));
    painter.end();
    return target;
}

inline QImage qtAffineComposite(QImage target,
    const QRect &targetBounds,
    const QImage &source,
    const QRect &sourceBounds,
    const QTransform &transform,
    SamplingMode sampling)
{
    if (target.isNull() || target.size() != targetBounds.size()
        || source.isNull() || source.size() != sourceBounds.size())
    {
        return {};
    }
    QPainter painter(&target);
    painter.setRenderHint(
        QPainter::SmoothPixmapTransform, sampling == SamplingMode::Smooth);
    painter.setCompositionMode(QPainter::CompositionMode_SourceOver);
    QTransform targetTransform;
    targetTransform.translate(-targetBounds.left(), -targetBounds.top());
    painter.setWorldTransform(transform * targetTransform);
    painter.drawImage(sourceBounds.topLeft(), source);
    painter.end();
    return target;
}

inline QImage reframedRasterResult(
    const QImage &source, const QSize &targetSize, const QPoint &contentOffset)
{
    if (source.isNull() || !targetSize.isValid())
    {
        return {};
    }
    QImage target(targetSize, QImage::Format_ARGB32_Premultiplied);
    if (target.isNull())
    {
        return {};
    }
    target.fill(Qt::transparent);
    QPainter painter(&target);
    painter.setCompositionMode(QPainter::CompositionMode_Source);
    painter.drawImage(contentOffset, source);
    painter.end();
    return target;
}

inline QImage clearedSelectionResult(
    const QImage &before, const QImage &selection)
{
    if (before.isNull() || selection.size() != before.size()
        || selection.format() != QImage::Format_Grayscale8)
    {
        return {};
    }
    QImage result = before;
    for (int y = 0; y < result.height(); ++y)
    {
        QRgb *resultLine = reinterpret_cast<QRgb *>(result.scanLine(y));
        const uchar *selectionLine = selection.constScanLine(y);
        for (int x = 0; x < result.width(); ++x)
        {
            if (selectionLine[x] >= 128)
            {
                resultLine[x] = 0;
            }
        }
    }
    return result;
}

inline QImage expandedStrokeCoverage(
    const RenderEngine::StrokeCoverageRegion &coverage, const QSize &size)
{
    if (!coverage.valid || !size.isValid())
    {
        return {};
    }
    QImage expanded(size, QImage::Format_ARGB32_Premultiplied);
    if (expanded.isNull())
    {
        return {};
    }
    expanded.fill(Qt::transparent);
    if (!coverage.image.isNull())
    {
        QPainter painter(&expanded);
        painter.setCompositionMode(QPainter::CompositionMode_Source);
        painter.drawImage(coverage.bounds.topLeft(), coverage.image);
    }
    return expanded;
}

struct ImageDifference
{
    quint64 channelDifference = 0;
    quint64 comparedChannels = 0;
    qsizetype visiblyDifferentPixels = 0;
    int maximumChannelDifference = 0;
};

inline ImageDifference imageDifference(
    const QImage &left, const QImage &right, int visibleThreshold = 24)
{
    ImageDifference difference;
    if (left.isNull() || right.isNull() || left.size() != right.size())
    {
        difference.channelDifference = std::numeric_limits<quint64>::max();
        return difference;
    }
    difference.comparedChannels = static_cast<quint64>(left.width())
                                  * static_cast<quint64>(left.height()) * 4;
    for (int y = 0; y < left.height(); ++y)
    {
        for (int x = 0; x < left.width(); ++x)
        {
            const QColor a = left.pixelColor(x, y);
            const QColor b = right.pixelColor(x, y);
            const int red = std::abs(a.red() - b.red());
            const int green = std::abs(a.green() - b.green());
            const int blue = std::abs(a.blue() - b.blue());
            const int alpha = std::abs(a.alpha() - b.alpha());
            difference.maximumChannelDifference =
                std::max(difference.maximumChannelDifference,
                    std::max({red, green, blue, alpha}));
            difference.channelDifference +=
                static_cast<quint64>(red + green + blue + alpha);
            if (std::max({red, green, blue, alpha}) > visibleThreshold)
            {
                ++difference.visiblyDifferentPixels;
            }
        }
    }
    return difference;
}

inline ImageDifference storedImageDifference(
    const QImage &left, const QImage &right, int visibleThreshold = 24)
{
    ImageDifference difference;
    if (left.isNull() || right.isNull() || left.size() != right.size())
    {
        difference.channelDifference = std::numeric_limits<quint64>::max();
        return difference;
    }
    difference.comparedChannels = static_cast<quint64>(left.width())
                                  * static_cast<quint64>(left.height()) * 4;
    for (int y = 0; y < left.height(); ++y)
    {
        const auto *leftLine =
            reinterpret_cast<const QRgb *>(left.constScanLine(y));
        const auto *rightLine =
            reinterpret_cast<const QRgb *>(right.constScanLine(y));
        for (int x = 0; x < left.width(); ++x)
        {
            const int red = std::abs(qRed(leftLine[x]) - qRed(rightLine[x]));
            const int green =
                std::abs(qGreen(leftLine[x]) - qGreen(rightLine[x]));
            const int blue = std::abs(qBlue(leftLine[x]) - qBlue(rightLine[x]));
            const int alpha =
                std::abs(qAlpha(leftLine[x]) - qAlpha(rightLine[x]));
            const int maximum = std::max({red, green, blue, alpha});
            difference.channelDifference +=
                static_cast<quint64>(red + green + blue + alpha);
            difference.maximumChannelDifference =
                std::max(difference.maximumChannelDifference, maximum);
            if (maximum > visibleThreshold)
            {
                ++difference.visiblyDifferentPixels;
            }
        }
    }
    return difference;
}

}

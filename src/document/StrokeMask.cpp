// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "document/StrokeMask.hpp"

#include "document/SelectionOperation.hpp"

#include <QPainter>

#include <algorithm>
#include <cstring>

namespace ugurugu
{

QImage transformedMask(
    const QImage &source, const QSize &targetSize, const QTransform &transform)
{
    if (source.isNull())
    {
        return {};
    }
    QImage target(targetSize, QImage::Format_Grayscale8);
    if (target.isNull())
    {
        return {};
    }
    target.fill(0);
    QPainter painter(&target);
    painter.setRenderHint(QPainter::SmoothPixmapTransform, false);
    painter.setTransform(transform);
    painter.drawImage(QPointF(), source);
    painter.end();
    return target;
}

bool transformMask(QImage &mask,
    const QSize &targetSize,
    const QTransform &transform,
    QHash<qint64, QImage> &cache)
{
    if (mask.isNull())
    {
        return true;
    }
    const qint64 key = mask.cacheKey();
    const auto cached = cache.constFind(key);
    if (cached != cache.cend())
    {
        mask = cached.value();
        return true;
    }
    const QImage transformed = transformedMask(mask, targetSize, transform);
    if (transformed.isNull())
    {
        return false;
    }
    cache.insert(key, transformed);
    mask = transformed;
    return true;
}

QImage maskedPart(
    const QImage &source, const QImage &selection, bool insideSelection)
{
    if (selection.isNull() || selection.format() != QImage::Format_Grayscale8
        || (!source.isNull()
            && (source.size() != selection.size()
                || source.format() != QImage::Format_Grayscale8)))
    {
        return {};
    }

    QImage result(selection.size(), QImage::Format_Grayscale8);
    if (result.isNull())
    {
        return {};
    }
    result.fill(0);
    for (int y = 0; y < selection.height(); ++y)
    {
        const uchar *sourceLine =
            source.isNull() ? nullptr : source.constScanLine(y);
        const uchar *selectionLine = selection.constScanLine(y);
        uchar *resultLine = result.scanLine(y);
        for (int x = 0; x < selection.width(); ++x)
        {
            const bool sourceContains = !sourceLine || sourceLine[x] >= 128;
            const bool selectionContains = selectionLine[x] >= 128;
            resultLine[x] = sourceContains
                                    && (insideSelection ? selectionContains
                                                        : !selectionContains)
                                ? 255
                                : 0;
        }
    }
    return result;
}

bool maskHasContent(const QImage &mask)
{
    if (mask.isNull() || mask.format() != QImage::Format_Grayscale8)
    {
        return false;
    }
    for (int y = 0; y < mask.height(); ++y)
    {
        const uchar *line = mask.constScanLine(y);
        if (std::any_of(line,
                line + mask.width(),
                [](uchar value)
                {
                    return value >= 128;
                }))
        {
            return true;
        }
    }
    return false;
}

bool maskHasContent(
    const QImage &mask, const std::optional<QRect> &visibilityClip)
{
    if (mask.isNull() || mask.format() != QImage::Format_Grayscale8)
    {
        return false;
    }
    QRect bounds = mask.rect();
    if (visibilityClip)
    {
        bounds = bounds.intersected(*visibilityClip);
    }
    if (bounds.isEmpty())
    {
        return false;
    }
    for (int y = bounds.top(); y <= bounds.bottom(); ++y)
    {
        const uchar *line = mask.constScanLine(y);
        if (std::any_of(line + bounds.left(),
                line + bounds.right() + 1,
                [](uchar value)
                {
                    return value >= 128;
                }))
        {
            return true;
        }
    }
    return false;
}

bool masksIntersect(const QImage &first,
    const QImage &second,
    const std::optional<QRect> &visibilityClip)
{
    if ((!first.isNull() && first.format() != QImage::Format_Grayscale8)
        || (!second.isNull() && second.format() != QImage::Format_Grayscale8)
        || (!first.isNull() && !second.isNull()
            && first.size() != second.size()))
    {
        return false;
    }

    const QSize size = !first.isNull() ? first.size() : second.size();
    if (!size.isValid())
    {
        return false;
    }
    QRect bounds(QPoint(), size);
    if (visibilityClip)
    {
        bounds = bounds.intersected(*visibilityClip);
    }
    if (bounds.isEmpty())
    {
        return false;
    }
    for (int y = bounds.top(); y <= bounds.bottom(); ++y)
    {
        const uchar *firstLine =
            first.isNull() ? nullptr : first.constScanLine(y);
        const uchar *secondLine =
            second.isNull() ? nullptr : second.constScanLine(y);
        for (int x = bounds.left(); x <= bounds.right(); ++x)
        {
            if ((!firstLine || firstLine[x] >= 128)
                && (!secondLine || secondLine[x] >= 128))
            {
                return true;
            }
        }
    }
    return false;
}

std::optional<QImage> materializedVisibilityMask(
    const Stroke &stroke, const QSize &canvasSize)
{
    if (!canvasSize.isValid()
        || (!stroke.clipMask.isNull()
            && (stroke.clipMask.size() != canvasSize
                || stroke.clipMask.format() != QImage::Format_Grayscale8)))
    {
        return std::nullopt;
    }

    const QRect canvasRect(QPoint(), canvasSize);
    std::optional<QRect> clipRect = stroke.visibilityClip;
    if (clipRect)
    {
        *clipRect = clipRect->intersected(canvasRect);
        if (clipRect->isEmpty())
        {
            QImage empty(canvasSize, QImage::Format_Grayscale8);
            if (empty.isNull())
            {
                return std::nullopt;
            }
            empty.fill(0);
            return empty;
        }
    }

    if (!clipRect)
    {
        return stroke.clipMask;
    }

    QImage materialized(canvasSize, QImage::Format_Grayscale8);
    if (materialized.isNull())
    {
        return std::nullopt;
    }
    materialized.fill(0);
    for (int y = clipRect->top(); y <= clipRect->bottom(); ++y)
    {
        uchar *target = materialized.scanLine(y) + clipRect->left();
        if (stroke.clipMask.isNull())
        {
            std::fill(
                target, target + clipRect->width(), static_cast<uchar>(255));
        }
        else
        {
            std::memcpy(target,
                stroke.clipMask.constScanLine(y) + clipRect->left(),
                static_cast<std::size_t>(clipRect->width()));
        }
    }
    return materialized;
}

bool canonicalizeStrokeVisibility(Stroke &stroke, const QSize &canvasSize)
{
    const QRect canvasRect(QPoint(), canvasSize);
    if (!canvasSize.isValid())
    {
        return false;
    }
    if (stroke.mode == StrokeMode::PixelSelection
        || stroke.mode == StrokeMode::Reframe || stroke.pixelSelectionOp
        || stroke.reframeOp)
    {
        return false;
    }

    if (stroke.visibilityClip)
    {
        stroke.visibilityClip = stroke.visibilityClip->intersected(canvasRect);
        if (stroke.visibilityClip->isEmpty())
        {
            return false;
        }
        if (*stroke.visibilityClip == canvasRect)
        {
            stroke.visibilityClip.reset();
        }
    }

    if (!stroke.clipMask.isNull())
    {
        if (stroke.clipMask.size() != canvasSize
            || stroke.clipMask.format() != QImage::Format_Grayscale8)
        {
            return false;
        }
        bool full = true;
        for (int y = 0; y < stroke.clipMask.height() && full; ++y)
        {
            const uchar *line = stroke.clipMask.constScanLine(y);
            full = std::all_of(line,
                line + stroke.clipMask.width(),
                [](uchar value)
                {
                    return value >= 128;
                });
        }
        if (full)
        {
            stroke.clipMask = {};
        }
    }

    const std::optional<QImage> visibility =
        materializedVisibilityMask(stroke, canvasSize);
    if (!visibility)
    {
        return false;
    }
    if (!visibility->isNull() && !maskHasContent(*visibility))
    {
        return false;
    }

    if (stroke.mode != StrokeMode::Fill)
    {
        return !stroke.fillCoverage;
    }
    if (!stroke.fillMask.isNull() && stroke.fillCoverage)
    {
        return false;
    }
    const QImage packedCoverage =
        stroke.fillCoverage ? unpackBinaryMask(*stroke.fillCoverage) : QImage();
    const QImage &fillMask =
        stroke.fillCoverage ? packedCoverage : stroke.fillMask;
    if (fillMask.isNull())
    {
        return !stroke.fillCoverage;
    }
    if (fillMask.size() != canvasSize
        || fillMask.format() != QImage::Format_Grayscale8)
    {
        return false;
    }
    return visibility->isNull() ? maskHasContent(fillMask)
                                : masksIntersect(fillMask, *visibility);
}

}

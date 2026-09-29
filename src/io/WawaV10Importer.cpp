// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "io/WawaV10Importer.hpp"

#include "brush/BrushPreset.hpp"
#include "document/DocumentLimits.hpp"
#include "document/DocumentOperations.hpp"
#include "document/FrozenFillMask.hpp"
#include "io/serializer/RasterAssetTable.hpp"

#include <algorithm>
#include <cmath>
#include <utility>
#include <variant>

namespace ugurugu
{

namespace
{

void setError(QString *error, const QString &message)
{
    if (error)
    {
        *error = message;
    }
}

QColor importedColor(QColor color, int opacity)
{
    const qreal normalizedOpacity =
        static_cast<qreal>(std::clamp(opacity, 0, 100)) / 100.0;
    color.setAlpha(
        qRound(static_cast<qreal>(color.alphaF()) * normalizedOpacity * 255.0));
    return color;
}

BrushSettings importedBrush(bool airbrush, bool wobble)
{
    BrushSettings settings;
    settings.antialiasing = true;
    if (airbrush)
    {
        if (const BrushPreset *preset =
                BrushPresetCatalog::find(QStringLiteral("soft-airbrush")))
        {
            settings = preset->settings;
        }
        settings.opacity = 1.0;
    }
    settings.wobbleScale = wobble ? 1.0 : 0.0;
    return settings;
}

bool hasVisiblePixels(const QImage &image)
{
    const QImage canonical =
        image.convertToFormat(QImage::Format_ARGB32_Premultiplied);
    for (int y = 0; y < canonical.height(); ++y)
    {
        const auto *line =
            reinterpret_cast<const QRgb *>(canonical.constScanLine(y));
        if (std::any_of(line,
                line + canonical.width(),
                [](QRgb pixel)
                {
                    return qAlpha(pixel) != 0;
                }))
        {
            return true;
        }
    }
    return false;
}

Stroke importedStroke(const WawaStroke &source,
    StrokeMode mode,
    bool wobble,
    WawaImportSummary &summary)
{
    Stroke stroke;
    stroke.seed = static_cast<quint32>(source.seed);
    stroke.mode = mode;
    stroke.color = importedColor(source.color, source.opacity);
    stroke.width = std::clamp<qreal>(source.size,
        DocumentLimits::minimumStrokeWidth,
        DocumentLimits::maximumStrokeWidth);
    if (!qFuzzyCompare(stroke.width, qreal(source.size)))
    {
        ++summary.clampedWidths;
    }
    stroke.brush = importedBrush(source.airbrush, wobble);
    stroke.points.reserve(source.points.size());
    for (const QPointF &point : source.points)
    {
        stroke.points.append({point, 1.0});
    }
    return stroke;
}

std::optional<Stroke> importedFill(
    const WawaFill &source, const QSize &canvasSize, bool antialiasing)
{
    const std::optional<PackedMaskRegion> coverage =
        FrozenFillMask::packedFromPolygon(canvasSize, source.points);
    if (!coverage)
    {
        return std::nullopt;
    }
    Stroke fill;
    fill.seed = static_cast<quint32>(source.seed);
    fill.mode = StrokeMode::Fill;
    fill.color = importedColor(source.color, source.opacity);
    fill.brush.antialiasing = antialiasing;
    fill.brush.wobbleScale = 0.0;
    fill.points = {{source.points.first(), 1.0}};
    fill.fillCoverage = *coverage;
    return fill;
}

struct OrderedOperation
{
    int order = 0;
    int sequence = 0;
    std::variant<const WawaStroke *, const WawaFill *> source;
    StrokeMode mode = StrokeMode::Paint;
};

QVector<OrderedOperation> orderedOperations(const WawaLayer &layer)
{
    QVector<OrderedOperation> operations;
    operations.reserve(layer.paintStrokes.size() + layer.eraserStrokes.size()
                       + layer.fills.size());
    int sequence = 0;
    for (const WawaFill &fill : layer.fills)
    {
        operations.append({fill.order, sequence++, &fill, StrokeMode::Fill});
    }
    for (const WawaStroke &stroke : layer.paintStrokes)
    {
        operations.append(
            {stroke.order, sequence++, &stroke, StrokeMode::Paint});
    }
    for (const WawaStroke &eraser : layer.eraserStrokes)
    {
        operations.append(
            {eraser.order, sequence++, &eraser, StrokeMode::Erase});
    }
    std::stable_sort(operations.begin(),
        operations.end(),
        [](const OrderedOperation &left, const OrderedOperation &right)
        {
            return left.order < right.order
                   || (left.order == right.order
                       && left.sequence < right.sequence);
        });
    return operations;
}

void applyMotionSettings(Document &document, const WawaSettings &source)
{
    document.wobbleAmount = std::clamp(source.wobbleAmount * 0.35,
        DocumentLimits::minimumWobbleAmount,
        DocumentLimits::maximumWobbleAmount);
    document.motion.style =
        source.wobbleMode == 2 ? MotionStyle::Stepped : MotionStyle::Smooth;
    const int poseCount =
        source.wobbleMode == 2
            ? qCeil(document.animationFrames
                    / qreal(std::max(1, source.wobbleHoldFrames)))
            : source.wobbleSpeed;
    document.motion.poseCount = std::clamp(poseCount,
        DocumentLimits::minimumMotionPoseCount,
        document.animationFrames);
    document.motion.detail = std::clamp(source.wobbleDetail,
        DocumentLimits::minimumMotionDetail,
        DocumentLimits::maximumMotionDetail);
    document.motion.linked = source.linkedWiggle ? 1.0 : 0.0;
    document.motion.randomness =
        std::clamp(source.wobbleRandomness / 100.0, 0.0, 1.0);
    document.motion.brokenLine = source.brokenLine;
    document.motion.breakAmount =
        std::clamp(source.breakAmount / 100.0, 0.0, 1.0);
    document.motion.breakRange = std::clamp<qreal>(source.breakRange,
        DocumentLimits::minimumBreakRange,
        DocumentLimits::maximumBreakRange);
}

}

std::optional<WawaImportResult> WawaV10Importer::import(
    const QByteArray &data, QString *error)
{
    const std::optional<WawaProject> project = WawaV10Reader::read(data, error);
    return project ? convert(*project, error) : std::nullopt;
}

std::optional<WawaImportResult> WawaV10Importer::convert(
    const WawaProject &project, QString *error)
{
    if (project.layers.isEmpty()
        || project.layers.size() > DocumentLimits::maximumLayers
        || project.settings.activeLayer < 0
        || project.settings.activeLayer >= project.layers.size())
    {
        setError(
            error, QStringLiteral("The .wawa project cannot be imported."));
        return std::nullopt;
    }

    WawaImportResult result;
    Document &document = result.document;
    document.size = project.canvasSize;
    document.background = project.settings.backgroundColor;
    applyMotionSettings(document, project.settings);
    serializer_detail::RasterAssetTable assets;
    result.summary.layers = static_cast<int>(project.layers.size());
    document.layers.reserve(project.layers.size());

    for (int layerIndex = 0; layerIndex < project.layers.size(); ++layerIndex)
    {
        const WawaLayer &sourceLayer = project.layers[layerIndex];
        Layer layer;
        layer.name = sourceLayer.name.trimmed().left(
            DocumentLimits::maximumLayerNameLength);
        if (layer.name.isEmpty())
        {
            layer.name = QStringLiteral("Layer %1").arg(layerIndex + 1);
        }
        layer.visible = sourceLayer.visible;
        layer.initialCanvasSize = project.canvasSize;

        if (hasVisiblePixels(sourceLayer.baseImage))
        {
            const serializer_detail::RasterAssetRegistrationResult registered =
                assets.registerImage(sourceLayer.baseImage);
            if (registered.status
                    != serializer_detail::RasterAssetRegistrationStatus::
                        Registered
                && registered.status
                       != serializer_detail::RasterAssetRegistrationStatus::
                           Reused)
            {
                setError(error,
                    QStringLiteral(
                        "The .wawa base images exceed the image asset limit."));
                return std::nullopt;
            }
            const serializer_detail::RasterAssetEntry &entry =
                assets.entries().value(registered.id);
            document.rasterAssets.insert(registered.id,
                RasterAsset{entry.id, entry.size, entry.compressedRgba});
            Stroke image;
            image.mode = StrokeMode::Image;
            image.points.clear();
            image.imageOp =
                ImageOp{registered.id, QTransform(), SamplingMode::Nearest};
            layer.strokes.append(std::move(image));
            ++result.summary.baseImages;
        }

        for (const OrderedOperation &operation : orderedOperations(sourceLayer))
        {
            if (operation.mode == StrokeMode::Fill)
            {
                const WawaFill &source =
                    *std::get<const WawaFill *>(operation.source);
                const std::optional<Stroke> fill = importedFill(source,
                    project.canvasSize,
                    project.settings.bucketAntialias);
                if (!fill)
                {
                    ++result.summary.skippedOperations;
                    continue;
                }
                layer.strokes.append(*fill);
                ++result.summary.polygonFills;
                continue;
            }
            const WawaStroke &source =
                *std::get<const WawaStroke *>(operation.source);
            if (source.points.isEmpty())
            {
                ++result.summary.skippedOperations;
                continue;
            }
            const bool wobble = operation.mode == StrokeMode::Paint
                                || project.settings.wobbleEraser;
            layer.strokes.append(
                importedStroke(source, operation.mode, wobble, result.summary));
            if (operation.mode == StrokeMode::Paint)
            {
                ++result.summary.paintStrokes;
            }
            else
            {
                ++result.summary.eraserStrokes;
            }
        }

        document.layers.append(std::move(layer));
    }
    document.activeLayerId = document.layers[project.settings.activeLayer].id;
    if (!DocumentOperations::normalizeAndValidate(document))
    {
        setError(
            error, QStringLiteral("The converted .wawa project is not valid."));
        return std::nullopt;
    }
    return result;
}

}

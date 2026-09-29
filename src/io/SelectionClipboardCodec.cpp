// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "io/SelectionClipboardCodec.hpp"

#include "document/DocumentLimits.hpp"
#include "document/SelectionOperation.hpp"
#include "io/DocumentSerializer.hpp"
#include "render/RenderEngine.hpp"

#include <QMimeData>

#include <utility>

namespace ugurugu
{

namespace
{

void setCodecError(QString *error, const QString &message)
{
    if (error)
    {
        *error = message;
    }
}

bool maskCoversAnyPixelIn(const QImage &mask, const QRect &region)
{
    const QRect bounds = region.intersected(QRect(QPoint(), mask.size()));
    for (int y = bounds.top(); y <= bounds.bottom(); ++y)
    {
        const uchar *line = mask.constScanLine(y);
        for (int x = bounds.left(); x <= bounds.right(); ++x)
        {
            if (line[x] >= 128)
            {
                return true;
            }
        }
    }
    return false;
}

// Leaves out the strokes that cannot put a pixel inside the selection at any
// frame. The appended clip operation hides them, but hiding is not the same as
// not carrying them: the payload used to be the whole source layer, so a copy
// of a small selection published every stroke and every image asset of that
// layer to the clipboard, recoverable by anyone who dropped the clip.
//
// Reach comes from the coverage plan, which folds in the wobble margin and
// follows the later framebuffer operations, so a stroke that only enters the
// selection after being moved there survives. Strokes are dropped only on
// positive evidence — a non-empty reach that misses every selected pixel — so
// an unanalysable layer keeps everything and stays correct through the clip.
void dropStrokesOutsideSelection(
    const Document &document, Layer &layer, const QImage &selectionMask)
{
    const RenderEngine::StrokeCoveragePlan plan =
        RenderEngine::prepareStrokeCoverage(document, layer);
    if (!plan.valid)
    {
        return;
    }
    QVector<Stroke> kept;
    kept.reserve(layer.strokes.size());
    for (int index = 0; index < layer.strokes.size(); ++index)
    {
        const Stroke &stroke = layer.strokes[index];
        const bool paintsPixels = stroke.mode == StrokeMode::Paint
                                  || stroke.mode == StrokeMode::Erase
                                  || stroke.mode == StrokeMode::Fill
                                  || stroke.mode == StrokeMode::Image;
        if (paintsPixels)
        {
            const QRect reach = RenderEngine::conservativeStrokeCoverageBounds(
                document, layer, index, plan);
            if (!reach.isEmpty() && !maskCoversAnyPixelIn(selectionMask, reach))
            {
                continue;
            }
        }
        kept.append(stroke);
    }
    layer.strokes = std::move(kept);
}

}

QString SelectionClipboardCodec::mimeType()
{
    return QStringLiteral("application/x-ugurugu-selection+json");
}

QStringList SelectionClipboardCodec::legacyMimeTypes()
{
    // Payloads published by an earlier name of the application. A rename makes
    // the previous build a separate application that can run at the same time,
    // so a copy taken there is still worth accepting on paste.
    return {QStringLiteral("application/x-waglewaglepaint-selection+json")};
}

QString SelectionClipboardCodec::availableMimeType(const QMimeData &mimeData)
{
    if (mimeData.hasFormat(mimeType()))
    {
        return mimeType();
    }
    for (const QString &legacy : legacyMimeTypes())
    {
        if (mimeData.hasFormat(legacy))
        {
            return legacy;
        }
    }
    return {};
}

std::optional<SelectionClipboardCodec::Copy> SelectionClipboardCodec::makeCopy(
    const Document &document,
    const QUuid &layerId,
    const QImage &selectionMask,
    int frameIndex,
    QString *error)
{
    const Layer *source = document.layer(layerId);
    if (!source || source->kind != LayerKind::Paint)
    {
        setCodecError(error, tr("The selected layer cannot be copied."));
        return std::nullopt;
    }
    if (selectionMask.isNull() || selectionMask.size() != document.size
        || selectionMask.format() != QImage::Format_Grayscale8)
    {
        setCodecError(error, tr("The selection could not be copied."));
        return std::nullopt;
    }
    const std::optional<PackedMaskRegion> packedSelection =
        packBinaryMask(selectionMask);
    if (!packedSelection)
    {
        setCodecError(error, tr("The selection is empty."));
        return std::nullopt;
    }

    Layer copy = *source;
    copy.parentGroupId = QUuid();
    copy.clipToLayerBelow = false;
    copy.visible = true;
    copy.reference = false;
    dropStrokesOutsideSelection(document, copy, selectionMask);

    QMap<QString, RasterAsset> rasterAssets;
    for (const Stroke &stroke : std::as_const(copy.strokes))
    {
        if (!stroke.imageOp)
        {
            continue;
        }
        const auto asset =
            document.rasterAssets.constFind(stroke.imageOp->assetId);
        if (asset == document.rasterAssets.cend())
        {
            setCodecError(error, tr("The selected layer cannot be copied."));
            return std::nullopt;
        }
        rasterAssets.insert(asset.key(), *asset);
    }

    // Clipping to the selection is expressed as one appended erase operation
    // over the inverted mask instead of per-stroke clip masks. That stays
    // correct when the layer already carries ordered framebuffer operations,
    // and the copied strokes keep wobbling live inside the selection.
    QImage inverted = selectionMask;
    inverted.invertPixels();
    if (const std::optional<PixelSelectionOp> clipOperation =
            makePixelSelectionOp(inverted,
                QTransform(),
                /*clearSource=*/true,
                /*drawDestination=*/false))
    {
        if (copy.strokes.size() >= DocumentLimits::maximumStrokesPerLayer)
        {
            setCodecError(error, tr("The layer is too complex to copy."));
            return std::nullopt;
        }
        Stroke clip;
        clip.mode = StrokeMode::PixelSelection;
        clip.points.clear();
        clip.pixelSelectionOp = *clipOperation;
        copy.strokes.append(std::move(clip));
    }

    Document payloadDocument;
    payloadDocument.size = document.size;
    payloadDocument.background = QColor(0, 0, 0, 0);
    payloadDocument.animationFrames = document.animationFrames;
    payloadDocument.framesPerSecond = document.framesPerSecond;
    payloadDocument.wobbleAmount = document.wobbleAmount;
    payloadDocument.motion = document.motion;
    payloadDocument.rasterAssets = rasterAssets;
    payloadDocument.activeLayerId = copy.id;
    payloadDocument.layers = {copy};

    DocumentSerializer::SerializationCache cache;
    const std::optional<DocumentSerializer::PreparedDocument> prepared =
        DocumentSerializer::prepare(payloadDocument, cache, error);
    if (!prepared)
    {
        return std::nullopt;
    }
    Copy result;
    result.payload = DocumentSerializer::toJson(*prepared, cache);
    if (result.payload.isEmpty())
    {
        setCodecError(error, tr("The selection could not be copied."));
        return std::nullopt;
    }
    result.raster = RenderEngine::render(payloadDocument, frameIndex)
                        .copy(packedSelection->bounds)
                        .convertToFormat(QImage::Format_ARGB32);
    result.layer = std::move(copy);
    result.canvasSize = document.size;
    result.rasterAssets = std::move(rasterAssets);
    return result;
}

std::optional<SelectionClipboardCodec::Pasted> SelectionClipboardCodec::decode(
    const QByteArray &payload, QString *error)
{
    const std::optional<Document> document =
        DocumentSerializer::fromJson(payload, error);
    if (!document)
    {
        return std::nullopt;
    }
    if (document->layers.size() != 1
        || document->layers.first().kind != LayerKind::Paint)
    {
        setCodecError(error, tr("The clipboard content is not supported."));
        return std::nullopt;
    }
    Pasted result;
    result.canvasSize = document->size;
    result.layer = document->layers.first();
    for (const Stroke &stroke : std::as_const(result.layer.strokes))
    {
        if (!stroke.imageOp)
        {
            continue;
        }
        const auto asset =
            document->rasterAssets.constFind(stroke.imageOp->assetId);
        if (asset == document->rasterAssets.cend())
        {
            setCodecError(error, tr("The clipboard content is not supported."));
            return std::nullopt;
        }
        result.rasterAssets.insert(asset.key(), *asset);
    }
    return result;
}

}

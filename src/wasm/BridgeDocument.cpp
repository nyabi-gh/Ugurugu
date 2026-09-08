// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "wasm/BridgeDocument.hpp"

#include "document/DocumentLimits.hpp"
#include "document/DocumentOperations.hpp"
#include "document/SelectionOutline.hpp"
#include "document/StrokeMask.hpp"

#include <QColor>
#include <QPainter>
#include <QPainterPath>
#include <QPolygonF>
#include <QRandomGenerator>
#include <QRectF>
#include <QSizeF>

#include <algorithm>
#include <cmath>
#include <utility>

namespace ugurugu::wasm
{

QByteArray &lastError()
{
    static QByteArray error;
    return error;
}

int &lastErrorCode()
{
    static int code = 0;
    return code;
}

void setError(int code, QByteArray message)
{
    lastErrorCode() = code;
    lastError() = std::move(message);
}

void clearError()
{
    lastErrorCode() = StatusOk;
    lastError().clear();
}

QUuid paintTargetLayer(const ugurugu::Document &document)
{
    if (const ugurugu::Layer *layer = document.layer(document.activeLayerId);
        layer != nullptr && layer->kind == ugurugu::LayerKind::Paint)
    {
        return document.activeLayerId;
    }
    for (const ugurugu::Layer &layer : document.layers)
    {
        if (layer.kind == ugurugu::LayerKind::Paint)
        {
            return layer.id;
        }
    }
    return {};
}

const ugurugu::Layer *layerAtIndex(const BridgeDocument *handle, int index)
{
    const auto &layers = handle->controller->document().layers;
    if (index < 0 || index >= layers.size())
    {
        return nullptr;
    }
    return &layers[index];
}

// addStroke refuses a stroke outright when any point falls outside the canvas
// (isValidInputStrokePoint), so a drag that leaves the canvas would silently
// lose the whole line. The desktop clamps at begin, continue and end through
// CanvasWidget::clampedDocumentPosition; this is the same contract.
ugurugu::StrokePoint canvasPoint(
    const BridgeDocument *handle, const QPointF &position, double pressure)
{
    const QSize size = handle->controller->document().size;
    return ugurugu::StrokePoint{
        QPointF(std::clamp(position.x(), 0.0, static_cast<qreal>(size.width())),
            std::clamp(position.y(), 0.0, static_cast<qreal>(size.height()))),
        std::clamp(pressure, 0.0, 1.0)};
}

void invalidateSplit(BridgeDocument *handle)
{
    handle->split = {};
    handle->splitLayerId = {};
    handle->splitFrame = -1;
    handle->splitUsable = false;
}

// Makes handle->split hold layerId's committed pixels for frameIndex, reusing
// a split an earlier commit already promoted. Returns whether the split can be
// used; an invalid one means the caller has to render the whole document.
bool ensureLayerSplit(BridgeDocument *handle, const QUuid &layerId, int frame)
{
    const ugurugu::Document &document = handle->controller->document();
    const bool reusable =
        handle->splitUsable && handle->split.valid
        && handle->splitLayerId == layerId && handle->splitFrame == frame
        && handle->split.below.size() == document.size
        && ugurugu::RenderEngine::supportsLayerSplit(document, layerId);
    if (!reusable)
    {
        handle->split = ugurugu::RenderEngine::renderLayerSplit(document,
            frame,
            document.size,
            layerId,
            ugurugu::RenderEngine::ScaledRenderMode::NativeExact);
        handle->splitLayerId = layerId;
        handle->splitFrame = frame;
        handle->splitUsable = handle->split.valid;
    }
    return handle->split.valid;
}

void renderCommittedFrame(BridgeDocument *handle, int frameIndex)
{
    handle->renderedFrame = ugurugu::RenderEngine::render(
        handle->controller->document(), frameIndex);
    handle->dirty = handle->renderedFrame.rect();
    handle->renderedFrameIndex = frameIndex;
    handle->renderedFrameCommitted = true;
}

void renderFullStrokePreview(BridgeDocument *handle)
{
    ugurugu::Document preview = handle->controller->document();
    if (ugurugu::Layer *layer = preview.layer(handle->strokeLayerId))
    {
        layer->strokes.append(handle->activeStroke);
    }
    handle->renderedFrame =
        ugurugu::RenderEngine::render(preview, handle->strokeFrame);
    handle->dirty = handle->renderedFrame.rect();
    handle->renderedFrameIndex = handle->strokeFrame;
    handle->renderedFrameCommitted = false;
}

// Draws the active stroke's pending points over renderedFrame and reports the
// tiles it touched. Returns false when the incremental contract broke and the
// caller has to fall back to a full preview render.
bool updateIncrementalStroke(BridgeDocument *handle, QRect *dirtyOut)
{
    const ugurugu::Document &document = handle->controller->document();
    const ugurugu::Layer *strokeLayer = document.layer(handle->strokeLayerId);
    if (strokeLayer == nullptr)
    {
        return false;
    }
    // The live stroke has to wobble the way its own layer does, not the way
    // the document does.
    const ugurugu::Document strokeDocument =
        ugurugu::documentForLayer(document, *strokeLayer);
    const auto update = handle->incremental.update(handle->split.layerBase,
        strokeDocument,
        handle->activeStroke,
        handle->strokeFrame,
        document.size);
    if (!update.valid)
    {
        return false;
    }
    QRect dirty;
    QPainter painter(&handle->renderedFrame);
    painter.setCompositionMode(QPainter::CompositionMode_Source);
    for (const auto &patch : update.patches)
    {
        // composeLayerSplitRegion wants the tile-sized patch image, not a
        // full layer surface; a null result means the contract broke, so fall
        // back to the full preview instead of dropping the patch.
        const QImage region = ugurugu::RenderEngine::composeLayerSplitRegion(
            handle->split, patch.layerImage, patch.bounds);
        if (region.isNull())
        {
            painter.end();
            return false;
        }
        painter.drawImage(patch.bounds.topLeft(), region);
        dirty = dirty.united(patch.bounds);
    }
    painter.end();
    handle->renderedFrameCommitted = false;
    if (dirtyOut != nullptr)
    {
        *dirtyOut = dirty;
    }
    return true;
}

bool selectionAppliesTo(const BridgeDocument *handle, const QUuid &layerId)
{
    return !handle->selectionMask.isNull()
           && handle->selectionLayerId == layerId
           && handle->selectionMask.size()
                  == handle->controller->document().size;
}

bool selectionMaskUsable(
    const BridgeDocument *handle, const QImage &mask, const QUuid &layerId)
{
    return !mask.isNull() && mask.size() == handle->controller->document().size
           && mask.format() == QImage::Format_Grayscale8
           && ugurugu::maskHasContent(mask) && !layerId.isNull();
}

// Every selection change goes through here so the revision, the cached
// outline and the "empty means no selection" rule stay in one place.
void installSelection(BridgeDocument *handle, QImage mask, const QUuid &layerId)
{
    const bool usable = selectionMaskUsable(handle, mask, layerId);
    handle->selectionMask = usable ? std::move(mask) : QImage();
    handle->selectionLayerId = usable ? layerId : QUuid();
    handle->selectionRevision += 1;

    handle->outlinePoints.clear();
    if (handle->selectionMask.isNull())
    {
        return;
    }
    // Flat float buffer: each contour is a vertex count followed by that many
    // x, y pairs, so the shell can walk it without a second length array.
    for (const QPolygonF &contour :
        ugurugu::selectionOutline(handle->selectionMask))
    {
        if (contour.size() < 2)
        {
            continue;
        }
        handle->outlinePoints.append(static_cast<float>(contour.size()));
        for (const QPointF &point : contour)
        {
            handle->outlinePoints.append(static_cast<float>(point.x()));
            handle->outlinePoints.append(static_cast<float>(point.y()));
        }
    }
}

// Pushing the command runs its own redo, which emits
// selectionHistoryStateRequested and installs the mask, so callers hand the
// next state over instead of installing it themselves.
void pushSelectionChange(BridgeDocument *handle,
    const QImage &nextMask,
    const QUuid &nextLayerId,
    const char *text)
{
    const bool usable = selectionMaskUsable(handle, nextMask, nextLayerId);
    const QImage afterMask = usable ? nextMask : QImage();
    const QUuid afterLayerId = usable ? nextLayerId : QUuid();
    if (handle->selectionMask.isNull() && afterMask.isNull())
    {
        installSelection(handle, afterMask, afterLayerId);
        return;
    }
    const int revision = handle->selectionRevision;
    handle->controller->pushSelectionStateCommand(QString::fromLatin1(text),
        handle->selectionLayerId,
        handle->selectionMask,
        afterLayerId,
        afterMask);
    if (handle->selectionRevision == revision)
    {
        // The stack refused the entry. The selection still has to change.
        installSelection(handle, afterMask, afterLayerId);
    }
}

// Combines with the same >= 128 threshold every other selection consumer
// uses, so a combined mask never disagrees with hit testing or packing.
QImage combinedSelectionMask(
    const QImage &base, const QImage &addition, int combine)
{
    if (base.isNull() || base.size() != addition.size()
        || base.format() != addition.format())
    {
        return combine == CombineAdd ? addition : QImage();
    }
    QImage combined = base;
    for (int y = 0; y < combined.height(); ++y)
    {
        uchar *line = combined.scanLine(y);
        const uchar *additionLine = addition.constScanLine(y);
        for (int x = 0; x < combined.width(); ++x)
        {
            if (combine == CombineAdd)
            {
                line[x] = std::max(line[x], additionLine[x]);
            }
            else if (additionLine[x] >= 128)
            {
                line[x] = 0;
            }
        }
    }
    return combined;
}

void applySelectionCombine(BridgeDocument *handle,
    const QImage &mask,
    const QUuid &layerId,
    int combine)
{
    if (combine == CombineReplace || !selectionAppliesTo(handle, layerId))
    {
        pushSelectionChange(handle, mask, layerId, "Select area");
        return;
    }
    pushSelectionChange(handle,
        combinedSelectionMask(handle->selectionMask, mask, combine),
        layerId,
        combine == CombineAdd ? "Add to selection" : "Subtract from selection");
}

// Drops the floating transform without touching pixels. For callers that have
// already replaced renderedFrame, or are about to.
void clearSelectionTransform(BridgeDocument *handle)
{
    handle->transformActive = false;
    handle->transformLayerId = QUuid();
    handle->transformSourceMask = QImage();
    handle->transformOperation = {};
    handle->transformMatrix = QTransform();
    handle->transformFrame = -1;
    handle->transformBaseFrame = QImage();
    handle->transformPreviewRegion = QRect();
}

// Undo and redo of a selection change arrive as this signal, the way
// CanvasWidget routes it into restoreSelectionState.
void attachSelectionHistory(BridgeDocument *handle)
{
    QObject::connect(handle->controller.get(),
        &ugurugu::DocumentController::selectionHistoryStateRequested,
        handle->controller.get(),
        [handle](const QUuid &layerId, const QImage &mask)
        {
            clearSelectionTransform(handle);
            installSelection(handle, mask, layerId);
        });
}

// Puts the committed pixels back under wherever the preview painted, then ends
// the session. Only the patched region is restored, so cancelling a small move
// on a large canvas uploads a small rectangle.
void restoreSelectionTransformBase(BridgeDocument *handle)
{
    const QRect patched = handle->transformPreviewRegion;
    const QImage base = handle->transformBaseFrame;
    const int frame = handle->transformFrame;
    if (!base.isNull())
    {
        if (patched.isEmpty())
        {
            handle->dirty = QRect();
        }
        else if (handle->renderedFrame.size() == base.size())
        {
            QPainter painter(&handle->renderedFrame);
            painter.setCompositionMode(QPainter::CompositionMode_Source);
            painter.drawImage(patched.topLeft(), base, patched);
            painter.end();
            handle->dirty = patched;
        }
        else
        {
            handle->renderedFrame = base;
            handle->dirty = handle->renderedFrame.rect();
        }
        handle->renderedFrameIndex = frame;
        handle->renderedFrameCommitted = true;
    }
    clearSelectionTransform(handle);
}

// Draws the floating selection at its current matrix over the committed frame.
// Mirrors CanvasWidgetPreview: replay the operation against the layer split and
// patch only the region it moves, falling back to whole-layer and then whole-
// document renders when the split cannot serve the layer.
void renderSelectionTransformPreview(BridgeDocument *handle)
{
    const QRect previous = handle->transformPreviewRegion;
    const int frame = handle->transformFrame;
    if (handle->renderedFrame.size() != handle->transformBaseFrame.size())
    {
        handle->renderedFrame = handle->transformBaseFrame;
    }
    if (ensureLayerSplit(handle, handle->transformLayerId, frame))
    {
        const ugurugu::RenderEngine::PixelSelectionPreviewRegion region =
            ugurugu::RenderEngine::replayPixelSelectionOnLayerRegion(
                handle->split.layerBase, handle->transformOperation);
        const QImage composed =
            region.valid && !region.bounds.isEmpty()
                ? ugurugu::RenderEngine::composeLayerSplitRegion(
                      handle->split, region.image, region.bounds)
                : QImage();
        if (region.valid && (region.bounds.isEmpty() || !composed.isNull()))
        {
            const QRect dirty = previous.united(region.bounds)
                                    .intersected(handle->renderedFrame.rect());
            if (!dirty.isEmpty())
            {
                QPainter painter(&handle->renderedFrame);
                painter.setCompositionMode(QPainter::CompositionMode_Source);
                // The base goes back first: the previous preview may have left
                // ink outside the region this one touches.
                painter.drawImage(
                    dirty.topLeft(), handle->transformBaseFrame, dirty);
                if (!region.bounds.isEmpty())
                {
                    painter.drawImage(region.bounds.topLeft(), composed);
                }
                painter.end();
            }
            handle->transformPreviewRegion =
                region.bounds.intersected(handle->renderedFrame.rect());
            handle->dirty = dirty;
            handle->renderedFrameIndex = frame;
            handle->renderedFrameCommitted = false;
            return;
        }
        QImage layerImage = handle->split.layerBase;
        if (ugurugu::RenderEngine::replayPixelSelectionOnLayer(
                layerImage, handle->transformOperation))
        {
            QImage composedFrame = ugurugu::RenderEngine::composeLayerSplit(
                handle->split, layerImage);
            if (!composedFrame.isNull())
            {
                handle->renderedFrame = std::move(composedFrame);
                handle->transformPreviewRegion = handle->renderedFrame.rect();
                handle->dirty = handle->renderedFrame.rect();
                handle->renderedFrameIndex = frame;
                handle->renderedFrameCommitted = false;
                return;
            }
        }
    }
    ugurugu::Document preview = handle->controller->document();
    if (ugurugu::Layer *layer = preview.layer(handle->transformLayerId))
    {
        ugurugu::Stroke operation;
        operation.mode = ugurugu::StrokeMode::PixelSelection;
        operation.pixelSelectionOp = handle->transformOperation;
        layer->strokes.append(std::move(operation));
    }
    handle->renderedFrame = ugurugu::RenderEngine::render(preview, frame);
    handle->transformPreviewRegion = handle->renderedFrame.rect();
    handle->dirty = handle->renderedFrame.rect();
    handle->renderedFrameIndex = frame;
    handle->renderedFrameCommitted = false;
}

// The image the flood fill reads. Mirrors CanvasWidget's three reference
// renderers: one isolated layer, every layer marked as a reference, or the
// whole visible composite — always over transparency so alpha boundaries mean
// what the user sees.
QImage referenceImage(BridgeDocument *handle, int frame, const QUuid &layerId)
{
    ugurugu::Document document = handle->controller->document();
    if (!document.size.isValid())
    {
        return {};
    }
    document.background = Qt::transparent;
    if (handle->fillReference == ReferenceActiveLayer)
    {
        const ugurugu::Layer *layer = document.layer(layerId);
        if (layer == nullptr)
        {
            return {};
        }
        return ugurugu::RenderEngine::render(
            ugurugu::DocumentOperations::isolatedLayerDocument(
                document, *layer),
            frame);
    }
    if (handle->fillReference == ReferenceMarkedLayers)
    {
        bool hasVisibleReference = false;
        for (ugurugu::Layer &layer : document.layers)
        {
            if (layer.kind != ugurugu::LayerKind::Paint)
            {
                continue;
            }
            if (!layer.reference)
            {
                layer.visible = false;
                continue;
            }
            hasVisibleReference =
                hasVisibleReference
                || ugurugu::DocumentOperations::isLayerRenderable(
                    document, layer);
        }
        if (!hasVisibleReference)
        {
            return {};
        }
    }
    return ugurugu::RenderEngine::render(document, frame);
}

QImage maskFromShape(
    const QSize &size, int shape, const double *points, int count)
{
    if (!size.isValid() || points == nullptr || count < 2)
    {
        return {};
    }
    const auto pointAt = [points](int index)
    {
        return QPointF(points[index * 2], points[index * 2 + 1]);
    };
    QPainterPath path;
    if (shape == ShapeFreehand)
    {
        if (count < 3)
        {
            return {};
        }
        path.moveTo(pointAt(0));
        for (int index = 1; index < count; ++index)
        {
            path.lineTo(pointAt(index));
        }
    }
    else
    {
        const QRectF bounds =
            QRectF(pointAt(0), pointAt(count - 1)).normalized();
        if (bounds.width() < 1.0 || bounds.height() < 1.0)
        {
            return {};
        }
        if (shape == ShapeRectangle)
        {
            path.addRect(bounds);
        }
        else
        {
            path.addEllipse(bounds);
        }
    }

    QImage mask(size, QImage::Format_Grayscale8);
    if (mask.isNull())
    {
        return {};
    }
    mask.fill(0);
    // Antialiasing stays off: the mask is a binary stencil that packBinaryMask
    // and every hit test read at the >= 128 threshold.
    QPainter painter(&mask);
    painter.setPen(Qt::NoPen);
    painter.setBrush(Qt::white);
    painter.drawPath(path);
    painter.end();
    return mask;
}

// A selection and a promoted split both describe the canvas that just went
// away, and a floating transform holds pixels lifted from it.
void afterCanvasResize(BridgeDocument *handle)
{
    clearSelectionTransform(handle);
    installSelection(handle, QImage(), QUuid());
    invalidateSplit(handle);
}

// Clamps the way each desktop wobble control clamps its own field, so a
// slider that reaches its end is applied rather than rejected whole.
std::optional<ugurugu::MotionSettings> motionFromValues(int style,
    int poseCount,
    int detail,
    double linked,
    double randomness,
    int brokenLine,
    double breakAmount,
    double breakRange)
{
    const auto motionStyle = static_cast<ugurugu::MotionStyle>(style);
    if (!ugurugu::isValidMotionStyle(motionStyle) || !std::isfinite(linked)
        || !std::isfinite(randomness) || !std::isfinite(breakAmount)
        || !std::isfinite(breakRange))
    {
        return std::nullopt;
    }
    ugurugu::MotionSettings motion;
    motion.style = motionStyle;
    motion.poseCount = std::clamp(poseCount,
        ugurugu::DocumentLimits::minimumMotionPoseCount,
        ugurugu::DocumentLimits::maximumMotionPoseCount);
    motion.detail = std::clamp(detail,
        ugurugu::DocumentLimits::minimumMotionDetail,
        ugurugu::DocumentLimits::maximumMotionDetail);
    motion.linked = std::clamp(linked, 0.0, 1.0);
    motion.randomness = std::clamp(randomness, 0.0, 1.0);
    motion.brokenLine = brokenLine != 0;
    motion.breakAmount = std::clamp(breakAmount, 0.0, 1.0);
    motion.breakRange = std::clamp(breakRange,
        ugurugu::DocumentLimits::minimumBreakRange,
        ugurugu::DocumentLimits::maximumBreakRange);
    return motion;
}

// Commits a coverage mask as a Fill stroke, the same shape of document
// operation the desktop bucket and lasso-paint modes produce.
int commitFrozenFill(BridgeDocument *handle, const QImage &coverage, int frame)
{
    const ugurugu::Document &document = handle->controller->document();
    const QUuid layerId = paintTargetLayer(document);
    const ugurugu::Layer *layer = document.layer(layerId);
    if (layer == nullptr)
    {
        setError(StatusNoPaintLayer,
            QByteArrayLiteral("document has no paint layer"));
        return 0;
    }
    if (!ugurugu::DocumentOperations::isLayerRenderable(document, *layer))
    {
        setError(StatusLayerNotDrawable,
            QByteArrayLiteral("the active layer is hidden or fully "
                              "transparent"));
        return 0;
    }
    const std::optional<ugurugu::PackedMaskRegion> packedCoverage =
        ugurugu::packBinaryMask(coverage);
    if (!packedCoverage)
    {
        setError(
            StatusEmptyRegion, QByteArrayLiteral("no fillable area was found"));
        return 0;
    }

    ugurugu::Stroke fill;
    fill.id = QUuid::createUuid();
    fill.seed = QRandomGenerator::global()->generate64();
    fill.mode = ugurugu::StrokeMode::Fill;
    fill.color = handle->brushTemplate.color;
    fill.width = std::clamp(handle->brushTemplate.width,
        ugurugu::DocumentLimits::minimumStrokeWidth,
        ugurugu::DocumentLimits::maximumStrokeWidth);
    fill.brush = handle->brushTemplate.brush;
    fill.brush.antialiasing = handle->bucketAntialiasing;
    fill.fillCoverage = *packedCoverage;
    if (selectionAppliesTo(handle, layerId))
    {
        fill.clipMask = handle->selectionMask;
    }
    fill.points.append(
        ugurugu::StrokePoint{QPointF(packedCoverage->bounds.center()), 1.0});

    using AddStrokeResult = ugurugu::DocumentController::AddStrokeResult;
    const auto result = handle->controller->addStroke(layerId, std::move(fill));
    invalidateSplit(handle);
    renderCommittedFrame(handle, frame);
    if (result != AddStrokeResult::Added
        && result != AddStrokeResult::AddedWithResampledPoints)
    {
        setError(StatusStrokeRejected,
            QByteArrayLiteral("the fill was "
                              "rejected"));
        return 0;
    }
    clearError();
    return 1;
}

}

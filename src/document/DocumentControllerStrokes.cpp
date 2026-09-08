// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "document/DocumentBudget.hpp"
#include "document/DocumentController.hpp"
#include "document/DocumentLimits.hpp"
#include "document/SelectionOperation.hpp"
#include "document/SelectionVisibility.hpp"
#include "document/StrokeMask.hpp"
#include "document/history/HistoryEffects.hpp"

#include <QSet>

#include <cmath>
#include <memory>
#include <utility>

namespace ugurugu
{

using DocumentBudget::distinctClipMaskBytes;
using DocumentBudget::totalPointCount;
using DocumentBudget::totalStrokeCount;

namespace
{

QVector<StrokePoint> resampleStrokePoints(
    const QVector<StrokePoint> &source, qsizetype targetCount)
{
    Q_ASSERT(targetCount >= 2 && targetCount < source.size());
    QVector<StrokePoint> result;
    result.reserve(targetCount);
    const qsizetype segmentCount = targetCount - 1;
    const qsizetype lastSourceIndex = source.size() - 1;
    const qsizetype baseStep = lastSourceIndex / segmentCount;
    const qsizetype remainderStep = lastSourceIndex % segmentCount;
    qsizetype sourceIndex = 0;
    qsizetype remainder = 0;
    for (qsizetype index = 0; index < targetCount; ++index)
    {
        result.append(source[sourceIndex]);
        sourceIndex += baseStep;
        remainder += remainderStep;
        if (remainder >= segmentCount)
        {
            ++sourceIndex;
            remainder -= segmentCount;
        }
    }
    return result;
}
bool isValidInputStrokePoint(const StrokePoint &point, const QSize &size)
{
    return std::isfinite(point.position.x())
           && std::isfinite(point.position.y()) && std::isfinite(point.pressure)
           && point.position.x() >= 0.0 && point.position.y() >= 0.0
           && point.position.x() <= size.width()
           && point.position.y() <= size.height() && point.pressure >= 0.0
           && point.pressure <= 1.0;
}
bool isValidStoredStrokePoint(const StrokePoint &point)
{
    return std::isfinite(point.position.x())
           && std::isfinite(point.position.y())
           && std::abs(point.position.x())
                  <= DocumentLimits::maximumStoredCoordinateMagnitude
           && std::abs(point.position.y())
                  <= DocumentLimits::maximumStoredCoordinateMagnitude
           && std::isfinite(point.pressure) && point.pressure >= 0.0
           && point.pressure <= 1.0;
}
QString visibilityCacheKey(const Stroke &stroke)
{
    const qint64 maskKey =
        stroke.clipMask.isNull() ? 0 : stroke.clipMask.cacheKey();
    if (!stroke.visibilityClip)
    {
        return QString::number(maskKey);
    }
    const QRect &rect = *stroke.visibilityClip;
    return QStringLiteral("%1:%2,%3,%4,%5")
        .arg(maskKey)
        .arg(rect.x())
        .arg(rect.y())
        .arg(rect.width())
        .arg(rect.height());
}
std::optional<Stroke> selectionOperationStroke(const QImage &selectionMask,
    const QTransform &transform,
    bool clearSource,
    bool drawDestination,
    std::optional<SamplingMode> sampling = std::nullopt)
{
    const std::optional<PixelSelectionOp> operation =
        sampling ? makePixelSelectionOp(selectionMask,
                       transform,
                       clearSource,
                       drawDestination,
                       *sampling)
                 : makePixelSelectionOp(
                       selectionMask, transform, clearSource, drawDestination);
    if (!operation)
    {
        return std::nullopt;
    }
    Stroke stroke;
    stroke.mode = StrokeMode::PixelSelection;
    stroke.points.clear();
    stroke.pixelSelectionOp = *operation;
    return stroke;
}

QImage materializedFillCoverage(const Stroke &stroke)
{
    return stroke.fillCoverage ? unpackBinaryMask(*stroke.fillCoverage)
                               : stroke.fillMask;
}
}

DocumentController::AddStrokeResult DocumentController::addStroke(
    const QUuid &layerId, Stroke stroke)
{
    const auto reject = [this](AddStrokeResult result)
    {
        failHistoryMacro();
        return result;
    };
    const PreparedState before = editableState();
    if (!before)
    {
        return reject(AddStrokeResult::RejectedCommit);
    }
    const Document &current = document();
    const Layer *layer = current.layer(layerId);
    if (!layer || layer->kind != LayerKind::Paint)
    {
        return reject(AddStrokeResult::RejectedInvalidLayer);
    }
    if (layer->strokes.size() >= DocumentLimits::maximumStrokesPerLayer
        || before->totalStrokeCount() >= DocumentLimits::maximumTotalStrokes)
    {
        return reject(AddStrokeResult::RejectedStrokeLimit);
    }
    if (stroke.id.isNull()
        || (stroke.mode != StrokeMode::Paint && stroke.mode != StrokeMode::Erase
            && stroke.mode != StrokeMode::Fill)
        || !stroke.color.isValid() || !std::isfinite(stroke.width)
        || stroke.width < DocumentLimits::minimumStrokeWidth
        || stroke.width > DocumentLimits::maximumStrokeWidth
        || !isValidBrushSettings(stroke.brush) || stroke.points.isEmpty()
        || (stroke.visibilityClip
            && (stroke.visibilityClip->isEmpty()
                || !QRect(QPoint(), current.size)
                    .contains(*stroke.visibilityClip)))
        || (!stroke.clipMask.isNull()
            && (stroke.clipMask.size() != current.size
                || stroke.clipMask.format() != QImage::Format_Grayscale8))
        || (!stroke.fillMask.isNull()
            && (stroke.mode != StrokeMode::Fill
                || stroke.fillMask.size() != current.size
                || stroke.fillMask.format() != QImage::Format_Grayscale8))
        || (stroke.fillCoverage
            && (stroke.mode != StrokeMode::Fill || !stroke.fillMask.isNull()
                || !isValidPackedMaskRegion(*stroke.fillCoverage)
                || stroke.fillCoverage->canvasSize != current.size)))
    {
        return reject(AddStrokeResult::RejectedInvalidStroke);
    }
    if (!std::all_of(stroke.points.cbegin(),
            stroke.points.cend(),
            [&current](const StrokePoint &point)
            {
                return isValidInputStrokePoint(point, current.size);
            }))
    {
        return reject(AddStrokeResult::RejectedInvalidStroke);
    }

    const qsizetype currentPointCount = before->totalPointCount();
    if (currentPointCount >= DocumentLimits::maximumTotalPoints)
    {
        return reject(AddStrokeResult::RejectedPointLimit);
    }
    const qsizetype availablePoints =
        DocumentLimits::maximumTotalPoints - currentPointCount;
    const qsizetype acceptedPointCount = std::min(stroke.points.size(),
        std::min(static_cast<qsizetype>(DocumentLimits::maximumPointsPerStroke),
            availablePoints));
    const bool pointsResampled = acceptedPointCount < stroke.points.size();
    if (pointsResampled && acceptedPointCount < 2)
    {
        return reject(AddStrokeResult::RejectedPointLimit);
    }
    if (pointsResampled)
    {
        stroke.points = resampleStrokePoints(stroke.points, acceptedPointCount);
    }
    if (!canonicalizeStrokeVisibility(stroke, current.size))
    {
        return reject(AddStrokeResult::RejectedInvalidStroke);
    }

    const QUuid strokeId = stroke.id;
    const std::optional<PackedMaskRegion> clipMask =
        stroke.clipMask.isNull() ? std::optional<PackedMaskRegion>()
                                 : packBinaryMask(stroke.clipMask);
    if (!stroke.clipMask.isNull() && !clipMask)
    {
        return reject(AddStrokeResult::RejectedMaskLimit);
    }
    auto effects = std::make_shared<HistoryEffects>();
    effects->beforeDocumentChanged.append(
        HistoryEffects::StrokePresence{layerId, strokeId, clipMask});
    effects->afterDocumentChanged.append(
        HistoryEffects::LayerThumbnail{layerId});

    DocumentSerializer::AppendStrokeResult appended =
        DocumentSerializer::appendStroke(*before,
            layerId,
            stroke,
            m_serializationCache,
            DocumentLimits::maximumProjectBytes);
    switch (appended.status)
    {
    case DocumentSerializer::AppendStrokeStatus::Appended:
    {
        PreparedState after = std::make_shared<const PreparedDocument>(
            std::move(appended.prepared));
        if (!tryCommitPreparedCandidate(tr("Draw stroke"),
                before,
                std::move(after),
                std::move(effects),
                ActiveLayerPolicy::PreserveCurrentIfPresent,
                -1,
                {},
                layerId))
        {
            return AddStrokeResult::RejectedCommit;
        }
        return pointsResampled ? AddStrokeResult::AddedWithResampledPoints
                               : AddStrokeResult::Added;
    }
    case DocumentSerializer::AppendStrokeStatus::NotApplicable:
    {
        Document candidate = current;
        candidate.layer(layerId)->strokes.append(stroke);
        if (distinctClipMaskBytes(candidate)
            > DocumentLimits::maximumDistinctClipMaskBytes)
        {
            return reject(AddStrokeResult::RejectedMaskLimit);
        }
        if (!tryCommitCandidate(
                tr("Draw stroke"), std::move(candidate), std::move(effects)))
        {
            return AddStrokeResult::RejectedCommit;
        }
        return pointsResampled ? AddStrokeResult::AddedWithResampledPoints
                               : AddStrokeResult::Added;
    }
    case DocumentSerializer::AppendStrokeStatus::StrokeLimit:
        return reject(AddStrokeResult::RejectedStrokeLimit);
    case DocumentSerializer::AppendStrokeStatus::PointLimit:
        return reject(AddStrokeResult::RejectedPointLimit);
    case DocumentSerializer::AppendStrokeStatus::MaskLimit:
        return reject(AddStrokeResult::RejectedMaskLimit);
    case DocumentSerializer::AppendStrokeStatus::Invalid:
        return reject(AddStrokeResult::RejectedInvalidStroke);
    case DocumentSerializer::AppendStrokeStatus::TooLarge:
        return reject(AddStrokeResult::RejectedCommit);
    }
    return reject(AddStrokeResult::RejectedCommit);
}

bool DocumentController::moveStrokes(const QUuid &layerId,
    const QVector<QUuid> &strokeIds,
    const QPointF &delta,
    const QImage &selectionMask)
{
    if (!std::isfinite(delta.x()) || !std::isfinite(delta.y())
        || (qFuzzyIsNull(delta.x()) && qFuzzyIsNull(delta.y())))
    {
        return rejectHistoryMutation();
    }
    QTransform transform;
    transform.translate(delta.x(), delta.y());
    return transformStrokes(layerId,
        strokeIds,
        transform,
        1.0,
        tr("Move selection"),
        selectionMask);
}

bool DocumentController::scaleStrokes(const QUuid &layerId,
    const QVector<QUuid> &strokeIds,
    const QPointF &center,
    qreal factor,
    const QImage &selectionMask)
{
    if (!std::isfinite(center.x()) || !std::isfinite(center.y())
        || !std::isfinite(factor) || factor <= 0.0)
    {
        return rejectHistoryMutation();
    }
    QTransform transform;
    transform.translate(center.x(), center.y());
    transform.scale(factor, factor);
    transform.translate(-center.x(), -center.y());
    return transformStrokes(layerId,
        strokeIds,
        transform,
        factor,
        tr("Scale selection"),
        selectionMask);
}

bool DocumentController::rotateStrokes(const QUuid &layerId,
    const QVector<QUuid> &strokeIds,
    const QPointF &center,
    qreal degrees,
    const QImage &selectionMask)
{
    if (!std::isfinite(center.x()) || !std::isfinite(center.y())
        || !std::isfinite(degrees) || qFuzzyIsNull(degrees))
    {
        return rejectHistoryMutation();
    }
    QTransform transform;
    transform.translate(center.x(), center.y());
    transform.rotate(degrees);
    transform.translate(-center.x(), -center.y());
    return transformStrokes(layerId,
        strokeIds,
        transform,
        1.0,
        tr("Rotate selection"),
        selectionMask);
}

bool DocumentController::flipStrokes(const QUuid &layerId,
    const QVector<QUuid> &strokeIds,
    const QPointF &center,
    bool horizontal,
    const QImage &selectionMask)
{
    if (!std::isfinite(center.x()) || !std::isfinite(center.y()))
    {
        return rejectHistoryMutation();
    }
    QTransform transform;
    transform.translate(center.x(), center.y());
    transform.scale(horizontal ? -1.0 : 1.0, horizontal ? 1.0 : -1.0);
    transform.translate(-center.x(), -center.y());
    return transformStrokes(layerId,
        strokeIds,
        transform,
        1.0,
        horizontal ? tr("Flip selection horizontally")
                   : tr("Flip selection vertically"),
        selectionMask);
}

bool DocumentController::transformSelection(const QUuid &layerId,
    const QVector<QUuid> &strokeIds,
    const QTransform &transform,
    const QImage &selectionMask)
{
    return transformSelection(layerId,
        strokeIds,
        transform,
        selectionMask,
        samplingForSelectionTransform(transform));
}

bool DocumentController::transformSelection(const QUuid &layerId,
    const QVector<QUuid> &strokeIds,
    const QTransform &transform,
    const QImage &selectionMask,
    SamplingMode sampling)
{
    const bool finite =
        std::isfinite(transform.m11()) && std::isfinite(transform.m12())
        && std::isfinite(transform.m13()) && std::isfinite(transform.m21())
        && std::isfinite(transform.m22()) && std::isfinite(transform.m23())
        && std::isfinite(transform.m31()) && std::isfinite(transform.m32())
        && std::isfinite(transform.m33());
    const qreal determinant = transform.determinant();
    const qreal widthScale = std::sqrt(std::abs(determinant));
    if ((sampling != SamplingMode::Smooth && sampling != SamplingMode::Nearest)
        || !finite || !transform.isAffine() || transform.isIdentity()
        || !std::isfinite(widthScale) || widthScale <= 0.0)
    {
        return rejectHistoryMutation();
    }
    return transformStrokes(layerId,
        strokeIds,
        transform,
        widthScale,
        tr("Transform selection"),
        selectionMask,
        sampling);
}

bool DocumentController::duplicateStrokes(const QUuid &layerId,
    const QVector<QUuid> &strokeIds,
    const QPointF &delta,
    const QImage &selectionMask)
{
    const Document &current = document();
    if (strokeIds.isEmpty() || !std::isfinite(delta.x())
        || !std::isfinite(delta.y())
        || (!selectionMask.isNull()
            && (selectionMask.size() != current.size
                || selectionMask.format() != QImage::Format_Grayscale8)))
    {
        return rejectHistoryMutation();
    }

    const Layer *layer = current.layer(layerId);
    if (!layer)
    {
        return rejectHistoryMutation();
    }
    if (selectionMask.isNull()
        && std::any_of(layer->strokes.cbegin(),
            layer->strokes.cend(),
            [](const Stroke &stroke)
            {
                return stroke.mode == StrokeMode::PixelSelection
                       || stroke.mode == StrokeMode::Reframe;
            }))
    {
        return rejectHistoryMutation();
    }
    const QSet<QUuid> requested(strokeIds.cbegin(), strokeIds.cend());
    if (!selectionMask.isNull())
    {
        if (layer->strokes.size() >= DocumentLimits::maximumStrokesPerLayer
            || totalStrokeCount(current) >= DocumentLimits::maximumTotalStrokes
            || !std::any_of(layer->strokes.cbegin(),
                layer->strokes.cend(),
                [&requested](const Stroke &stroke)
                {
                    return requested.contains(stroke.id);
                })
            || !selectionHasVisibleLayerPixels(layerId, selectionMask))
        {
            return rejectHistoryMutation();
        }
        QTransform transform;
        transform.translate(delta.x(), delta.y());
        const std::optional<Stroke> operation =
            selectionOperationStroke(selectionMask, transform, false, true);
        if (!operation || !operation->pixelSelectionOp)
        {
            return rejectHistoryMutation();
        }
        const PixelSelectionOp &pixelOperation = *operation->pixelSelectionOp;
        const QVector<Stroke> before = layer->strokes;
        QVector<Stroke> after = before;
        after.append(*operation);
        Document withCopy = current;
        withCopy.layer(layerId)->strokes = after;
        if (distinctClipMaskBytes(withCopy)
            > DocumentLimits::maximumDistinctClipMaskBytes)
        {
            return rejectHistoryMutation();
        }
        const QImage nextSelectionMask = transformedSelectionSupport(
            selectionMask, current.size, transform, pixelOperation.sampling);
        if (!maskHasContent(nextSelectionMask))
        {
            return rejectHistoryMutation();
        }
        const PackedMaskRegion sourceMaskSnapshot{pixelOperation.canvasSize,
            pixelOperation.sourceBounds,
            pixelOperation.packedMask};
        std::optional<PackedMaskRegion> nextMaskSnapshot =
            packBinaryMask(nextSelectionMask);
        if (!nextMaskSnapshot)
        {
            return rejectHistoryMutation();
        }
        const QVector<QUuid> resultIds{operation->id};
        auto effects = std::make_shared<HistoryEffects>();
        effects->beforeDocumentChanged.append(
            HistoryEffects::SelectionOverlay{layerId,
                strokeIds,
                resultIds,
                sourceMaskSnapshot,
                std::move(nextMaskSnapshot)});
        effects->afterDocumentChanged.append(
            HistoryEffects::LayerThumbnail{layerId});
        return tryCommitCandidate(
            tr("Duplicate selection"), std::move(withCopy), std::move(effects));
    }
    QVector<Stroke> copies;
    QVector<QUuid> sourceIds;
    QVector<QUuid> duplicateIds;
    qsizetype addedPoints = 0;
    QTransform transform;
    transform.translate(delta.x(), delta.y());
    QHash<QString, QImage> selectedMasks;
    QHash<qint64, QImage> transformedMasks;
    for (const Stroke &stroke : layer->strokes)
    {
        if (!requested.contains(stroke.id))
        {
            continue;
        }
        const QString sourceMaskKey = visibilityCacheKey(stroke);
        QImage duplicateMask;
        if (!selectionMask.isNull())
        {
            auto selected = selectedMasks.constFind(sourceMaskKey);
            if (selected == selectedMasks.cend())
            {
                const std::optional<QImage> visibility =
                    materializedVisibilityMask(stroke, current.size);
                if (!visibility)
                {
                    return rejectHistoryMutation();
                }
                selected = selectedMasks.insert(sourceMaskKey,
                    maskedPart(*visibility, selectionMask, true));
            }
            if (!maskHasContent(selected.value())
                || (stroke.mode == StrokeMode::Fill
                    && !masksIntersect(
                        materializedFillCoverage(stroke), selected.value())))
            {
                continue;
            }
            duplicateMask =
                transformedMask(selected.value(), current.size, transform);
            if (!maskHasContent(duplicateMask))
            {
                continue;
            }
        }

        Stroke copy = stroke;
        copy.id = QUuid::createUuid();
        if (copy.imageOp)
        {
            copy.imageOp->transform = transform * copy.imageOp->transform;
            if (!isValidImageOp(*copy.imageOp))
            {
                return rejectHistoryMutation();
            }
        }
        for (StrokePoint &point : copy.points)
        {
            point.position += delta;
            const bool valid =
                selectionMask.isNull()
                    ? isValidInputStrokePoint(point, current.size)
                    : isValidStoredStrokePoint(point);
            if (!valid)
            {
                return rejectHistoryMutation();
            }
        }
        if (!selectionMask.isNull())
        {
            copy.visibilityClip.reset();
            copy.clipMask = duplicateMask;
        }
        else
        {
            const std::optional<QImage> visibility =
                materializedVisibilityMask(stroke, current.size);
            if (!visibility)
            {
                return rejectHistoryMutation();
            }
            copy.visibilityClip.reset();
            copy.clipMask = *visibility;
            if (!transformMask(
                    copy.clipMask, current.size, transform, transformedMasks))
            {
                return rejectHistoryMutation();
            }
        }
        if (!transformMask(
                copy.fillMask, current.size, transform, transformedMasks))
        {
            return rejectHistoryMutation();
        }
        if (copy.fillCoverage)
        {
            copy.fillCoverage = transformedPackedMask(
                *copy.fillCoverage, current.size, transform);
            if (!copy.fillCoverage)
            {
                return rejectHistoryMutation();
            }
        }
        if (!canonicalizeStrokeVisibility(copy, current.size))
        {
            return rejectHistoryMutation();
        }
        addedPoints += copy.points.size();
        sourceIds.append(stroke.id);
        duplicateIds.append(copy.id);
        copies.append(std::move(copy));
    }
    if (copies.isEmpty()
        || layer->strokes.size()
               > DocumentLimits::maximumStrokesPerLayer - copies.size()
        || totalStrokeCount(current)
               > DocumentLimits::maximumTotalStrokes - copies.size()
        || totalPointCount(current)
               > DocumentLimits::maximumTotalPoints - addedPoints)
    {
        return rejectHistoryMutation();
    }
    Document withCopies = current;
    if (Layer *target = withCopies.layer(layerId))
    {
        target->strokes += copies;
    }
    if (distinctClipMaskBytes(withCopies)
        > DocumentLimits::maximumDistinctClipMaskBytes)
    {
        return rejectHistoryMutation();
    }

    auto effects = std::make_shared<HistoryEffects>();
    effects->beforeDocumentChanged.append(HistoryEffects::StrokeDuplicate{
        layerId, sourceIds, duplicateIds, delta});
    effects->afterDocumentChanged.append(
        HistoryEffects::LayerThumbnail{layerId});
    return tryCommitCandidate(
        tr("Duplicate selection"), std::move(withCopies), std::move(effects));
}

bool DocumentController::removeSelectedContent(const QUuid &layerId,
    const QVector<QUuid> &strokeIds,
    const QImage &selectionMask)
{
    const Document &current = document();
    if (strokeIds.isEmpty() || selectionMask.size() != current.size
        || selectionMask.format() != QImage::Format_Grayscale8)
    {
        return rejectHistoryMutation();
    }

    const Layer *layer = current.layer(layerId);
    if (!layer)
    {
        return rejectHistoryMutation();
    }
    const QSet<QUuid> requested(strokeIds.cbegin(), strokeIds.cend());
    if (layer->strokes.size() >= DocumentLimits::maximumStrokesPerLayer
        || totalStrokeCount(current) >= DocumentLimits::maximumTotalStrokes
        || !std::any_of(layer->strokes.cbegin(),
            layer->strokes.cend(),
            [&requested](const Stroke &stroke)
            {
                return requested.contains(stroke.id);
            })
        || !selectionHasVisibleLayerPixels(layerId, selectionMask))
    {
        return rejectHistoryMutation();
    }
    const std::optional<Stroke> operation =
        selectionOperationStroke(selectionMask, QTransform(), true, false);
    const std::optional<PackedMaskRegion> packedSelection =
        packBinaryMask(selectionMask);
    if (!operation || !packedSelection)
    {
        return rejectHistoryMutation();
    }
    Document withoutSelection = current;
    withoutSelection.layer(layerId)->strokes.append(*operation);
    if (distinctClipMaskBytes(withoutSelection)
        > DocumentLimits::maximumDistinctClipMaskBytes)
    {
        return rejectHistoryMutation();
    }

    auto effects = std::make_shared<HistoryEffects>();
    effects->afterDocumentChanged.append(
        HistoryEffects::LayerThumbnail{layerId});
    effects->selectionState = HistoryEffects::SelectionStateTransition{
        {layerId, packedSelection}, {{}, std::nullopt}};
    return tryCommitCandidate(tr("Delete selected content"),
        std::move(withoutSelection),
        std::move(effects));
}

bool DocumentController::updateStrokeAttributes(const QUuid &layerId,
    const QVector<QUuid> &strokeIds,
    const std::optional<QColor> &color,
    const std::optional<qreal> &width)
{
    const Document &current = document();
    const Layer *layer = current.layer(layerId);
    if (!layer || layer->kind != LayerKind::Paint || strokeIds.isEmpty()
        || (!color && !width) || (color && !color->isValid())
        || (width
            && (!std::isfinite(*width)
                || *width < DocumentLimits::minimumStrokeWidth
                || *width > DocumentLimits::maximumStrokeWidth)))
    {
        return rejectHistoryMutation();
    }

    const QSet<QUuid> requested(strokeIds.cbegin(), strokeIds.cend());
    Document candidate = current;
    Layer *target = candidate.layer(layerId);
    bool changed = false;
    for (Stroke &stroke : target->strokes)
    {
        if (!requested.contains(stroke.id))
        {
            continue;
        }
        if (color
            && (stroke.mode == StrokeMode::Paint
                || stroke.mode == StrokeMode::Fill)
            && stroke.color != *color)
        {
            stroke.color = *color;
            changed = true;
        }
        if (width
            && (stroke.mode == StrokeMode::Paint
                || stroke.mode == StrokeMode::Erase)
            && !qFuzzyCompare(stroke.width, *width))
        {
            stroke.width = *width;
            changed = true;
        }
    }
    if (!changed)
    {
        return rejectHistoryMutation();
    }

    auto effects = std::make_shared<HistoryEffects>();
    effects->afterDocumentChanged.append(
        HistoryEffects::LayerThumbnail{layerId});
    return tryCommitCandidate(
        tr("Edit stroke properties"), std::move(candidate), std::move(effects));
}

bool DocumentController::transformStrokes(const QUuid &layerId,
    const QVector<QUuid> &strokeIds,
    const QTransform &transform,
    qreal widthScale,
    const QString &text,
    const QImage &selectionMask,
    std::optional<SamplingMode> sampling)
{
    const Document &current = document();
    const Layer *layer = current.layer(layerId);
    if (!layer || strokeIds.isEmpty() || !std::isfinite(widthScale)
        || widthScale <= 0.0)
    {
        return rejectHistoryMutation();
    }
    if (selectionMask.isNull()
        && std::any_of(layer->strokes.cbegin(),
            layer->strokes.cend(),
            [](const Stroke &stroke)
            {
                return stroke.mode == StrokeMode::PixelSelection
                       || stroke.mode == StrokeMode::Reframe;
            }))
    {
        return rejectHistoryMutation();
    }
    bool invertible = false;
    const QTransform inverse = transform.inverted(&invertible);
    if (!invertible)
    {
        return rejectHistoryMutation();
    }

    const QSet<QUuid> requested(strokeIds.cbegin(), strokeIds.cend());
    if (!selectionMask.isNull())
    {
        if (selectionMask.size() != current.size
            || selectionMask.format() != QImage::Format_Grayscale8)
        {
            return rejectHistoryMutation();
        }
        const Layer *sourceLayer = current.layer(layerId);
        if (!sourceLayer
            || sourceLayer->strokes.size()
                   >= DocumentLimits::maximumStrokesPerLayer
            || totalStrokeCount(current) >= DocumentLimits::maximumTotalStrokes
            || !std::any_of(sourceLayer->strokes.cbegin(),
                sourceLayer->strokes.cend(),
                [&requested](const Stroke &stroke)
                {
                    return requested.contains(stroke.id);
                })
            || !selectionHasVisibleLayerPixels(layerId, selectionMask))
        {
            return rejectHistoryMutation();
        }
        const std::optional<Stroke> operation = selectionOperationStroke(
            selectionMask, transform, true, true, sampling);
        if (!operation || !operation->pixelSelectionOp)
        {
            return rejectHistoryMutation();
        }
        const PixelSelectionOp &pixelOperation = *operation->pixelSelectionOp;
        Document transformedDocument = current;
        transformedDocument.layer(layerId)->strokes.append(*operation);
        if (distinctClipMaskBytes(transformedDocument)
            > DocumentLimits::maximumDistinctClipMaskBytes)
        {
            return rejectHistoryMutation();
        }
        const QImage nextSelectionMask = transformedSelectionSupport(
            selectionMask, current.size, transform, pixelOperation.sampling);
        if (!maskHasContent(nextSelectionMask))
        {
            return rejectHistoryMutation();
        }
        const PackedMaskRegion sourceMaskSnapshot{pixelOperation.canvasSize,
            pixelOperation.sourceBounds,
            pixelOperation.packedMask};
        std::optional<PackedMaskRegion> nextMaskSnapshot =
            packBinaryMask(nextSelectionMask);
        if (!nextMaskSnapshot)
        {
            return rejectHistoryMutation();
        }
        const QVector<QUuid> resultIds{operation->id};
        auto effects = std::make_shared<HistoryEffects>();
        effects->beforeDocumentChanged.append(
            HistoryEffects::SelectionOverlay{layerId,
                strokeIds,
                resultIds,
                sourceMaskSnapshot,
                std::move(nextMaskSnapshot)});
        effects->afterDocumentChanged.append(
            HistoryEffects::LayerThumbnail{layerId});
        return tryCommitCandidate(
            text, std::move(transformedDocument), std::move(effects));
    }

    QVector<Stroke> after;
    QVector<QUuid> transformedIds;
    QHash<qint64, QImage> transformedMasks;
    for (const Stroke &stroke : layer->strokes)
    {
        if (!requested.contains(stroke.id))
        {
            continue;
        }
        if (stroke.mode == StrokeMode::PixelSelection
            || stroke.mode == StrokeMode::Reframe)
        {
            return rejectHistoryMutation();
        }
        Stroke transformed = stroke;
        if (transformed.imageOp)
        {
            transformed.imageOp->transform =
                transform * transformed.imageOp->transform;
            if (!isValidImageOp(*transformed.imageOp))
            {
                return rejectHistoryMutation();
            }
        }
        for (StrokePoint &point : transformed.points)
        {
            point.position = transform.map(point.position);
            if (!isValidInputStrokePoint(point, current.size))
            {
                return rejectHistoryMutation();
            }
        }
        if (transformed.mode != StrokeMode::Image)
        {
            transformed.width = std::clamp(transformed.width * widthScale,
                DocumentLimits::minimumStrokeWidth,
                DocumentLimits::maximumStrokeWidth);
        }
        if (!transformMask(transformed.clipMask,
                current.size,
                transform,
                transformedMasks))
        {
            return rejectHistoryMutation();
        }
        if (!transformMask(transformed.fillMask,
                current.size,
                transform,
                transformedMasks))
        {
            return rejectHistoryMutation();
        }
        if (transformed.fillCoverage)
        {
            transformed.fillCoverage = transformedPackedMask(
                *transformed.fillCoverage, current.size, transform);
            if (!transformed.fillCoverage)
            {
                return rejectHistoryMutation();
            }
        }
        if (!canonicalizeStrokeVisibility(transformed, current.size))
        {
            return rejectHistoryMutation();
        }
        after.append(std::move(transformed));
        transformedIds.append(stroke.id);
    }
    if (after.isEmpty())
    {
        return rejectHistoryMutation();
    }
    Document transformedDocument = current;
    if (Layer *target = transformedDocument.layer(layerId))
    {
        QHash<QUuid, Stroke> replacements;
        for (const Stroke &stroke : after)
        {
            replacements.insert(stroke.id, stroke);
        }
        for (Stroke &stroke : target->strokes)
        {
            const auto replacement = replacements.constFind(stroke.id);
            if (replacement != replacements.cend())
            {
                stroke = replacement.value();
            }
        }
    }
    if (distinctClipMaskBytes(transformedDocument)
        > DocumentLimits::maximumDistinctClipMaskBytes)
    {
        return rejectHistoryMutation();
    }

    auto effects = std::make_shared<HistoryEffects>();
    effects->beforeDocumentChanged.append(HistoryEffects::StrokeTransform{
        layerId, transformedIds, transform, inverse});
    effects->afterDocumentChanged.append(
        HistoryEffects::LayerThumbnail{layerId});
    return tryCommitCandidate(
        text, std::move(transformedDocument), std::move(effects));
}

void DocumentController::removeStrokes(
    const QUuid &layerId, const QVector<QUuid> &strokeIds)
{
    const Document &current = document();
    const Layer *layer = current.layer(layerId);
    if (!layer || strokeIds.isEmpty())
    {
        failHistoryMacro();
        return;
    }

    const QSet<QUuid> requested(strokeIds.cbegin(), strokeIds.cend());
    QSet<QUuid> removableIds;
    for (const Stroke &stroke : layer->strokes)
    {
        if (requested.contains(stroke.id)
            && stroke.mode != StrokeMode::PixelSelection
            && stroke.mode != StrokeMode::Reframe)
        {
            removableIds.insert(stroke.id);
        }
    }
    if (removableIds.isEmpty())
    {
        failHistoryMacro();
        return;
    }

    Document candidate = current;
    candidate.layer(layerId)->strokes.removeIf(
        [&removableIds](const Stroke &stroke)
        {
            return removableIds.contains(stroke.id);
        });
    auto effects = std::make_shared<HistoryEffects>();
    effects->afterDocumentChanged.append(
        HistoryEffects::LayerThumbnail{layerId});
    tryCommitCandidate(
        tr("Delete selection"), std::move(candidate), std::move(effects));
}
}

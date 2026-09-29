// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "app/WatchedFutureResult.hpp"
#include "document/DocumentLimits.hpp"
#include "document/DocumentOperations.hpp"
#include "document/SelectionOperation.hpp"
#include "document/SelectionVisibility.hpp"
#include "document/StrokeMask.hpp"
#include "render/FloodFillMask.hpp"
#include "render/RenderEngine.hpp"
#include "ui/CanvasViewport.hpp"
#include "ui/CanvasWidget.hpp"
#include "ui/SelectionActionBar.hpp"
#include "ui/Theme.hpp"

#include <QFutureWatcher>
#include <QHash>
#include <QPainter>
#include <QPointer>
#include <QRandomGenerator>
#include <QtConcurrentRun>

#include <algorithm>
#include <cmath>
#include <utility>

namespace ugurugu
{

using namespace canvas_detail;

namespace
{

// Combines with the same >= 128 threshold every other selection consumer
// uses, so a combined mask never disagrees with hit testing or packing.
QImage combinedSelectionMask(
    const QImage &base, const QImage &addition, CanvasSelectionCombine combine)
{
    if (base.isNull() || base.size() != addition.size()
        || base.format() != addition.format())
    {
        return combine == CanvasSelectionCombine::Add ? addition : QImage();
    }
    QImage combined = base;
    for (int y = 0; y < combined.height(); ++y)
    {
        uchar *line = combined.scanLine(y);
        const uchar *additionLine = addition.constScanLine(y);
        for (int x = 0; x < combined.width(); ++x)
        {
            if (combine == CanvasSelectionCombine::Add)
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

FloodFillMask::Comparison floodComparison(CanvasFillComparison comparison)
{
    return comparison == CanvasFillComparison::Color
               ? FloodFillMask::Comparison::Color
               : FloodFillMask::Comparison::AlphaBoundary;
}

}

bool CanvasWidget::selectionContains(const QPointF &documentPosition) const
{
    const QImage &sourceMask = m_selectionTransformSession.active
                                   ? m_selectionTransformSession.sourceMask
                                   : m_selectionMask;
    if (sourceMask.isNull() || !std::isfinite(documentPosition.x())
        || !std::isfinite(documentPosition.y()))
    {
        return false;
    }
    QPointF sourcePosition = documentPosition;
    if (m_selectionTransformSession.active)
    {
        bool invertible = false;
        const QTransform inverse =
            m_selectionTransformSession.transform.inverted(&invertible);
        if (!invertible)
        {
            return false;
        }
        sourcePosition = inverse.map(documentPosition);
    }
    if (!std::isfinite(sourcePosition.x()) || !std::isfinite(sourcePosition.y())
        || sourcePosition.x() < 0.0 || sourcePosition.y() < 0.0
        || sourcePosition.x() >= sourceMask.width()
        || sourcePosition.y() >= sourceMask.height())
    {
        return false;
    }
    const QPoint pixel(static_cast<int>(std::floor(sourcePosition.x())),
        static_cast<int>(std::floor(sourcePosition.y())));
    return sourceMask.constScanLine(pixel.y())[pixel.x()] >= 128;
}

void CanvasWidget::beginAreaSelection(
    const QPointF &documentPosition, SelectionCombine combine)
{
    cancelSelectionTransformForBoundary(
        tr("The pending selection transform was canceled before selecting."));
    m_selectionBeforeArea = currentSelectionState();
    m_hasSelectionBeforeArea = true;
    if (m_lassoMode == LassoMode::Select
        && combine == SelectionCombine::Replace)
    {
        clearSelection();
    }
    m_areaSelectionCombine = combine;
    m_areaSelectionActive = true;
    m_areaSelectionAnchor = clampedDocumentPosition(documentPosition);
    m_areaSelectionCurrent = m_areaSelectionAnchor;
    m_areaSelectionPoints.clear();
    m_areaSelectionPoints.append(m_areaSelectionAnchor);
    updateSelectionAnimation();
    requestDisplayUpdate();
}

void CanvasWidget::continueAreaSelection(const QPointF &documentPosition)
{
    if (!m_areaSelectionActive)
    {
        return;
    }
    m_areaSelectionCurrent = clampedDocumentPosition(documentPosition);
    if (m_selectionShape == SelectionShape::Freehand
        && (m_areaSelectionPoints.isEmpty()
            || pointDistance(
                   m_areaSelectionCurrent, m_areaSelectionPoints.constLast())
                   >= 1.0))
    {
        m_areaSelectionPoints.append(m_areaSelectionCurrent);
    }
    requestDisplayUpdate();
}

void CanvasWidget::finishAreaSelection()
{
    if (!m_areaSelectionActive)
    {
        return;
    }
    const bool valid = canFinishAreaSelection();
    const QPainterPath path = areaSelectionPath();
    const SelectionState previousSelection =
        m_hasSelectionBeforeArea ? m_selectionBeforeArea : SelectionState();
    const SelectionCombine combine = m_areaSelectionCombine;
    const bool paintMode = m_lassoMode == LassoMode::Paint;
    m_areaSelectionCombine = SelectionCombine::Replace;
    m_areaSelectionActive = false;
    m_areaSelectionPoints.clear();
    m_areaSelectionAnchor = {};
    m_areaSelectionCurrent = {};
    m_selectionBeforeArea = {};
    m_hasSelectionBeforeArea = false;
    if (!valid)
    {
        if (!paintMode && combine == SelectionCombine::Replace)
        {
            pushSelectionChange(previousSelection, {}, tr("Deselect"));
        }
        else
        {
            updateSelectionAnimation();
            requestDisplayUpdate();
        }
        return;
    }

    const QSize size = m_controller->document().size;
    QImage mask(size, QImage::Format_Grayscale8);
    mask.fill(0);
    QPainter painter(&mask);
    painter.setPen(Qt::NoPen);
    painter.setBrush(Qt::white);
    painter.drawPath(path);
    painter.end();
    if (paintMode)
    {
        commitFrozenFill(mask);
        updateSelectionAnimation();
        requestDisplayUpdate();
        return;
    }
    if (combine == SelectionCombine::Replace)
    {
        applySelectionMask(std::move(mask), previousSelection);
        return;
    }
    pushSelectionChange(previousSelection,
        selectionStateForMask(
            combinedSelectionMask(previousSelection.mask, mask, combine)),
        combine == SelectionCombine::Add ? tr("Add to selection")
                                         : tr("Subtract from selection"));
}

void CanvasWidget::cancelAreaSelection()
{
    if (!m_areaSelectionActive && m_areaSelectionPoints.isEmpty())
    {
        return;
    }
    m_areaSelectionActive = false;
    m_areaSelectionPoints.clear();
    m_areaSelectionAnchor = {};
    m_areaSelectionCurrent = {};
    m_selectionBeforeArea = {};
    m_hasSelectionBeforeArea = false;
    updateSelectionAnimation();
    requestDisplayUpdate();
}

bool CanvasWidget::canFinishAreaSelection() const
{
    if (m_selectionShape == SelectionShape::Freehand)
    {
        return m_areaSelectionPoints.size() >= 3;
    }
    const QRectF bounds =
        QRectF(m_areaSelectionAnchor, m_areaSelectionCurrent).normalized();
    return bounds.width() >= 1.0 && bounds.height() >= 1.0;
}

QPainterPath CanvasWidget::areaSelectionPath() const
{
    QPainterPath path;
    switch (m_selectionShape)
    {
    case SelectionShape::Freehand:
        if (m_areaSelectionPoints.isEmpty())
        {
            return path;
        }
        path.moveTo(m_areaSelectionPoints.first());
        for (int index = 1; index < m_areaSelectionPoints.size(); ++index)
        {
            path.lineTo(m_areaSelectionPoints.at(index));
        }
        return path;
    case SelectionShape::Rectangle:
        path.addRect(
            QRectF(m_areaSelectionAnchor, m_areaSelectionCurrent).normalized());
        return path;
    case SelectionShape::Ellipse:
        path.addEllipse(
            QRectF(m_areaSelectionAnchor, m_areaSelectionCurrent).normalized());
        return path;
    }
    return path;
}

void CanvasWidget::applySelectionMask(
    QImage mask, const SelectionState &previousSelection)
{
    const SelectionState nextSelection = selectionStateForMask(std::move(mask));
    pushSelectionChange(previousSelection, nextSelection, tr("Select area"));
}

CanvasWidget::SelectionState CanvasWidget::selectionStateForMask(
    QImage mask) const
{
    const Document &document = m_controller->document();
    const Layer *layer = document.layer(document.activeLayerId);
    if (mask.isNull() || !maskHasContent(mask) || !layer)
    {
        return {};
    }

    SelectionState state;
    state.mask = std::move(mask);
    state.layer = document.activeLayerId;
    return state;
}

CanvasWidget::SelectionState CanvasWidget::currentSelectionState() const
{
    return {m_selectedStrokes, m_selectionLayer, m_selectionMask};
}

void CanvasWidget::restoreSelectionState(const SelectionState &state)
{
    cancelSelectionMove();
    resetSelectionTransformSession();
    const Document &document = m_controller->document();
    const Layer *layer = document.layer(state.layer);
    if (state.mask.isNull() || !layer || state.mask.size() != document.size)
    {
        clearSelection();
        return;
    }

    m_selectedStrokes.clear();
    m_selectionLayer = state.layer;
    m_selectionMask = state.mask;
    m_movingSelection = false;
    setSelectionMoveMode(false);
    rebuildSelectionOutline();
    updateSelectionAnimation();
    notifySelectionTransformAvailability();
    requestDisplayUpdate();
    evaluateSelectionVisibility();
}

void CanvasWidget::cancelSelectionVisibilityEvaluation()
{
    if (m_selectionVisibilityCancellation)
    {
        m_selectionVisibilityCancellation->store(
            true, std::memory_order_relaxed);
        m_selectionVisibilityCancellation.reset();
    }
    ++m_selectionVisibilityGeneration;
}

void CanvasWidget::applySelectionVisibility(bool hasVisiblePixels)
{
    QSet<QUuid> selected;
    const Layer *layer = m_controller->document().layer(m_selectionLayer);
    if (hasVisiblePixels && layer)
    {
        for (const Stroke &stroke : layer->strokes)
        {
            selected.insert(stroke.id);
        }
    }
    m_selectedStrokes = std::move(selected);
    notifySelectionTransformAvailability();
    const bool armedForThisSelection =
        std::exchange(m_armSelectionMoveMode, false)
        && m_armSelectionMoveLayer == m_selectionLayer
        && m_armSelectionMoveMaskKey == m_selectionMask.cacheKey();
    if (armedForThisSelection && !m_selectedStrokes.isEmpty())
    {
        setSelectionMoveMode(true);
    }
    emit interactionMessage(
        m_selectedStrokes.isEmpty()
            ? tr("No content in the selected area.")
            : tr("Selected content. Use the action bar to transform or remove "
                 "it."));
    requestDisplayUpdate();
}

void CanvasWidget::evaluateSelectionVisibility()
{
    const Document document = m_controller->document();
    const QUuid layerId = m_selectionLayer;
    const QImage mask = m_selectionMask;
    const qint64 maskKey = mask.cacheKey();
    const int frame = m_currentFrame;
    const Layer *layer = document.layer(layerId);
    if (!layer || !layer->visible || layer->opacity <= 0.0 || mask.isNull())
    {
        cancelSelectionVisibilityEvaluation();
        m_armSelectionMoveMode = false;
        m_selectedStrokes.clear();
        if (layer && !mask.isNull())
        {
            m_controller->cacheSelectionVisibility(layerId, mask, false);
        }
        notifySelectionTransformAvailability();
        emit interactionMessage(tr("No content in the selected area."));
        requestDisplayUpdate();
        return;
    }

    // Restoring a selection the document has not changed under - escaping a
    // lasso, undoing back onto it - asks a question the controller already
    // answered. Rendering it again would cost the whole animation.
    const std::optional<bool> cached =
        m_controller->cachedSelectionVisibility(layerId, mask);
    cancelSelectionVisibilityEvaluation();
    if (cached)
    {
        applySelectionVisibility(*cached);
        return;
    }

    const quint64 generation = m_selectionVisibilityGeneration;
    const auto cancellation = std::make_shared<std::atomic_bool>(false);
    m_selectionVisibilityCancellation = cancellation;
    // QObject parenting retains the watcher until its finished callback queues
    // deleteLater(); the analyzer does not model either Qt ownership mechanism.
    // NOLINTBEGIN(clang-analyzer-cplusplus.NewDeleteLeaks)
    auto *watcher = new QFutureWatcher<SelectionVisibility::Result>(this);
    connect(watcher,
        &QFutureWatcher<SelectionVisibility::Result>::finished,
        this,
        [this, watcher, generation, layerId, maskKey]()
        {
            const SelectionVisibility::Result result =
                watchedFutureResult(*watcher);
            watcher->deleteLater();
            if (generation != m_selectionVisibilityGeneration)
            {
                return;
            }
            m_selectionVisibilityCancellation.reset();
            if (layerId != m_selectionLayer
                || maskKey != m_selectionMask.cacheKey())
            {
                return;
            }
            if (result.renderSucceeded)
            {
                m_controller->cacheSelectionVisibility(
                    m_selectionLayer, m_selectionMask, result.hasVisiblePixels);
            }
            applySelectionVisibility(
                result.renderSucceeded && result.hasVisiblePixels);
        });
    watcher->setFuture(QtConcurrent::run(
        [document, layerId, mask, frame, cancellation]()
        {
            const Layer *snapshotLayer = document.layer(layerId);
            return snapshotLayer ? SelectionVisibility::evaluate(document,
                                       *snapshotLayer,
                                       mask,
                                       frame,
                                       cancellation.get())
                                 : SelectionVisibility::Result();
        }));
}
// NOLINTEND(clang-analyzer-cplusplus.NewDeleteLeaks)

void CanvasWidget::pushSelectionChange(const SelectionState &previousSelection,
    const SelectionState &nextSelection,
    const QString &text)
{
    const bool bothEmpty =
        previousSelection.mask.isNull() && nextSelection.mask.isNull();
    if (bothEmpty)
    {
        restoreSelectionState(nextSelection);
        return;
    }

    m_controller->pushSelectionStateCommand(text,
        previousSelection.layer,
        previousSelection.mask,
        nextSelection.layer,
        nextSelection.mask);
}

void CanvasWidget::computeWandSelection(
    const QPointF &documentPosition, SelectionCombine combine)
{
    cancelSelectionTransformForBoundary(
        tr("The pending selection transform was canceled before selecting."));
    const SelectionState previousSelection = currentSelectionState();
    if (combine == SelectionCombine::Replace)
    {
        clearSelection();
    }
    const QSize size = m_controller->document().size;
    const QRectF bounds(QPointF(0.0, 0.0), QSizeF(size));
    if (!bounds.contains(documentPosition))
    {
        if (combine == SelectionCombine::Replace)
        {
            pushSelectionChange(previousSelection, {}, tr("Deselect"));
        }
        return;
    }
    const QPoint seed(
        std::clamp(static_cast<int>(documentPosition.x()), 0, size.width() - 1),
        std::clamp(
            static_cast<int>(documentPosition.y()), 0, size.height() - 1));

    QImage referenceImage;
    switch (m_wandReference)
    {
    case WandReference::ActiveLayer:
        referenceImage = renderActiveLayerImage();
        break;
    case WandReference::ReferenceLayers:
        referenceImage = renderReferenceLayersImage();
        break;
    case WandReference::AllVisibleLayers:
        referenceImage = renderAllVisibleLayersImage();
        break;
    }
    if (referenceImage.isNull())
    {
        if (combine == SelectionCombine::Replace)
        {
            pushSelectionChange(previousSelection, {}, tr("Deselect"));
        }
        if (m_wandReference == WandReference::ReferenceLayers)
        {
            emit interactionMessage(
                tr("Set a visible paint layer as a reference layer first."));
        }
        return;
    }
    const QImage mask = FloodFillMask::fromImage(referenceImage,
        seed,
        floodComparison(m_fillComparison),
        m_fillTolerance);
    if (mask.isNull())
    {
        if (combine == SelectionCombine::Replace)
        {
            pushSelectionChange(previousSelection, {}, tr("Deselect"));
        }
        emit interactionMessage(
            tr("Click an empty area surrounded by lines to select it."));
        return;
    }
    if (combine == SelectionCombine::Replace)
    {
        applySelectionMask(mask, previousSelection);
        return;
    }
    pushSelectionChange(previousSelection,
        selectionStateForMask(
            combinedSelectionMask(previousSelection.mask, mask, combine)),
        combine == SelectionCombine::Add ? tr("Add to selection")
                                         : tr("Subtract from selection"));
}

void CanvasWidget::applyBucketFill(const QPointF &documentPosition)
{
    const Document &document = m_controller->document();
    const QRectF bounds(QPointF(0.0, 0.0), QSizeF(document.size));
    const Layer *layer = document.layer(document.activeLayerId);
    if (!bounds.contains(documentPosition))
    {
        return;
    }
    if (!layer)
    {
        emit interactionMessage(tr("Add a layer before using this tool."));
        return;
    }
    if (m_groupSelectionActive)
    {
        emit interactionMessage(
            tr("Groups can't be painted on. Select a paint layer to draw."));
        return;
    }
    cancelSelectionTransformForBoundary(
        tr("The pending selection transform was canceled before filling."));
    if (!m_selectionMask.isNull()
        && (m_selectionLayer != document.activeLayerId
            || !selectionContains(documentPosition)))
    {
        emit interactionMessage(
            tr("Click inside the selected area to fill it."));
        return;
    }
    if (!reportLayerAcceptsPaint(*layer))
    {
        return;
    }

    const QPoint seed(std::clamp(static_cast<int>(documentPosition.x()),
                          0,
                          document.size.width() - 1),
        std::clamp(static_cast<int>(documentPosition.y()),
            0,
            document.size.height() - 1));
    QImage referenceImage;
    switch (m_wandReference)
    {
    case WandReference::ActiveLayer:
        referenceImage = renderActiveLayerImage();
        break;
    case WandReference::ReferenceLayers:
        referenceImage = renderReferenceLayersImage();
        break;
    case WandReference::AllVisibleLayers:
        referenceImage = renderAllVisibleLayersImage();
        break;
    }
    if (referenceImage.isNull())
    {
        if (m_wandReference == WandReference::ReferenceLayers)
        {
            emit interactionMessage(
                tr("Set a visible paint layer as a reference layer first."));
        }
        return;
    }
    const QImage coverage = FloodFillMask::fromImage(referenceImage,
        seed,
        floodComparison(m_fillComparison),
        m_fillTolerance);
    if (coverage.isNull())
    {
        emit interactionMessage(tr("No fillable area was found."));
        return;
    }

    commitFrozenFill(coverage);
}

bool CanvasWidget::fillSelection()
{
    cancelSelectionTransformForBoundary(
        tr("The pending selection transform was canceled before filling."));
    const Document &document = m_controller->document();
    if (m_selectionMask.isNull() || m_selectionLayer != document.activeLayerId)
    {
        emit interactionMessage(tr("Select an area on this layer to fill."));
        return false;
    }
    if (m_groupSelectionActive)
    {
        emit interactionMessage(
            tr("Groups can't be painted on. Select a paint layer to draw."));
        return false;
    }
    // Held by value because committing the fill can prune the selection, and
    // commitFrozenFill clips to the same mask, so the fill lands exactly
    // inside the marching ants instead of bleeding a pixel past them.
    const QImage mask = m_selectionMask;
    commitFrozenFill(mask);
    return true;
}

void CanvasWidget::commitFrozenFill(const QImage &coverage)
{
    const Document &document = m_controller->document();
    const Layer *layer = document.layer(document.activeLayerId);
    if (!layer || layer->kind != LayerKind::Paint
        || coverage.size() != document.size
        || coverage.format() != QImage::Format_Grayscale8)
    {
        emit interactionMessage(tr("The fill could not be added."));
        return;
    }
    if (!reportLayerAcceptsPaint(*layer))
    {
        return;
    }
    const std::optional<PackedMaskRegion> packedCoverage =
        packBinaryMask(coverage);
    if (!packedCoverage)
    {
        emit interactionMessage(tr("No fillable area was found."));
        return;
    }

    Stroke fillStroke;
    fillStroke.seed = QRandomGenerator::global()->generate64();
    fillStroke.mode = StrokeMode::Fill;
    fillStroke.color = m_brushColor;
    fillStroke.width = std::clamp(m_brushWidth,
        DocumentLimits::minimumStrokeWidth,
        DocumentLimits::maximumStrokeWidth);
    fillStroke.brush = m_brushSettings;
    fillStroke.brush.antialiasing = m_bucketAntialiasing;
    fillStroke.fillCoverage = *packedCoverage;
    if (!m_selectionMask.isNull())
    {
        fillStroke.clipMask = m_selectionMask;
    }
    fillStroke.points.append({QPointF(packedCoverage->bounds.center()), 1.0});
    commitStroke(document.activeLayerId, std::move(fillStroke));
}

void CanvasWidget::beginSelectionMove(const QPointF &documentPosition)
{
    const Document &document = m_controller->document();
    const Layer *layer = document.layer(m_selectionLayer);
    if (!layer || m_selectionMask.isNull() || !hasTransformableSelection())
    {
        return;
    }
    const bool alreadyActive = hasSelectionTransformSession();
    if (!beginSelectionTransformSession())
    {
        emit interactionMessage(
            tr("The selection transform could not be started."));
        return;
    }
    m_movingSelection = true;
    m_moveStartPosition = documentPosition;
    m_moveBaseTransform = m_selectionTransformSession.transform;
    m_moveStartedTransformSession = !alreadyActive;
    updateSelectionActionBar();
    requestDisplayUpdate();
}

void CanvasWidget::continueSelectionMove(const QPointF &documentPosition)
{
    if (!m_movingSelection)
    {
        return;
    }
    const QRectF baseBounds =
        m_moveBaseTransform.mapRect(m_selectionTransformSession.sourceBounds);
    const QPointF delta = safeSelectionDeltaForBounds(
        documentPosition - m_moveStartPosition, baseBounds);
    QTransform translation;
    translation.translate(delta.x(), delta.y());
    setPendingSelectionTransform(m_moveBaseTransform * translation);
}

void CanvasWidget::commitSelectionMove()
{
    if (!m_movingSelection)
    {
        return;
    }
    m_movingSelection = false;
    if (m_moveStartedTransformSession && !hasPendingSelectionTransform())
    {
        resetSelectionTransformSession();
    }
    m_moveStartedTransformSession = false;
    m_moveBaseTransform = QTransform();
    updateSelectionActionBar();
    updateCursor();
    requestDisplayUpdate();
}

void CanvasWidget::cancelSelectionMove()
{
    if (!m_movingSelection)
    {
        return;
    }
    const bool startedSession = m_moveStartedTransformSession;
    const QTransform baseTransform = m_moveBaseTransform;
    m_movingSelection = false;
    m_moveStartedTransformSession = false;
    m_moveBaseTransform = QTransform();
    if (startedSession)
    {
        resetSelectionTransformSession();
    }
    else if (m_selectionTransformSession.active)
    {
        setPendingSelectionTransform(baseTransform);
    }
    updateSelectionActionBar();
    updateCursor();
    requestDisplayUpdate();
}

bool CanvasWidget::beginSelectionTransformSession()
{
    if (m_selectionTransformSession.active)
    {
        return true;
    }
    if (m_selectionMask.isNull() || m_selectionOutline.isEmpty()
        || m_selectedStrokes.isEmpty()
        || !m_controller->document().layer(m_selectionLayer))
    {
        return false;
    }
    const std::optional<PixelSelectionOp> previewOperation =
        makePixelSelectionOp(m_selectionMask, QTransform(), true, true);
    if (!previewOperation)
    {
        return false;
    }

    FloatingTransformSession session;
    session.active = true;
    session.layer = m_selectionLayer;
    session.strokeIds =
        QVector<QUuid>(m_selectedStrokes.cbegin(), m_selectedStrokes.cend());
    session.sourceMask = m_selectionMask;
    session.sourceOutline = m_selectionOutline;
    session.sourceBounds = QRectF(previewOperation->sourceBounds);
    session.previewOperation = *previewOperation;
    session.transform = QTransform();
    m_selectionTransformSession = std::move(session);
    emit selectionTransformSessionChanged(true, false);
    updateSelectionActionBar();
    requestDisplayUpdate();
    return true;
}

bool CanvasWidget::setPendingSelectionTransform(const QTransform &transform)
{
    if (!m_selectionTransformSession.active
        || !isValidSelectionTransform(transform))
    {
        return false;
    }
    m_selectionTransformSession.transform = transform;
    m_selectionTransformSession.previewOperation.transform = transform;
    m_selectionTransformSession.previewOperation.sampling =
        selectionSamplingForTransform(transform);
    emit selectionTransformSessionChanged(true, !fuzzyIdentity(transform));
    updateSelectionActionBar();
    requestDisplayUpdate();
    return true;
}

bool CanvasWidget::isValidSelectionTransform(const QTransform &transform) const
{
    if (!m_selectionTransformSession.active)
    {
        return false;
    }
    PixelSelectionOp operation = m_selectionTransformSession.previewOperation;
    operation.transform = transform;
    operation.sampling = selectionSamplingForTransform(transform);
    return isValidPixelSelectionOp(operation);
}

SamplingMode CanvasWidget::selectionSamplingForTransform(
    const QTransform &transform) const
{
    return m_selectionTransformSampling == SamplingMode::Nearest
               ? SamplingMode::Nearest
               : samplingForSelectionTransform(transform);
}

void CanvasWidget::resetSelectionTransformSession()
{
    if (!m_selectionTransformSession.active)
    {
        return;
    }
    m_selectionTransformSession = {};
    m_moveBaseTransform = QTransform();
    m_moveStartedTransformSession = false;
    emit selectionTransformSessionChanged(false, false);
    updateSelectionActionBar();
    requestDisplayUpdate();
}

void CanvasWidget::cancelSelectionTransformForBoundary(const QString &message)
{
    if (!hasSelectionTransformSession())
    {
        return;
    }
    cancelSelectionMove();
    resetSelectionTransformSession();
    setSelectionMoveMode(false);
    if (!message.isEmpty())
    {
        emit interactionMessage(message);
    }
}

QPainterPath CanvasWidget::displayedSelectionOutline() const
{
    return m_selectionTransformSession.active
               ? m_selectionTransformSession.transform.map(
                     m_selectionTransformSession.sourceOutline)
               : m_selectionOutline;
}

QRectF CanvasWidget::displayedSelectionBounds() const
{
    return displayedSelectionOutline().boundingRect();
}

QPointF CanvasWidget::safeSelectionDeltaForBounds(
    const QPointF &delta, const QRectF &bounds) const
{
    if (!std::isfinite(delta.x()) || !std::isfinite(delta.y())
        || !bounds.isValid())
    {
        return {};
    }
    const qreal maximum = DocumentLimits::maximumStoredCoordinateMagnitude;
    return QPointF(
        std::clamp(
            delta.x(), -maximum - bounds.left(), maximum - bounds.right()),
        std::clamp(
            delta.y(), -maximum - bounds.top(), maximum - bounds.bottom()));
}

void CanvasWidget::clearSelection()
{
    cancelSelectionVisibilityEvaluation();
    m_armSelectionMoveMode = false;
    m_armSelectionMoveLayer = QUuid();
    m_armSelectionMoveMaskKey = 0;
    setSelectionMoveMode(false);
    resetSelectionTransformSession();
    if (m_selectedStrokes.isEmpty() && m_selectionMask.isNull()
        && !m_movingSelection)
    {
        updateSelectionAnimation();
        updateSelectionActionBar();
        return;
    }
    m_selectedStrokes.clear();
    m_selectionMask = QImage();
    m_selectionOutline = QPainterPath();
    m_selectionLayer = QUuid();
    m_movingSelection = false;
    updateSelectionAnimation();
    notifySelectionTransformAvailability();
    updateSelectionActionBar();
    requestDisplayUpdate();
}

void CanvasWidget::pruneSelection()
{
    if (m_selectionMask.isNull())
    {
        return;
    }
    const Document &document = m_controller->document();
    const Layer *layer = document.layer(m_selectionLayer);
    if (!layer || document.size != m_selectionMask.size())
    {
        clearSelection();
        return;
    }

    m_selectedStrokes.clear();
    notifySelectionTransformAvailability();
    requestDisplayUpdate();
    evaluateSelectionVisibility();
}

void CanvasWidget::transformSelectionOverlay(const QUuid &layerId,
    const QVector<QUuid> &strokeIds,
    const QTransform &transform)
{
    cancelSelectionTransformForBoundary();
    if (m_selectionMask.isNull() || m_selectionLayer != layerId)
    {
        return;
    }
    const QSet<QUuid> transformed(strokeIds.cbegin(), strokeIds.cend());
    const bool affectsSelection = std::any_of(m_selectedStrokes.cbegin(),
        m_selectedStrokes.cend(),
        [&transformed](const QUuid &id)
        {
            return transformed.contains(id);
        });
    if (!affectsSelection)
    {
        return;
    }
    const std::optional<PixelSelectionOp> operation =
        makePixelSelectionOp(m_selectionMask, transform, false, true);
    const QImage transformedSelection =
        operation ? transformedSelectionSupport(m_selectionMask,
                        m_selectionMask.size(),
                        transform,
                        operation->sampling)
                  : QImage();
    if (transformedSelection.isNull() || !maskHasContent(transformedSelection))
    {
        clearSelection();
        return;
    }
    m_selectionMask = transformedSelection;
    rebuildSelectionOutline();
    requestDisplayUpdate();
}

void CanvasWidget::handleStrokesDuplicated(const QUuid &layerId,
    const QVector<QUuid> &sourceIds,
    const QVector<QUuid> &duplicateIds,
    const QPointF &delta,
    bool duplicated)
{
    cancelSelectionTransformForBoundary();
    if (m_selectionMask.isNull() || m_selectionLayer != layerId
        || sourceIds.size() != duplicateIds.size())
    {
        return;
    }
    const QVector<QUuid> &fromIds = duplicated ? sourceIds : duplicateIds;
    const QVector<QUuid> &toIds = duplicated ? duplicateIds : sourceIds;
    const QSet<QUuid> from(fromIds.cbegin(), fromIds.cend());
    const bool affectsSelection = std::any_of(m_selectedStrokes.cbegin(),
        m_selectedStrokes.cend(),
        [&from](const QUuid &id)
        {
            return from.contains(id);
        });
    if (!affectsSelection)
    {
        return;
    }
    QSet<QUuid> remapped = m_selectedStrokes;
    for (int index = 0; index < fromIds.size(); ++index)
    {
        if (remapped.remove(fromIds[index]))
        {
            remapped.insert(toIds[index]);
        }
    }
    m_selectedStrokes = std::move(remapped);
    QTransform transform;
    const QPointF appliedDelta = duplicated ? delta : -delta;
    transform.translate(appliedDelta.x(), appliedDelta.y());
    const std::optional<PixelSelectionOp> operation =
        makePixelSelectionOp(m_selectionMask, transform, false, true);
    const QImage transformedSelection =
        operation ? transformedSelectionSupport(m_selectionMask,
                        m_selectionMask.size(),
                        transform,
                        operation->sampling)
                  : QImage();
    if (transformedSelection.isNull() || !maskHasContent(transformedSelection))
    {
        clearSelection();
        return;
    }
    m_selectionMask = transformedSelection;
    rebuildSelectionOutline();
    notifySelectionTransformAvailability();
    requestDisplayUpdate();
}

void CanvasWidget::handleSelectionOverlayTransition(const QUuid &layerId,
    const QVector<QUuid> &fromStrokeIds,
    const QVector<QUuid> &toStrokeIds,
    const QImage &fromMask,
    const QImage &toMask)
{
    cancelSelectionTransformForBoundary();
    if (m_selectionMask.isNull() || m_selectionLayer != layerId
        || fromStrokeIds.isEmpty() || toStrokeIds.isEmpty()
        || fromMask.size() != m_selectionMask.size()
        || toMask.size() != m_selectionMask.size()
        || fromMask.format() != QImage::Format_Grayscale8
        || toMask.format() != QImage::Format_Grayscale8)
    {
        return;
    }
    if (m_selectionMask.cacheKey() != fromMask.cacheKey()
        && m_selectionMask != fromMask)
    {
        return;
    }

    for (const QUuid &strokeId : fromStrokeIds)
    {
        m_selectedStrokes.remove(strokeId);
    }
    for (const QUuid &strokeId : toStrokeIds)
    {
        m_selectedStrokes.insert(strokeId);
    }
    m_selectionMask = toMask;
    m_movingSelection = false;
    rebuildSelectionOutline();
    updateSelectionAnimation();
    notifySelectionTransformAvailability();
    updateSelectionActionBar();
    requestDisplayUpdate();
}

void CanvasWidget::handleCanvasResized(const QSize &previousSize,
    const QSize &currentSize,
    const QTransform &transform)
{
    cancelSelectionTransformForBoundary(
        tr("The pending selection transform was canceled before resizing."));
    if (!m_selectionMask.isNull() && m_selectionMask.size() == previousSize)
    {
        const QImage transformedSelection =
            transformedMask(m_selectionMask, currentSize, transform);
        if (transformedSelection.isNull()
            || !maskHasContent(transformedSelection))
        {
            clearSelection();
        }
        else
        {
            m_selectionMask = transformedSelection;
            rebuildSelectionOutline();
        }
    }
    // The rollback snapshot a live lasso would restore has to follow the
    // resize the same way the live selection does, or the restore's size
    // guard discards it.
    if (m_hasSelectionBeforeArea && !m_selectionBeforeArea.mask.isNull()
        && m_selectionBeforeArea.mask.size() == previousSize)
    {
        const QImage transformedSnapshot =
            transformedMask(m_selectionBeforeArea.mask, currentSize, transform);
        if (transformedSnapshot.isNull()
            || !maskHasContent(transformedSnapshot))
        {
            m_selectionBeforeArea = {};
        }
        else
        {
            m_selectionBeforeArea.mask = transformedSnapshot;
        }
    }
    for (QPointF &point : m_areaSelectionPoints)
    {
        point = transform.map(point);
    }
    if (m_areaSelectionActive)
    {
        m_areaSelectionAnchor = transform.map(m_areaSelectionAnchor);
        m_areaSelectionCurrent = transform.map(m_areaSelectionCurrent);
    }
    setSelectionMoveMode(false);
    fitToWindow();
}

void CanvasWidget::rebuildSelectionOutline()
{
    m_selectionOutline = outlinePath(m_selectionMask);
}

void CanvasWidget::updateSelectionAnimation()
{
    const bool active = m_areaSelectionActive || !m_selectionMask.isNull();
    if (active && !m_selectionAnimationTimer.isActive())
    {
        m_selectionAnimationTimer.start();
    }
    else if (!active && m_selectionAnimationTimer.isActive())
    {
        m_selectionAnimationTimer.stop();
        m_selectionDashOffset = 0.0;
    }
}

void CanvasWidget::notifySelectionTransformAvailability()
{
    emit selectionTransformAvailabilityChanged(hasTransformableSelection());
    emit selectionAvailabilityChanged(
        hasSelection(), hasTransformableSelection());
    updateSelectionActionBar();
}

bool CanvasWidget::flipSelection(bool horizontal)
{
    if (!hasTransformableSelection())
    {
        return false;
    }
    setSelectionMoveMode(false);
    const bool alreadyActive = hasSelectionTransformSession();
    if (!beginSelectionTransformSession())
    {
        return false;
    }
    const QPointF center = displayedSelectionBounds().center();
    QTransform delta;
    delta.translate(center.x(), center.y());
    delta.scale(horizontal ? -1.0 : 1.0, horizontal ? 1.0 : -1.0);
    delta.translate(-center.x(), -center.y());
    if (!setPendingSelectionTransform(
            m_selectionTransformSession.transform * delta))
    {
        if (!alreadyActive)
        {
            resetSelectionTransformSession();
        }
        emit interactionMessage(
            tr("The selected content could not be flipped."));
        return false;
    }
    return true;
}

void CanvasWidget::updateSelectionActionBar()
{
    if (!m_selectionActionBar)
    {
        return;
    }
    if (m_selectionMask.isNull() || m_selectionOutline.isEmpty()
        || m_areaSelectionActive || m_movingSelection)
    {
        m_selectionActionBar->hide();
        return;
    }

    m_selectionActionBar->adjustSize();
    const QRectF widgetBounds =
        documentTransform().map(displayedSelectionOutline()).boundingRect();
    const QSize barSize = m_selectionActionBar->size();
    constexpr int edgeMargin = 8;
    constexpr int selectionGap = 4;
    const int x = barSize.width() + 2 * edgeMargin <= width()
                      ? std::clamp(qRound(widgetBounds.center().x()
                                          - barSize.width() * 0.5),
                            edgeMargin,
                            width() - barSize.width() - edgeMargin)
                      : (width() - barSize.width()) / 2;
    int y = qCeil(widgetBounds.bottom()) + selectionGap;
    if (y + barSize.height() > height() - edgeMargin)
    {
        y = qFloor(widgetBounds.top()) - barSize.height() - selectionGap;
    }
    y = barSize.height() + 2 * edgeMargin <= height()
            ? std::clamp(
                  y, edgeMargin, height() - barSize.height() - edgeMargin)
            : (height() - barSize.height()) / 2;
    m_selectionActionBar->move(x, y);
    m_selectionActionBar->show();
    m_selectionActionBar->raise();
}

QPointF CanvasWidget::clampedSelectionDelta(const QPointF &delta) const
{
    const Document &document = m_controller->document();
    const Layer *layer = document.layer(m_selectionLayer);
    if (!layer || m_selectedStrokes.isEmpty())
    {
        return delta;
    }
    if (!m_selectionMask.isNull() && !m_selectionOutline.isEmpty())
    {
        const QRectF bounds = m_selectionOutline.boundingRect();
        return QPointF(
            std::clamp(delta.x(),
                -bounds.left(),
                static_cast<qreal>(document.size.width()) - bounds.right()),
            std::clamp(delta.y(),
                -bounds.top(),
                static_cast<qreal>(document.size.height()) - bounds.bottom()));
    }
    qreal minX = document.size.width();
    qreal minY = document.size.height();
    qreal maxX = 0.0;
    qreal maxY = 0.0;
    bool found = false;
    for (const Stroke &stroke : layer->strokes)
    {
        if (!m_selectedStrokes.contains(stroke.id))
        {
            continue;
        }
        for (const StrokePoint &point : stroke.points)
        {
            minX = std::min(minX, point.position.x());
            minY = std::min(minY, point.position.y());
            maxX = std::max(maxX, point.position.x());
            maxY = std::max(maxY, point.position.y());
            found = true;
        }
    }
    if (!found)
    {
        return delta;
    }
    return QPointF(
        std::clamp(
            delta.x(), -minX, static_cast<qreal>(document.size.width()) - maxX),
        std::clamp(delta.y(),
            -minY,
            static_cast<qreal>(document.size.height()) - maxY));
}

QImage CanvasWidget::renderActiveLayerImage() const
{
    const Document document = displayDocument();
    const Layer *layer = document.layer(document.activeLayerId);
    if (!layer)
    {
        return {};
    }
    return RenderEngine::render(
        DocumentOperations::isolatedLayerDocument(document, *layer),
        m_currentFrame);
}

QImage CanvasWidget::renderReferenceLayersImage() const
{
    Document document = displayDocument();
    if (!document.size.isValid())
    {
        return {};
    }
    bool hasVisibleReference = false;
    for (Layer &layer : document.layers)
    {
        if (layer.kind != LayerKind::Paint)
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
            || DocumentOperations::isLayerRenderable(document, layer);
    }
    if (!hasVisibleReference)
    {
        return {};
    }
    document.background = Qt::transparent;
    return RenderEngine::render(document, m_currentFrame);
}

QImage CanvasWidget::renderAllVisibleLayersImage() const
{
    Document document = displayDocument();
    document.background = Qt::transparent;
    return RenderEngine::render(document, m_currentFrame);
}

void CanvasWidget::drawSelectionOverlay(
    QPainter &painter, const QTransform &transform)
{
    painter.save();
    painter.setTransform(transform);

    if (m_areaSelectionActive)
    {
        const QPainterPath path = areaSelectionPath();
        if (!path.isEmpty())
        {
            drawSelectionPath(painter, path, m_selectionDashOffset);
        }
    }

    if (!m_selectionOutline.isEmpty())
    {
        drawSelectionPath(
            painter, displayedSelectionOutline(), m_selectionDashOffset);
    }
    painter.restore();
}
}

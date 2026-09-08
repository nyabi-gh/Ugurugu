// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "ui/CanvasWidget.hpp"

#include "brush/BrushPreset.hpp"
#include "brush/EraserPreset.hpp"
#include "document/DocumentLimits.hpp"
#include "document/DocumentOperations.hpp"
#include "document/SelectionOperation.hpp"
#include "document/SelectionVisibility.hpp"
#include "document/StrokeMask.hpp"
#include "io/SelectionClipboardCodec.hpp"
#include "render/PreviewRenderPolicy.hpp"
#include "render/RenderEngine.hpp"
#include "ui/CanvasFrameView.hpp"
#include "ui/CanvasOverlayView.hpp"
#include "ui/CanvasViewport.hpp"
#include "ui/SelectionActionBar.hpp"
#include "ui/Theme.hpp"

#include <QClipboard>
#include <QEnterEvent>
#include <QFutureWatcher>
#include <QGuiApplication>
#include <QHash>
#include <QKeyEvent>
#include <QMimeData>
#include <QMouseEvent>
#include <QNativeGestureEvent>
#include <QPaintEvent>
#include <QPainter>
#include <QPointer>
#include <QRandomGenerator>
#include <QResizeEvent>
#include <QTabletEvent>
#include <QThread>
#include <QWheelEvent>
#include <QtConcurrentRun>

#include <spdlog/spdlog.h>

#include <algorithm>
#include <cmath>
#include <utility>

namespace ugurugu
{

using namespace canvas_detail;

CanvasWidget::CanvasWidget(DocumentController *controller, QWidget *parent)
    : QWidget(parent)
    , m_controller(controller)
{
    setAttribute(Qt::WA_OpaquePaintEvent);
    setAttribute(Qt::WA_AcceptTouchEvents);
    setFocusPolicy(Qt::StrongFocus);
    setMouseTracking(true);
    setTabletTracking(true);
    setCursor(Qt::BlankCursor);
    m_frameCache.setMaxCost(PreviewRenderPolicy::maximumCacheKiB());
    const BrushPreset &defaultPreset = BrushPresetCatalog::defaultPreset();
    m_brushPresetId = defaultPreset.id;
    m_brushSettings = defaultPreset.settings;
    m_brushWidth = defaultPreset.defaultSize;
    m_brushPresetWidths.insert(m_brushPresetId, m_brushWidth);
    const EraserPreset &defaultEraser = EraserPresetCatalog::defaultPreset();
    m_eraserPresetId = defaultEraser.id;
    m_eraserSettings = defaultEraser.settings;
    m_eraserWidth = defaultEraser.defaultSize;
    m_eraserPresetWidths.insert(m_eraserPresetId, m_eraserWidth);

    connect(m_controller,
        &DocumentController::documentChanged,
        this,
        [this]()
        {
            cancelSelectionTransformForBoundary(
                tr("The pending selection transform was canceled because "
                   "the document changed."));
            const PendingStrokeRefreshHint hint =
                std::exchange(m_pendingStrokeRefreshHint, {});
            if (!hint.armed
                || !tryRegionalStrokeInvalidation(hint.layerId, hint.strokeId))
            {
                invalidateFrames();
            }
            pruneSelection();
        });
    connect(m_controller,
        &DocumentController::historyMovementStarting,
        this,
        &CanvasWidget::cancelActiveInteraction);
    connect(m_controller,
        &DocumentController::documentReplaced,
        this,
        [this]()
        {
            // A stroke or lasso that survives the replacement would commit
            // into the newly loaded document; the layer UUIDs even match when
            // the same file is reopened. Cancel first: the cancellation may
            // restore the pre-lasso selection captured from the outgoing
            // document, and the clear removes that too.
            cancelActiveInteraction();
            clearSelection();
        });
    connect(m_controller,
        &DocumentController::selectionHistoryStateRequested,
        this,
        [this](const QUuid &layerId, const QImage &mask)
        {
            restoreSelectionState({{}, layerId, mask});
        });
    connect(m_controller,
        &DocumentController::strokesTransformed,
        this,
        &CanvasWidget::transformSelectionOverlay);
    connect(m_controller,
        &DocumentController::strokesDuplicated,
        this,
        &CanvasWidget::handleStrokesDuplicated);
    connect(m_controller,
        &DocumentController::selectionOverlayTransition,
        this,
        &CanvasWidget::handleSelectionOverlayTransition);
    connect(m_controller,
        &DocumentController::canvasResized,
        this,
        &CanvasWidget::handleCanvasResized);
    connect(m_controller,
        &DocumentController::activeLayerChanged,
        this,
        [this](const QUuid &layerId)
        {
            if (!m_selectionLayer.isNull() && m_selectionLayer != layerId)
            {
                clearSelection();
            }
            if (!m_drawing)
            {
                cancelInteractionFrameWarmup();
                m_previewSplit = {};
                m_previewSplitLayer = QUuid();
                m_previewSplitFrame = -1;
                m_previewLayerRasters = {};
                m_previewLayerRasterFrame = -1;
                updateFrameCacheBudget();
                if (!m_animating)
                {
                    requestInteractionFrameWarmup(m_currentFrame);
                }
            }
        });
    // Windows delivers coarse timers as WM_TIMER, which the message queue only
    // generates once nothing else is pending. A stroke keeps pointer packets
    // and repaints in the queue continuously, so a coarse animation timer stops
    // firing for exactly as long as the pen is down. A precise timer is posted
    // instead of synthesised and survives that.
    m_animationTimer.setTimerType(Qt::PreciseTimer);
    connect(&m_animationTimer,
        &QTimer::timeout,
        this,
        [this]()
        {
            advanceFrame();
        });
    m_selectionAnimationTimer.setInterval(120);
    connect(&m_selectionAnimationTimer,
        &QTimer::timeout,
        this,
        [this]()
        {
            m_selectionDashOffset -= 1.0;
            requestDisplayUpdate();
        });
    m_zoomRenderTimer.setSingleShot(true);
    m_zoomRenderTimer.setInterval(zoomRenderDelayMilliseconds);
    connect(&m_zoomRenderTimer,
        &QTimer::timeout,
        this,
        [this]()
        {
            requestDisplayUpdate();
        });

    // Warmup renders on a dedicated pool: sharing the global QtConcurrent
    // pool would let a 30-frame warmup starve the layer thumbnail and
    // selection visibility workers that also run there. Two cores stay free
    // for the GUI thread and those workers.
    m_frameCacheWarmupPool.setMaxThreadCount(
        std::clamp(QThread::idealThreadCount() - 2, 1, 8));
    m_interactionFramePool.setMaxThreadCount(1);
    connect(&m_interactionFrameWatcher,
        &QFutureWatcher<PreparedInteractionFrame>::finished,
        this,
        &CanvasWidget::finishInteractionFrameWarmup);

    updateTimerInterval();
    m_animationTimer.start();
    scheduleFrameCacheWarmup();
    initializeDisplayViews();
}

CanvasWidget::~CanvasWidget()
{
    cancelFrameCacheWarmup();
    cancelInteractionFrameWarmup();
    cancelSelectionVisibilityEvaluation();
}

CanvasWidget::Tool CanvasWidget::tool() const
{
    return m_tool;
}

QColor CanvasWidget::brushColor() const
{
    return m_brushColor;
}

qreal CanvasWidget::brushWidth() const
{
    return m_brushWidth;
}

qreal CanvasWidget::brushPresetWidth(const QString &presetId) const
{
    const BrushPreset *preset = BrushPresetCatalog::find(presetId);
    if (!preset)
    {
        return 0.0;
    }
    return std::clamp(
        m_brushPresetWidths.value(preset->id, preset->defaultSize),
        DocumentLimits::minimumStrokeWidth,
        DocumentLimits::maximumStrokeWidth);
}

qreal CanvasWidget::eraserWidth() const
{
    return m_eraserWidth;
}

qreal CanvasWidget::eraserPresetWidth(const QString &presetId) const
{
    const EraserPreset *preset = EraserPresetCatalog::find(presetId);
    if (!preset)
    {
        return 0.0;
    }
    return std::clamp(
        m_eraserPresetWidths.value(preset->id, preset->defaultSize),
        DocumentLimits::minimumStrokeWidth,
        DocumentLimits::maximumStrokeWidth);
}

qreal CanvasWidget::brushStabilization() const
{
    return brushPresetStabilization(m_brushPresetId);
}

qreal CanvasWidget::brushPresetStabilization(const QString &presetId) const
{
    const BrushPreset *preset = BrushPresetCatalog::find(presetId);
    if (!preset)
    {
        return 0.0;
    }
    return std::clamp(
        m_brushPresetStabilizations.value(preset->id, 0.0), 0.0, 1.0);
}

qreal CanvasWidget::eraserStabilization() const
{
    return eraserPresetStabilization(m_eraserPresetId);
}

qreal CanvasWidget::eraserPresetStabilization(const QString &presetId) const
{
    const EraserPreset *preset = EraserPresetCatalog::find(presetId);
    if (!preset)
    {
        return 0.0;
    }
    return std::clamp(
        m_eraserPresetStabilizations.value(preset->id, 0.0), 0.0, 1.0);
}

bool CanvasWidget::brushAntialiasing() const
{
    return m_brushAntialiasing;
}

bool CanvasWidget::tabletPressureEnabled() const
{
    return m_tabletPressureEnabled;
}

bool CanvasWidget::isWobbleAnimationEnabled() const
{
    return m_wobbleAnimationEnabled;
}

Document CanvasWidget::displayDocument() const
{
    Document document = m_controller->document();
    applyWobbleAnimationSetting(document);
    return document;
}

void CanvasWidget::applyWobbleAnimationSetting(Document &document) const
{
    if (m_wobbleAnimationEnabled)
    {
        return;
    }
    document.wobbleAmount = 0.0;
    for (Layer &layer : document.layers)
    {
        if (layer.wobbleAmount)
        {
            layer.wobbleAmount = 0.0;
        }
    }
}

Document CanvasWidget::displayDocumentWithPendingSelectionTransform() const
{
    Document document = documentWithPendingSelectionTransform();
    applyWobbleAnimationSetting(document);
    return document;
}

QString CanvasWidget::brushPresetId() const
{
    return m_brushPresetId;
}

QString CanvasWidget::eraserPresetId() const
{
    return m_eraserPresetId;
}

CanvasWidget::WandReference CanvasWidget::wandReference() const
{
    return m_wandReference;
}

CanvasWidget::FillComparison CanvasWidget::fillComparison() const
{
    return m_fillComparison;
}

int CanvasWidget::fillTolerance() const
{
    return m_fillTolerance;
}

bool CanvasWidget::bucketAntialiasing() const
{
    return m_bucketAntialiasing;
}

CanvasWidget::SelectionShape CanvasWidget::selectionShape() const
{
    return m_selectionShape;
}

CanvasWidget::LassoMode CanvasWidget::lassoMode() const
{
    return m_lassoMode;
}

SamplingMode CanvasWidget::selectionTransformSampling() const
{
    return m_selectionTransformSampling;
}

bool CanvasWidget::isAnimating() const
{
    return m_animating;
}

int CanvasWidget::currentFrame() const
{
    return m_currentFrame;
}

qreal CanvasWidget::zoom() const
{
    return m_zoom;
}

qreal CanvasWidget::canvasRotation() const
{
    return m_canvasRotation;
}

bool CanvasWidget::isCanvasMirrored() const
{
    return m_canvasMirrored;
}

bool CanvasWidget::hasSelection() const
{
    return !m_selectionMask.isNull();
}

bool CanvasWidget::hasTransformableSelection() const
{
    return !m_selectedStrokes.isEmpty();
}

bool CanvasWidget::hasEditableStrokeSelection() const
{
    return !selectedStrokeIds().isEmpty();
}

QUuid CanvasWidget::selectionLayerId() const
{
    return m_selectionLayer;
}

QVector<QUuid> CanvasWidget::selectedStrokeIds() const
{
    const Document &document = m_controller->document();
    const Layer *layer = document.layer(m_selectionLayer);
    if (!layer || m_selectionMask.isNull())
    {
        return {};
    }
    const qint64 maskKey = m_selectionMask.cacheKey();
    if (m_editableStrokeMaskKey != maskKey
        || m_editableStrokeLayer != m_selectionLayer
        || m_editableStrokeFrame != m_currentFrame)
    {
        m_editableStrokeIds = SelectionVisibility::editableStrokeIds(
            document, *layer, m_selectionMask, m_currentFrame);
        m_editableStrokeMaskKey = maskKey;
        m_editableStrokeLayer = m_selectionLayer;
        m_editableStrokeFrame = m_currentFrame;
    }
    return m_editableStrokeIds;
}

bool CanvasWidget::selectionMoveMode() const
{
    return m_selectionMoveMode;
}

bool CanvasWidget::hasSelectionTransformSession() const
{
    return m_selectionTransformSession.active;
}

bool CanvasWidget::hasPendingSelectionTransform() const
{
    return m_selectionTransformSession.active
           && !fuzzyIdentity(m_selectionTransformSession.transform);
}

QTransform CanvasWidget::pendingSelectionTransform() const
{
    return m_selectionTransformSession.active
               ? m_selectionTransformSession.transform
               : QTransform();
}

Document CanvasWidget::documentWithPendingSelectionTransform() const
{
    Document document = m_controller->document();
    if (!hasPendingSelectionTransform())
    {
        return document;
    }
    Layer *layer = document.layer(m_selectionTransformSession.layer);
    if (!layer || layer->kind != LayerKind::Paint)
    {
        return document;
    }
    Stroke stroke;
    stroke.mode = StrokeMode::PixelSelection;
    stroke.pixelSelectionOp = m_selectionTransformSession.previewOperation;
    layer->strokes.append(std::move(stroke));
    return document;
}

bool CanvasWidget::scaleSelection(qreal factor)
{
    if (!std::isfinite(factor) || factor <= 0.0 || qFuzzyCompare(factor, 1.0)
        || !hasTransformableSelection())
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
    delta.scale(factor, factor);
    delta.translate(-center.x(), -center.y());
    if (!setPendingSelectionTransform(
            delta * m_selectionTransformSession.transform))
    {
        if (!alreadyActive)
        {
            resetSelectionTransformSession();
        }
        emit interactionMessage(tr("The selection could not be scaled."));
        return false;
    }
    return true;
}

bool CanvasWidget::rotateSelection(qreal degrees)
{
    if (!std::isfinite(degrees) || qFuzzyIsNull(degrees)
        || !hasTransformableSelection())
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
    delta.rotate(degrees);
    delta.translate(-center.x(), -center.y());
    if (!setPendingSelectionTransform(
            delta * m_selectionTransformSession.transform))
    {
        if (!alreadyActive)
        {
            resetSelectionTransformSession();
        }
        emit interactionMessage(tr("The selection could not be rotated."));
        return false;
    }
    return true;
}

bool CanvasWidget::flipSelectionHorizontally()
{
    return flipSelection(true);
}

bool CanvasWidget::flipSelectionVertically()
{
    return flipSelection(false);
}

bool CanvasWidget::applySelectionTransform()
{
    if (m_movingSelection)
    {
        commitSelectionMove();
    }
    if (!hasPendingSelectionTransform())
    {
        return false;
    }
    setSelectionMoveMode(false);

    const FloatingTransformSession session = m_selectionTransformSession;
    resetSelectionTransformSession();
    const bool applied = m_controller->transformSelection(session.layer,
        session.strokeIds,
        session.transform,
        session.sourceMask,
        session.previewOperation.sampling);
    if (!applied)
    {
        m_selectionTransformSession = session;
        emit selectionTransformSessionChanged(true, true);
        updateSelectionActionBar();
        requestDisplayUpdate();
        emit interactionMessage(
            tr("The selection transform could not be applied."));
        return false;
    }
    emit interactionMessage(tr("Selection transform applied."));
    return true;
}

void CanvasWidget::cancelSelectionTransform()
{
    if (!hasSelectionTransformSession())
    {
        return;
    }
    cancelSelectionMove();
    resetSelectionTransformSession();
    setSelectionMoveMode(false);
    emit interactionMessage(tr("Selection transform canceled."));
}

// A layer inside a hidden group is as invisible as a hidden layer, and its own
// flags do not say so. Without the hierarchy check the tools committed strokes
// and fills nobody could see, exactly the failure the per-layer guard exists to
// prevent.
bool CanvasWidget::reportLayerAcceptsPaint(const Layer &layer)
{
    if (!layer.visible)
    {
        emit interactionMessage(
            tr("The active layer is hidden. Make it visible to draw."));
        return false;
    }
    if (layer.opacity <= 0.0)
    {
        emit interactionMessage(
            tr("The active layer opacity is 0%. Increase it to draw."));
        return false;
    }
    if (!DocumentOperations::isLayerRenderable(m_controller->document(), layer))
    {
        emit interactionMessage(tr("The group holding the active layer is "
                                   "hidden. Make it visible to draw."));
        return false;
    }
    return true;
}

bool CanvasWidget::copySelectionToClipboard(
    SelectionClipboardCodec::Copy *outCopy)
{
    QString error;
    std::optional<SelectionClipboardCodec::Copy> copy =
        SelectionClipboardCodec::makeCopy(m_controller->document(),
            m_selectionLayer,
            m_selectionMask,
            m_currentFrame,
            &error);
    if (!copy)
    {
        emit interactionMessage(
            error.isEmpty() ? tr("The selection could not be copied.") : error);
        return false;
    }
    auto *mimeData = new QMimeData;
    mimeData->setData(SelectionClipboardCodec::mimeType(), copy->payload);
    mimeData->setImageData(copy->raster);
    QGuiApplication::clipboard()->setMimeData(mimeData);
    if (outCopy)
    {
        *outCopy = std::move(*copy);
    }
    return true;
}

bool CanvasWidget::copySelection()
{
    if (m_selectedStrokes.isEmpty() || m_selectionMask.isNull())
    {
        return false;
    }
    if (hasPendingSelectionTransform())
    {
        emit interactionMessage(
            tr("Apply or cancel the selection transform before copying."));
        return false;
    }
    SelectionClipboardCodec::Copy copy;
    if (!copySelectionToClipboard(&copy))
    {
        return false;
    }
    const QPointF delta = clampedSelectionDelta(QPointF(12.0, 12.0));
    if (m_controller->pasteLayer(std::move(copy.layer),
            copy.canvasSize,
            delta,
            m_selectionMask,
            copy.rasterAssets)
        != DocumentController::PasteLayerResult::Pasted)
    {
        emit interactionMessage(
            tr("The copy could not be placed on a new layer."));
        return false;
    }
    m_armSelectionMoveMode = true;
    m_armSelectionMoveLayer = m_selectionLayer;
    m_armSelectionMoveMaskKey = m_selectionMask.cacheKey();
    emit interactionMessage(
        tr("Copied to a new layer. Drag inside the selection to move it."));
    return true;
}

bool CanvasWidget::cutSelection()
{
    if (m_selectedStrokes.isEmpty() || m_selectionMask.isNull())
    {
        return false;
    }
    if (hasPendingSelectionTransform())
    {
        emit interactionMessage(
            tr("Apply or cancel the selection transform before cutting."));
        return false;
    }
    if (!copySelectionToClipboard(nullptr))
    {
        return false;
    }
    setSelectionMoveMode(false);
    DocumentUndoStack *undoStack = m_controller->undoStack();
    undoStack->beginMacro(tr("Cut selection"));
    const bool removed = m_controller->removeSelectedContent(m_selectionLayer,
        QVector<QUuid>(m_selectedStrokes.cbegin(), m_selectedStrokes.cend()),
        m_selectionMask);
    undoStack->endMacro();
    if (!removed)
    {
        emit interactionMessage(tr("The selection could not be cut."));
        return false;
    }
    emit interactionMessage(tr("Selection cut."));
    return true;
}

bool CanvasWidget::deleteSelection()
{
    if (m_selectedStrokes.isEmpty() || m_selectionMask.isNull())
    {
        return false;
    }
    cancelSelectionTransformForBoundary(
        tr("The pending transform was canceled before deleting."));
    setSelectionMoveMode(false);
    const bool removed = m_controller->removeSelectedContent(m_selectionLayer,
        QVector<QUuid>(m_selectedStrokes.cbegin(), m_selectedStrokes.cend()),
        m_selectionMask);
    if (!removed)
    {
        emit interactionMessage(
            tr("The selected content could not be deleted."));
    }
    return removed;
}

void CanvasWidget::selectAll()
{
    const Document &document = m_controller->document();
    if (!document.layer(document.activeLayerId))
    {
        emit interactionMessage(tr("Add a layer before using this tool."));
        return;
    }
    cancelSelectionTransformForBoundary(
        tr("The pending selection transform was canceled before selecting."));
    cancelAreaSelection();
    setSelectionMoveMode(false);
    QImage mask(document.size, QImage::Format_Grayscale8);
    mask.fill(255);
    pushSelectionChange(currentSelectionState(),
        selectionStateForMask(std::move(mask)),
        tr("Select all"));
}

void CanvasWidget::invertSelection()
{
    if (m_selectionMask.isNull())
    {
        return;
    }
    cancelSelectionTransformForBoundary(
        tr("The pending transform was canceled before inverting."));
    setSelectionMoveMode(false);
    QImage mask = m_selectionMask;
    mask.invertPixels();
    pushSelectionChange(currentSelectionState(),
        selectionStateForMask(std::move(mask)),
        tr("Invert selection"));
}

void CanvasWidget::deselectSelection()
{
    if (m_selectionMask.isNull())
    {
        setSelectionMoveMode(false);
        return;
    }
    cancelSelectionTransformForBoundary(
        tr("The pending transform was canceled before deselecting."));
    setSelectionMoveMode(false);
    pushSelectionChange(currentSelectionState(), {}, tr("Deselect"));
}

void CanvasWidget::setSelectionActionBar(SelectionActionBar *actionBar)
{
    if (m_selectionActionBar == actionBar)
    {
        updateSelectionActionBar();
        return;
    }
    if (m_selectionActionBar)
    {
        m_selectionActionBar->hide();
    }
    m_selectionActionBar = actionBar;
    if (m_selectionActionBar)
    {
        m_selectionActionBar->setParent(this);
        m_selectionActionBar->hide();
    }
    updateSelectionActionBar();
}

void CanvasWidget::releaseTransientRenderCaches()
{
    invalidateFrames();
}

void CanvasWidget::requestDisplayUpdate()
{
    if (usingGpuDisplay())
    {
        m_frameView->update();
        m_overlayView->update();
        return;
    }
    update();
}

void CanvasWidget::requestDisplayUpdate(const QRect &rect)
{
    if (usingGpuDisplay())
    {
        m_frameView->update();
        m_overlayView->update(rect);
        return;
    }
    update(rect);
}

bool CanvasWidget::usingGpuDisplay() const
{
    return m_frameView != nullptr;
}

void CanvasWidget::initializeDisplayViews()
{
    const QByteArray mode = qgetenv("UGURUGU_CANVAS_DISPLAY");
    if (mode == "software")
    {
        spdlog::info("Canvas display: software (forced by environment)");
        return;
    }
    // Headless platforms have no window-integrated RHI, so QRhiWidget cannot
    // composite there; the QPainter path in paintEvent covers them.
    const QString platform = QGuiApplication::platformName();
    if (mode != "gpu"
        && (platform == QStringLiteral("offscreen")
            || platform == QStringLiteral("minimal")))
    {
        return;
    }
    m_frameView = new CanvasFrameView(this);
    m_overlayView = new CanvasOverlayView(this);
    connect(
        m_frameView,
        &QRhiWidget::renderFailed,
        this,
        [this]()
        {
            discardDisplayViews();
        },
        Qt::QueuedConnection);
    syncDisplayViewGeometry();
    m_frameView->show();
    m_overlayView->show();
}

void CanvasWidget::discardDisplayViews()
{
    if (!m_frameView && !m_overlayView)
    {
        return;
    }
    spdlog::warn("Canvas display: GPU initialization failed, "
                 "falling back to software rendering");
    if (m_frameView)
    {
        m_frameView->hide();
        m_frameView->deleteLater();
        m_frameView = nullptr;
    }
    if (m_overlayView)
    {
        m_overlayView->hide();
        m_overlayView->deleteLater();
        m_overlayView = nullptr;
    }
    update();
}

void CanvasWidget::syncDisplayViewGeometry()
{
    if (m_frameView)
    {
        m_frameView->setGeometry(rect());
    }
    if (m_overlayView)
    {
        m_overlayView->setGeometry(rect());
    }
}

void CanvasWidget::setTool(Tool tool)
{
    if (m_tool == tool)
    {
        return;
    }
    cancelSelectionTransformForBoundary(tr(
        "The pending selection transform was canceled when changing tools."));
    cancelStroke();
    cancelTextPlacement();
    endColorPick();
    setSelectionMoveMode(false);
    if (m_areaSelectionActive)
    {
        const SelectionState previousSelection =
            m_hasSelectionBeforeArea ? m_selectionBeforeArea : SelectionState();
        cancelAreaSelection();
        restoreSelectionState(previousSelection);
    }
    m_tool = tool;
    emit toolChanged(tool);
    updateCursor();
    requestDisplayUpdate();
}

void CanvasWidget::setSelectionMoveMode(bool enabled)
{
    const bool next = enabled && hasTransformableSelection();
    if (m_selectionMoveMode == next)
    {
        updateSelectionActionBar();
        return;
    }
    if (!next)
    {
        cancelSelectionMove();
    }
    m_selectionMoveMode = next;
    emit selectionMoveModeChanged(next);
    updateCursor();
    updateSelectionActionBar();
    requestDisplayUpdate();
}

void CanvasWidget::handleEscape()
{
    if (m_movingSelection)
    {
        cancelSelectionMove();
    }
    else if (hasSelectionTransformSession())
    {
        cancelSelectionTransform();
    }
    else if (m_areaSelectionActive)
    {
        const SelectionState previousSelection =
            m_hasSelectionBeforeArea ? m_selectionBeforeArea : SelectionState();
        cancelAreaSelection();
        restoreSelectionState(previousSelection);
    }
    else if (m_selectionMoveMode)
    {
        setSelectionMoveMode(false);
    }
    else if (m_textPlacementActive)
    {
        cancelTextPlacement();
    }
    else if (m_drawing || m_panning || m_zoomDragging || m_rotatingCanvas
             || m_touchSequence || m_pickingColor)
    {
        cancelStroke();
        endPan();
        endZoomDrag();
        endCanvasRotation();
        cancelTouchSequence();
        endColorPick();
        m_tabletSequence = false;
    }
    else
    {
        deselectSelection();
    }
}

void CanvasWidget::setBrushColor(const QColor &color)
{
    if (!color.isValid() || m_brushColor == color)
    {
        return;
    }
    m_brushColor = color;
    emit brushColorChanged(color);
    requestDisplayUpdate();
}

void CanvasWidget::setBrushWidth(qreal width)
{
    if (!std::isfinite(width))
    {
        return;
    }
    const qreal normalized = std::clamp(width,
        DocumentLimits::minimumStrokeWidth,
        DocumentLimits::maximumStrokeWidth);
    if (qFuzzyCompare(m_brushWidth, normalized))
    {
        return;
    }
    m_brushWidth = normalized;
    if (!m_brushPresetId.isEmpty())
    {
        m_brushPresetWidths.insert(m_brushPresetId, normalized);
    }
    emit brushWidthChanged(normalized);
    requestDisplayUpdate();
}

void CanvasWidget::setBrushPresetWidth(const QString &presetId, qreal width)
{
    const BrushPreset *preset = BrushPresetCatalog::find(presetId);
    if (!preset || !std::isfinite(width))
    {
        return;
    }
    const qreal normalized = std::clamp(width,
        DocumentLimits::minimumStrokeWidth,
        DocumentLimits::maximumStrokeWidth);
    if (m_brushPresetId == preset->id)
    {
        setBrushWidth(normalized);
        return;
    }
    m_brushPresetWidths.insert(preset->id, normalized);
}

void CanvasWidget::setEraserWidth(qreal width)
{
    if (!std::isfinite(width))
    {
        return;
    }
    const qreal normalized = std::clamp(width,
        DocumentLimits::minimumStrokeWidth,
        DocumentLimits::maximumStrokeWidth);
    if (qFuzzyCompare(m_eraserWidth, normalized))
    {
        return;
    }
    m_eraserWidth = normalized;
    if (!m_eraserPresetId.isEmpty())
    {
        m_eraserPresetWidths.insert(m_eraserPresetId, normalized);
    }
    emit eraserWidthChanged(normalized);
    requestDisplayUpdate();
}

void CanvasWidget::setEraserPresetWidth(const QString &presetId, qreal width)
{
    const EraserPreset *preset = EraserPresetCatalog::find(presetId);
    if (!preset || !std::isfinite(width))
    {
        return;
    }
    const qreal normalized = std::clamp(width,
        DocumentLimits::minimumStrokeWidth,
        DocumentLimits::maximumStrokeWidth);
    if (m_eraserPresetId == preset->id)
    {
        setEraserWidth(normalized);
        return;
    }
    m_eraserPresetWidths.insert(preset->id, normalized);
}

void CanvasWidget::setBrushStabilization(qreal strength)
{
    setBrushPresetStabilization(m_brushPresetId, strength);
}

void CanvasWidget::setBrushPresetStabilization(
    const QString &presetId, qreal strength)
{
    const BrushPreset *preset = BrushPresetCatalog::find(presetId);
    if (!preset || !std::isfinite(strength))
    {
        return;
    }
    const qreal normalized = std::clamp(strength, 0.0, 1.0);
    if (qFuzzyCompare(
            1.0 + brushPresetStabilization(preset->id), 1.0 + normalized))
    {
        return;
    }
    m_brushPresetStabilizations.insert(preset->id, normalized);
    if (m_brushPresetId == preset->id)
    {
        emit brushStabilizationChanged(normalized);
    }
}

void CanvasWidget::setEraserStabilization(qreal strength)
{
    setEraserPresetStabilization(m_eraserPresetId, strength);
}

void CanvasWidget::setEraserPresetStabilization(
    const QString &presetId, qreal strength)
{
    const EraserPreset *preset = EraserPresetCatalog::find(presetId);
    if (!preset || !std::isfinite(strength))
    {
        return;
    }
    const qreal normalized = std::clamp(strength, 0.0, 1.0);
    if (qFuzzyCompare(
            1.0 + eraserPresetStabilization(preset->id), 1.0 + normalized))
    {
        return;
    }
    m_eraserPresetStabilizations.insert(preset->id, normalized);
    if (m_eraserPresetId == preset->id)
    {
        emit eraserStabilizationChanged(normalized);
    }
}

void CanvasWidget::setBrushAntialiasing(bool antialiasing)
{
    if (m_brushAntialiasing == antialiasing)
    {
        return;
    }
    m_brushAntialiasing = antialiasing;
    emit brushAntialiasingChanged(antialiasing);
    requestDisplayUpdate();
}

void CanvasWidget::setTabletPressureEnabled(bool enabled)
{
    if (m_tabletPressureEnabled == enabled)
    {
        return;
    }
    m_tabletPressureEnabled = enabled;
    emit tabletPressureEnabledChanged(enabled);
}

void CanvasWidget::setWobbleAnimationEnabled(bool enabled)
{
    if (m_wobbleAnimationEnabled == enabled)
    {
        return;
    }
    m_wobbleAnimationEnabled = enabled;
    if (!enabled)
    {
        setAnimating(false);
    }
    invalidateFrames();
}

void CanvasWidget::setBrushPreset(const QString &presetId)
{
    const BrushPreset *preset = BrushPresetCatalog::find(presetId);
    if (!preset || m_brushPresetId == preset->id)
    {
        return;
    }
    cancelStroke();
    m_brushPresetId = preset->id;
    m_brushSettings = preset->settings;
    const qreal nextWidth =
        std::clamp(m_brushPresetWidths.value(preset->id, preset->defaultSize),
            DocumentLimits::minimumStrokeWidth,
            DocumentLimits::maximumStrokeWidth);
    m_brushPresetWidths.insert(preset->id, nextWidth);
    if (!qFuzzyCompare(m_brushWidth, nextWidth))
    {
        m_brushWidth = nextWidth;
        emit brushWidthChanged(nextWidth);
    }
    emit brushPresetChanged(preset->id);
    emit brushStabilizationChanged(brushStabilization());
    requestDisplayUpdate();
}

void CanvasWidget::setEraserPreset(const QString &presetId)
{
    const EraserPreset *preset = EraserPresetCatalog::find(presetId);
    if (!preset || m_eraserPresetId == preset->id)
    {
        return;
    }
    cancelStroke();
    m_eraserPresetId = preset->id;
    m_eraserSettings = preset->settings;
    const qreal nextWidth =
        std::clamp(m_eraserPresetWidths.value(preset->id, preset->defaultSize),
            DocumentLimits::minimumStrokeWidth,
            DocumentLimits::maximumStrokeWidth);
    m_eraserPresetWidths.insert(preset->id, nextWidth);
    if (!qFuzzyCompare(m_eraserWidth, nextWidth))
    {
        m_eraserWidth = nextWidth;
        emit eraserWidthChanged(nextWidth);
    }
    emit eraserPresetChanged(preset->id);
    emit eraserStabilizationChanged(eraserStabilization());
    requestDisplayUpdate();
}

void CanvasWidget::setWandReference(WandReference reference)
{
    switch (reference)
    {
    case WandReference::ActiveLayer:
    case WandReference::ReferenceLayers:
    case WandReference::AllVisibleLayers:
        break;
    default:
        return;
    }
    if (m_wandReference == reference)
    {
        return;
    }
    m_wandReference = reference;
    emit wandReferenceChanged(reference);
}

void CanvasWidget::setFillComparison(FillComparison comparison)
{
    if (comparison != FillComparison::AlphaBoundary
        && comparison != FillComparison::Color)
    {
        return;
    }
    if (m_fillComparison == comparison)
    {
        return;
    }
    m_fillComparison = comparison;
    emit fillComparisonChanged(comparison);
}

void CanvasWidget::setFillTolerance(int tolerance)
{
    const int normalized = std::clamp(tolerance, 0, 255);
    if (m_fillTolerance == normalized)
    {
        return;
    }
    m_fillTolerance = normalized;
    emit fillToleranceChanged(normalized);
}

void CanvasWidget::setBucketAntialiasing(bool antialiasing)
{
    if (m_bucketAntialiasing == antialiasing)
    {
        return;
    }
    m_bucketAntialiasing = antialiasing;
    emit bucketAntialiasingChanged(antialiasing);
}

void CanvasWidget::setSelectionShape(SelectionShape shape)
{
    switch (shape)
    {
    case SelectionShape::Freehand:
    case SelectionShape::Rectangle:
    case SelectionShape::Ellipse:
        break;
    default:
        return;
    }
    if (m_selectionShape == shape)
    {
        return;
    }
    if (m_areaSelectionActive)
    {
        const SelectionState previousSelection =
            m_hasSelectionBeforeArea ? m_selectionBeforeArea : SelectionState();
        cancelAreaSelection();
        restoreSelectionState(previousSelection);
    }
    m_selectionShape = shape;
    emit selectionShapeChanged(shape);
    requestDisplayUpdate();
}

void CanvasWidget::setLassoMode(LassoMode mode)
{
    if (mode != LassoMode::Select && mode != LassoMode::Paint)
    {
        return;
    }
    if (m_lassoMode == mode)
    {
        return;
    }
    if (m_areaSelectionActive)
    {
        const SelectionState previousSelection =
            m_hasSelectionBeforeArea ? m_selectionBeforeArea : SelectionState();
        cancelAreaSelection();
        restoreSelectionState(previousSelection);
    }
    m_lassoMode = mode;
    emit lassoModeChanged(mode);
}

void CanvasWidget::setSelectionTransformSampling(SamplingMode sampling)
{
    if ((sampling != SamplingMode::Smooth && sampling != SamplingMode::Nearest)
        || m_selectionTransformSampling == sampling)
    {
        return;
    }
    m_selectionTransformSampling = sampling;
    if (m_selectionTransformSession.active)
    {
        m_selectionTransformSession.previewOperation.sampling =
            selectionSamplingForTransform(
                m_selectionTransformSession.transform);
        emit selectionTransformSessionChanged(
            true, hasPendingSelectionTransform());
        requestDisplayUpdate();
    }
    emit selectionTransformSamplingChanged(sampling);
}

void CanvasWidget::setAnimating(bool animating)
{
    if (animating && !m_wobbleAnimationEnabled)
    {
        return;
    }
    if (m_animating == animating)
    {
        return;
    }
    const QSize previousPreviewSize = previewRenderSize();
    m_animating = animating;
    const bool previewSizeChanged = previewRenderSize() != previousPreviewSize;
    if (previewSizeChanged)
    {
        invalidateFrames();
    }
    if (m_animating)
    {
        updateTimerInterval();
        m_animationTimer.start();
    }
    else
    {
        m_animationTimer.stop();
        cancelFrameCacheWarmup();
        if (hasSelection())
        {
            // Playback left the selection's per-frame editability unresolved;
            // the frame it stopped on is the one the user acts on.
            notifySelectionTransformAvailability();
        }
    }
    if (m_animating && !previewSizeChanged)
    {
        scheduleFrameCacheWarmup();
    }
    if (m_drawing && m_animateWhileDrawing)
    {
        if (m_animating)
        {
            const QSize renderSize = previewRenderSize();
            if (hasInteractionFrame(
                    m_currentFrame, renderSize, m_activeStrokeLayer))
            {
                const int frameCount =
                    std::max(1, m_controller->document().animationFrames);
                requestInteractionFrameWarmup(
                    (m_currentFrame + 1) % frameCount);
            }
            else
            {
                requestInteractionFrameWarmup(m_currentFrame);
            }
        }
        else
        {
            cancelInteractionFrameWarmup();
        }
    }
    else if (!m_drawing && !m_animating)
    {
        requestInteractionFrameWarmup(m_currentFrame);
    }
    else if (!m_drawing)
    {
        cancelInteractionFrameWarmup();
    }
    emit animatingChanged(animating);
    requestDisplayUpdate();
}

void CanvasWidget::toggleAnimating()
{
    setAnimating(!m_animating);
}

void CanvasWidget::setAnimateWhileDrawing(bool animate)
{
    if (m_animateWhileDrawing == animate)
    {
        return;
    }
    m_animateWhileDrawing = animate;
    if (!m_drawing || !m_animating)
    {
        return;
    }
    if (!animate)
    {
        cancelInteractionFrameWarmup();
        return;
    }
    cancelFrameCacheWarmup();
    const QSize renderSize = previewRenderSize();
    if (hasInteractionFrame(m_currentFrame, renderSize, m_activeStrokeLayer))
    {
        const int frameCount =
            std::max(1, m_controller->document().animationFrames);
        requestInteractionFrameWarmup((m_currentFrame + 1) % frameCount);
    }
    else
    {
        requestInteractionFrameWarmup(m_currentFrame);
    }
}

void CanvasWidget::setGroupSelectionActive(bool active)
{
    if (m_groupSelectionActive == active)
    {
        return;
    }
    m_groupSelectionActive = active;
    updateCursor();
    requestDisplayUpdate();
}

void CanvasWidget::fitToWindow()
{
    cancelTouchSequence();
    m_zoom = fitZoom();
    m_pan = QPointF();
    notifyZoomChanged();
    requestDisplayUpdate();
}

void CanvasWidget::resetZoom()
{
    zoomToward(1.0, zoomAnchorPosition());
}

void CanvasWidget::setZoomPercent(int percent)
{
    zoomToward(std::clamp(percent / 100.0, minimumZoom, maximumZoom),
        zoomAnchorPosition());
}

void CanvasWidget::zoomIn()
{
    zoomToward(m_zoom * keyboardZoomStep, zoomAnchorPosition());
}

void CanvasWidget::zoomOut()
{
    zoomToward(m_zoom / keyboardZoomStep, zoomAnchorPosition());
}

void CanvasWidget::setCanvasRotation(qreal degrees)
{
    if (!std::isfinite(degrees))
    {
        return;
    }
    const qreal normalized = normalizedRotation(degrees);
    if (qFuzzyIsNull(m_canvasRotation - normalized))
    {
        return;
    }
    cancelTouchSequence();
    cancelStroke();
    endPan();
    endZoomDrag();
    endCanvasRotation();
    endColorPick();
    applyCanvasRotation(normalized);
}

void CanvasWidget::rotateCanvasLeft()
{
    setCanvasRotation(m_canvasRotation - canvasRotationStep);
}

void CanvasWidget::rotateCanvasRight()
{
    setCanvasRotation(m_canvasRotation + canvasRotationStep);
}

void CanvasWidget::resetCanvasRotation()
{
    setCanvasRotation(0.0);
}

void CanvasWidget::setCanvasMirrored(bool mirrored)
{
    if (m_canvasMirrored == mirrored)
    {
        return;
    }
    cancelTouchSequence();
    cancelStroke();
    endPan();
    endZoomDrag();
    endCanvasRotation();
    endColorPick();
    m_canvasMirrored = mirrored;
    updatePointerPosition(m_pointerWidgetPosition);
    emit canvasMirroredChanged(mirrored);
    requestDisplayUpdate();
}

void CanvasWidget::toggleCanvasMirrored()
{
    setCanvasMirrored(!m_canvasMirrored);
}

void CanvasWidget::setCurrentFrame(int frame)
{
    const int frameCount =
        std::max(1, m_controller->document().animationFrames);
    const int normalized = ((frame % frameCount) + frameCount) % frameCount;
    if (normalized == m_currentFrame)
    {
        return;
    }
    m_currentFrame = normalized;
    invalidateActiveStrokePreview();
    if (usesPreparedInteractionFrames())
    {
        const QSize renderSize = previewRenderSize();
        adoptPreparedInteractionFrame(
            normalized, renderSize, m_activeStrokeLayer);
        if (!hasInteractionFrame(normalized, renderSize, m_activeStrokeLayer))
        {
            requestInteractionFrameWarmup(normalized);
        }
        else if (m_previewSplitFrame == normalized
                 || m_previewLayerRasterFrame == normalized)
        {
            requestInteractionFrameWarmup((normalized + 1) % frameCount);
        }
    }
    else if (!m_animating)
    {
        requestInteractionFrameWarmup(normalized);
    }
    emit currentFrameChanged(normalized);
    // Which strokes a selection can edit is resolved against the displayed
    // frame, so scrubbing changes the answer. Playback is excluded: it would
    // repeat that per-frame search on every tick, and setAnimating settles the
    // state once the animation stops.
    if (!m_animating && hasSelection())
    {
        notifySelectionTransformAvailability();
    }
    requestDisplayUpdate();
}

void CanvasWidget::setPanModifierActive(bool active)
{
    if (m_spacePressed == active)
    {
        return;
    }
    m_spacePressed = active;
    updateCursor();
    requestDisplayUpdate();
}

void CanvasWidget::cancelActiveInteraction()
{
    const bool restoreAreaSelection =
        m_areaSelectionActive && m_hasSelectionBeforeArea;
    const SelectionState selectionBeforeArea =
        restoreAreaSelection ? m_selectionBeforeArea : SelectionState();

    cancelStroke();
    cancelSelectionMove();
    cancelAreaSelection();
    endTextDrag();
    if (restoreAreaSelection)
    {
        restoreSelectionState(selectionBeforeArea);
    }
    endPan();
    endZoomDrag();
    endCanvasRotation();
    cancelTouchSequence();
    endColorPick();
    m_tabletSequence = false;
    m_tabletPointerEraser = false;
    m_shiftPressed = false;
    setPanModifierActive(false);
    updateCursor();
    requestDisplayUpdate();
}

}

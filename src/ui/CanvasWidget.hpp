// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#pragma once

#include "document/DocumentController.hpp"
#include "input/StrokeStabilizer.hpp"
#include "io/SelectionClipboardCodec.hpp"
#include "render/IncrementalStrokeRenderer.hpp"
#include "render/PreviewMemoryUsage.hpp"
#include "render/RenderEngine.hpp"
#include "ui/CanvasTypes.hpp"

#include <QCache>
#include <QColor>
#include <QFont>
#include <QFutureWatcher>
#include <QHash>
#include <QImage>
#include <QPainterPath>
#include <QPointer>
#include <QRegion>
#include <QSet>
#include <QThreadPool>
#include <QTimer>
#include <QWidget>

#include <atomic>
#include <memory>

class QMouseEvent;
class QPointingDevice;
class QTabletEvent;
class QTouchEvent;
class QWheelEvent;

namespace ugurugu
{

class SelectionActionBar;
class CanvasWidgetTestAccess;
class CanvasFrameView;
class CanvasOverlayView;

class CanvasWidget final : public QWidget
{
    Q_OBJECT

public:
    using Tool = CanvasTool;
    using WandReference = CanvasWandReference;
    using FillComparison = CanvasFillComparison;
    using SelectionShape = CanvasSelectionShape;
    using LassoMode = CanvasLassoMode;
    using SelectionCombine = CanvasSelectionCombine;

    explicit CanvasWidget(
        DocumentController *controller, QWidget *parent = nullptr);
    ~CanvasWidget() override;

    Tool tool() const;
    QColor brushColor() const;
    qreal brushWidth() const;
    qreal brushPresetWidth(const QString &presetId) const;
    qreal eraserWidth() const;
    qreal eraserPresetWidth(const QString &presetId) const;
    qreal brushStabilization() const;
    qreal brushPresetStabilization(const QString &presetId) const;
    qreal eraserStabilization() const;
    qreal eraserPresetStabilization(const QString &presetId) const;
    bool brushAntialiasing() const;
    bool tabletPressureEnabled() const;
    QString brushPresetId() const;
    QString eraserPresetId() const;
    WandReference wandReference() const;
    FillComparison fillComparison() const;
    int fillTolerance() const;
    bool bucketAntialiasing() const;
    SelectionShape selectionShape() const;
    LassoMode lassoMode() const;
    SamplingMode selectionTransformSampling() const;
    QString textContent() const;
    QString textFontFamily() const;
    qreal textFontSize() const;
    bool textFilled() const;
    QFont textFont() const;
    bool hasTextPlacement() const;
    bool applyTextPlacement();
    void cancelTextPlacement();
    bool isAnimating() const;
    bool isWobbleAnimationEnabled() const;
    int currentFrame() const;
    qreal zoom() const;
    qreal canvasRotation() const;
    bool isCanvasMirrored() const;
    bool hasSelection() const;
    bool hasTransformableSelection() const;
    bool hasEditableStrokeSelection() const;
    QUuid selectionLayerId() const;
    QVector<QUuid> selectedStrokeIds() const;
    bool selectionMoveMode() const;
    bool hasSelectionTransformSession() const;
    bool hasPendingSelectionTransform() const;
    QTransform pendingSelectionTransform() const;
    // Persistence snapshot: appends the pending PixelSelectionOp to a
    // document copy without touching the live document or history, keeping
    // the true wobble values autosave must record.
    Document documentWithPendingSelectionTransform() const;
    // Display variant of the pending-transform snapshot: additionally applies
    // the wobble-disabled normalization the on-screen preview uses, layer
    // overrides included. What the eyedropper and still-image export sample.
    Document displayDocumentWithPendingSelectionTransform() const;
    bool scaleSelection(qreal factor);
    bool rotateSelection(qreal degrees);
    bool flipSelectionHorizontally();
    bool flipSelectionVertically();
    bool applySelectionTransform();
    void cancelSelectionTransform();
    bool copySelection();
    bool cutSelection();
    bool deleteSelection();
    bool fillSelection();
    void selectAll();
    void invertSelection();
    void deselectSelection();
    void setSelectionActionBar(SelectionActionBar *actionBar);
    void releaseTransientRenderCaches();

public slots:
    void setTool(Tool tool);
    void setBrushColor(const QColor &color);
    void setBrushWidth(qreal width);
    void setBrushPresetWidth(const QString &presetId, qreal width);
    void setEraserWidth(qreal width);
    void setEraserPresetWidth(const QString &presetId, qreal width);
    void setBrushStabilization(qreal strength);
    void setBrushPresetStabilization(const QString &presetId, qreal strength);
    void setEraserStabilization(qreal strength);
    void setEraserPresetStabilization(const QString &presetId, qreal strength);
    void setBrushAntialiasing(bool antialiasing);
    void setTabletPressureEnabled(bool enabled);
    void setWobbleAnimationEnabled(bool enabled);
    void setBrushPreset(const QString &presetId);
    void setEraserPreset(const QString &presetId);
    void setWandReference(WandReference reference);
    void setFillComparison(FillComparison comparison);
    void setFillTolerance(int tolerance);
    void setBucketAntialiasing(bool antialiasing);
    void setSelectionShape(SelectionShape shape);
    void setLassoMode(LassoMode mode);
    void setSelectionTransformSampling(SamplingMode sampling);
    void setTextContent(const QString &text);
    void setTextFontFamily(const QString &family);
    void setTextFontSize(qreal size);
    void setTextFilled(bool filled);
    void setAnimating(bool animating);
    void toggleAnimating();
    void setAnimateWhileDrawing(bool animate);
    void setGroupSelectionActive(bool active);
    void fitToWindow();
    void resetZoom();
    void setZoomPercent(int percent);
    void zoomIn();
    void zoomOut();
    void setCanvasRotation(qreal degrees);
    void rotateCanvasLeft();
    void rotateCanvasRight();
    void resetCanvasRotation();
    void setCanvasMirrored(bool mirrored);
    void toggleCanvasMirrored();
    void setCurrentFrame(int frame);
    void setPanModifierActive(bool active);
    void setSelectionMoveMode(bool enabled);
    void handleEscape();
    void cancelActiveInteraction();

signals:
    void toolChanged(Tool tool);
    void brushColorChanged(const QColor &color);
    void colorUsed(const QColor &color);
    void brushWidthChanged(qreal width);
    void eraserWidthChanged(qreal width);
    void brushStabilizationChanged(qreal strength);
    void eraserStabilizationChanged(qreal strength);
    void brushAntialiasingChanged(bool antialiasing);
    void tabletPressureEnabledChanged(bool enabled);
    void brushPresetChanged(const QString &presetId);
    void eraserPresetChanged(const QString &presetId);
    void wandReferenceChanged(WandReference reference);
    void fillComparisonChanged(FillComparison comparison);
    void fillToleranceChanged(int tolerance);
    void bucketAntialiasingChanged(bool antialiasing);
    void selectionShapeChanged(SelectionShape shape);
    void lassoModeChanged(LassoMode mode);
    void selectionTransformSamplingChanged(SamplingMode sampling);
    void textContentChanged(const QString &text);
    void textFontFamilyChanged(const QString &family);
    void textFontSizeChanged(qreal size);
    void textFilledChanged(bool filled);
    void textPlacementChanged(bool active);
    void animatingChanged(bool animating);
    void currentFrameChanged(int frame);
    void zoomChanged(int percent);
    void canvasRotationChanged(qreal degrees);
    void canvasMirroredChanged(bool mirrored);
    void pointerPositionChanged(const QPointF &position, bool inside);
    void interactionMessage(const QString &message);
    void selectionTransformAvailabilityChanged(bool available);
    void selectionAvailabilityChanged(bool hasArea, bool hasContent);
    void selectionMoveModeChanged(bool enabled);
    void selectionTransformSessionChanged(bool active, bool dirty);

protected:
    bool event(QEvent *event) override;
    void paintEvent(QPaintEvent *event) override;
    void resizeEvent(QResizeEvent *event) override;
    void enterEvent(QEnterEvent *event) override;
    void mousePressEvent(QMouseEvent *event) override;
    void mouseMoveEvent(QMouseEvent *event) override;
    void mouseReleaseEvent(QMouseEvent *event) override;
    void mouseDoubleClickEvent(QMouseEvent *event) override;
    void wheelEvent(QWheelEvent *event) override;
    void tabletEvent(QTabletEvent *event) override;
    void keyPressEvent(QKeyEvent *event) override;
    void keyReleaseEvent(QKeyEvent *event) override;
    void leaveEvent(QEvent *event) override;

private:
    friend class CanvasWidgetTestAccess;
    friend class CanvasFrameView;
    friend class CanvasOverlayView;

    struct SelectionState
    {
        QSet<QUuid> strokes;
        QUuid layer;
        QImage mask;
    };

    // Armed by commitStroke around DocumentController::addStroke so the
    // documentChanged handler can tell a plain stroke append apart from every
    // other document change and refresh cached frames regionally instead of
    // discarding them.
    struct PendingStrokeRefreshHint
    {
        bool armed = false;
        QUuid layerId;
        QUuid strokeId;
    };

    struct FloatingTransformSession
    {
        bool active = false;
        QUuid layer;
        QVector<QUuid> strokeIds;
        QImage sourceMask;
        QPainterPath sourceOutline;
        QRectF sourceBounds;
        PixelSelectionOp previewOperation;
        QTransform transform;
    };

    // The frame pixels the viewport should show right now, together with the
    // texels that changed since the previous resolve. dirtyBounds is in output
    // (render-size) pixels: the full image when the backing store was
    // replaced, empty when nothing changed. Lets a texture-backed display
    // upload only what moved.
    struct DisplayedFrame
    {
        QImage image;
        QRect dirtyBounds;
    };

    struct PreparedInteractionFrame
    {
        int frame = -1;
        QSize renderSize;
        QUuid layerId;
        RenderEngine::LayerSplitFrame split;
        RenderEngine::LayerRasterFrame rasters;
        QImage baseFrame;

        bool isValid() const;
        bool matches(int candidateFrame,
            const QSize &candidateSize,
            const QUuid &candidateLayerId) const;
    };

    QTransform documentTransform() const;
    qreal fitZoom() const;
    Document displayDocument() const;
    void applyWobbleAnimationSetting(Document &document) const;
    DisplayedFrame resolveDisplayedFrame();
    void paintOverlay(QPainter &painter, const QRegion &exposedRegion);
    void refreshCanvasShadow(const QPolygonF &canvasPolygon);
    void requestDisplayUpdate();
    void requestDisplayUpdate(const QRect &rect);
    bool usingGpuDisplay() const;
    void initializeDisplayViews();
    void discardDisplayViews();
    void syncDisplayViewGeometry();
    QPointF mapToDocument(
        const QPointF &widgetPosition, bool *inside = nullptr) const;
    QPointF clampedDocumentPosition(const QPointF &position) const;
    QSize previewRenderSize() const;
    PreviewSurfaceUsage previewSurfaceUsage() const;
    void updateFrameCacheBudget();
    QImage frameImage(int frame);
    void resetFrameCacheStorage();
    bool tryRegionalStrokeInvalidation(
        const QUuid &layerId, const QUuid &strokeId);
    void clearCompletedFrameCacheRefresh();
    QImage activeStrokePreview(
        const Document &document, const QSize &renderSize, bool &resolved);
    void invalidateActiveStrokePreview();
    QImage interactionPreview(Document document, const QSize &renderSize) const;
    const RenderEngine::LayerSplitFrame &previewSplit(
        const QUuid &layerId, const QSize &renderSize);
    const RenderEngine::LayerRasterFrame &previewLayerRasters(
        const QSize &renderSize);
    bool usesPreparedInteractionFrames() const;
    bool hasInteractionFrame(
        int frame, const QSize &renderSize, const QUuid &layerId) const;
    bool adoptPreparedInteractionFrame(
        int frame, const QSize &renderSize, const QUuid &layerId);
    void requestInteractionFrameWarmup(int frame);
    void startInteractionFrameWarmup();
    void finishInteractionFrameWarmup();
    void cancelInteractionFrameWarmup();
    void clearPreparedInteractionFrame();
    void invalidateFrames();
    void cancelFrameCacheWarmup();
    void scheduleFrameCacheWarmup();
    void renderNextFrameCacheWarmup();
    void updateTimerInterval();
    void advanceFrame();
    void beginStroke(const QPointF &widgetPosition,
        qreal pressure,
        bool tabletEraser,
        quint64 timestamp);
    void continueStroke(
        const QPointF &widgetPosition, qreal pressure, quint64 timestamp);
    void endStroke(const QPointF &widgetPosition, quint64 timestamp);
    DocumentController::AddStrokeResult commitStroke(
        const QUuid &layerId, Stroke stroke);
    void cancelStroke();
    void beginPan(const QPointF &widgetPosition);
    void continuePan(const QPointF &widgetPosition);
    void endPan();
    QPointF zoomAnchorPosition() const;
    void zoomToward(qreal targetZoom, const QPointF &widgetPosition);
    void beginZoomDrag(const QPointF &widgetPosition);
    void continueZoomDrag(const QPointF &widgetPosition);
    void endZoomDrag();
    void applyCanvasRotation(qreal degrees);
    void rotateCanvasAround(qreal degrees, const QPointF &widgetPosition);
    void beginCanvasRotation(const QPointF &widgetPosition);
    void continueCanvasRotation(const QPointF &widgetPosition);
    void endCanvasRotation();
    bool handleTouchEvent(QTouchEvent *event);
    bool touchGestureConflictsWithActiveInteraction() const;
    void beginTouchGesture(int firstPointId,
        const QPointF &firstPosition,
        int secondPointId,
        const QPointF &secondPosition);
    void continueTouchGesture(
        const QPointF &firstPosition, const QPointF &secondPosition);
    void endTouchGesture();
    void suppressTouchSequence();
    void cancelTouchSequence();
    bool isColorPickableTool() const;
    void beginColorPick(const QPointF &widgetPosition);
    void endColorPick();
    void pickColorAt(const QPointF &widgetPosition);
    void updatePointerPosition(const QPointF &widgetPosition);
    void updateCursor();
    void notifyZoomChanged();
    QRect pointerUpdateRect() const;
    bool copySelectionToClipboard(SelectionClipboardCodec::Copy *outCopy);
    bool reportLayerAcceptsPaint(const Layer &layer);
    bool selectionContains(const QPointF &documentPosition) const;
    void beginAreaSelection(const QPointF &documentPosition,
        SelectionCombine combine = SelectionCombine::Replace);
    void continueAreaSelection(const QPointF &documentPosition);
    void finishAreaSelection();
    void cancelAreaSelection();
    bool canFinishAreaSelection() const;
    QPainterPath areaSelectionPath() const;
    void applySelectionMask(
        QImage mask, const SelectionState &previousSelection);
    SelectionState selectionStateForMask(QImage mask) const;
    SelectionState currentSelectionState() const;
    void restoreSelectionState(const SelectionState &state);
    void evaluateSelectionVisibility();
    void cancelSelectionVisibilityEvaluation();
    void applySelectionVisibility(bool hasVisiblePixels);
    void pushSelectionChange(const SelectionState &previousSelection,
        const SelectionState &nextSelection,
        const QString &text);
    void beginTextInteraction(const QPointF &documentPosition);
    void continueTextDrag(const QPointF &documentPosition);
    void endTextDrag();
    QPainterPath textPreviewPath() const;
    QRectF textPlacementBounds() const;
    void drawTextPlacementOverlay(
        QPainter &painter, const QTransform &transform);
    void computeWandSelection(const QPointF &documentPosition,
        SelectionCombine combine = SelectionCombine::Replace);
    void applyBucketFill(const QPointF &documentPosition);
    void commitFrozenFill(const QImage &coverage);
    void beginSelectionMove(const QPointF &documentPosition);
    void continueSelectionMove(const QPointF &documentPosition);
    void commitSelectionMove();
    void cancelSelectionMove();
    bool beginSelectionTransformSession();
    bool setPendingSelectionTransform(const QTransform &transform);
    bool isValidSelectionTransform(const QTransform &transform) const;
    SamplingMode selectionSamplingForTransform(
        const QTransform &transform) const;
    void resetSelectionTransformSession();
    void cancelSelectionTransformForBoundary(const QString &message = {});
    QPainterPath displayedSelectionOutline() const;
    QRectF displayedSelectionBounds() const;
    QPointF safeSelectionDeltaForBounds(
        const QPointF &delta, const QRectF &bounds) const;
    void clearSelection();
    void pruneSelection();
    void transformSelectionOverlay(const QUuid &layerId,
        const QVector<QUuid> &strokeIds,
        const QTransform &transform);
    void handleStrokesDuplicated(const QUuid &layerId,
        const QVector<QUuid> &sourceIds,
        const QVector<QUuid> &duplicateIds,
        const QPointF &delta,
        bool duplicated);
    void handleSelectionOverlayTransition(const QUuid &layerId,
        const QVector<QUuid> &fromStrokeIds,
        const QVector<QUuid> &toStrokeIds,
        const QImage &fromMask,
        const QImage &toMask);
    void handleCanvasResized(const QSize &previousSize,
        const QSize &currentSize,
        const QTransform &transform);
    void rebuildSelectionOutline();
    void updateSelectionAnimation();
    void notifySelectionTransformAvailability();
    bool flipSelection(bool horizontal);
    void updateSelectionActionBar();
    QPointF clampedSelectionDelta(const QPointF &delta) const;
    QImage renderActiveLayerImage() const;
    QImage renderReferenceLayersImage() const;
    QImage renderAllVisibleLayersImage() const;
    void drawSelectionOverlay(QPainter &painter, const QTransform &transform);

    DocumentController *m_controller;
    Tool m_tool = Tool::Brush;
    QColor m_brushColor = Qt::black;
    qreal m_brushWidth = 6.0;
    qreal m_eraserWidth = 6.0;
    StrokeStabilizer m_strokeStabilizer;
    bool m_brushAntialiasing = false;
    bool m_tabletPressureEnabled = true;
    bool m_wobbleAnimationEnabled = true;
    QString m_brushPresetId;
    QString m_eraserPresetId;
    BrushSettings m_brushSettings;
    BrushSettings m_eraserSettings;
    QHash<QString, qreal> m_brushPresetWidths;
    QHash<QString, qreal> m_brushPresetStabilizations;
    QHash<QString, qreal> m_eraserPresetWidths;
    QHash<QString, qreal> m_eraserPresetStabilizations;
    WandReference m_wandReference = WandReference::ActiveLayer;
    FillComparison m_fillComparison = FillComparison::AlphaBoundary;
    int m_fillTolerance = 32;
    bool m_bucketAntialiasing = true;
    SelectionShape m_selectionShape = SelectionShape::Freehand;
    LassoMode m_lassoMode = LassoMode::Select;
    SamplingMode m_selectionTransformSampling = SamplingMode::Smooth;
    QString m_textContent;
    QString m_textFontFamily;
    qreal m_textFontSize = 48.0;
    bool m_textFilled = false;
    bool m_textPlacementActive = false;
    bool m_textDragging = false;
    QPointF m_textAnchor;
    QPointF m_textDragStart;
    QPointF m_textAnchorAtDragStart;
    bool m_animating = true;
    bool m_animateWhileDrawing = false;
    bool m_groupSelectionActive = false;
    int m_currentFrame = 0;
    qreal m_zoom = 1.0;
    QPointF m_pan;
    qreal m_canvasRotation = 0.0;
    bool m_canvasMirrored = false;
    QCache<int, QImage> m_frameCache;
    QSize m_cachedRenderSize;
    // Frames still cached but rendered before the strokes covered by the
    // pending regional refresh; they display stale pixels only inside
    // m_frameCacheRefreshOutputBounds and must be patched before use.
    QSet<int> m_frameCacheStaleFrames;
    QRect m_frameCacheRefreshNativeBounds;
    QRect m_frameCacheRefreshOutputBounds;
    std::shared_ptr<const Document> m_frameCacheRefreshDocument;
    PendingStrokeRefreshHint m_pendingStrokeRefreshHint;
    // Armed by endStroke around commitStroke. The invalidation the commit
    // triggers must not start an interaction warmup itself: endStroke decides
    // after the commit, when it knows whether the pen-up promotion produced
    // the current frame and a reusable base that make the warmup redundant.
    bool m_strokeCommitDefersInteractionWarmup = false;
    std::shared_ptr<const Document> m_frameCacheWarmupDocument;
    std::shared_ptr<std::atomic_bool> m_frameCacheWarmupCancellation;
    QVector<int> m_frameCacheWarmupFrames;
    QSize m_frameCacheWarmupRenderSize;
    // Non-empty while the warmup run patches stale frames regionally instead
    // of rendering whole frames.
    QRect m_frameCacheWarmupPatchBounds;
    qsizetype m_frameCacheWarmupCursor = 0;
    quint64 m_frameCacheWarmupGeneration = 0;
    bool m_frameCacheWarmupActive = false;
    int m_frameCacheWarmupWorkersRunning = 0;
    QThreadPool m_frameCacheWarmupPool;
    QImage m_colorPickFrame;
    int m_colorPickFrameIndex = -1;
    RenderEngine::LayerSplitFrame m_previewSplit;
    QUuid m_previewSplitLayer;
    int m_previewSplitFrame = -1;
    RenderEngine::LayerRasterFrame m_previewLayerRasters;
    int m_previewLayerRasterFrame = -1;
    PreparedInteractionFrame m_preparedInteractionFrame;
    std::shared_ptr<std::atomic_bool> m_interactionFrameCancellation;
    QSize m_interactionFrameDesiredSize;
    QUuid m_interactionFrameDesiredLayer;
    int m_interactionFrameDesiredFrame = -1;
    QSize m_interactionFrameWorkerSize;
    QUuid m_interactionFrameWorkerLayer;
    int m_interactionFrameWorkerFrame = -1;
    quint64 m_interactionFrameWorkerGeneration = 0;
    quint64 m_interactionFrameGeneration = 0;
    bool m_interactionFrameWarmupActive = false;
    QFutureWatcher<PreparedInteractionFrame> m_interactionFrameWatcher;
    QThreadPool m_interactionFramePool;
    mutable quint64 m_synchronousPreviewRenderCount = 0;
    QImage m_activeStrokePreview;
    QSize m_activeStrokePreviewRenderSize;
    int m_activeStrokePreviewFrame = -1;
    bool m_activeStrokePreviewResolved = false;
    bool m_activeStrokePreviewIncludesStroke = false;
    // Union of the preview pixels that changed in the last resolve, in output
    // coordinates. Only meaningful while the valid flag is set; an empty rect
    // then means nothing changed. Lets continueStroke repaint just the stroke
    // tail instead of the whole widget.
    QRect m_activeStrokePreviewPatchBounds;
    bool m_activeStrokePreviewPatchBoundsValid = false;
    // Cleared by every resolveDisplayedFrame, so it marks whether the pointer
    // reports that arrived since the last paint have already paid for a
    // preview resolve. See continueStroke.
    bool m_strokePreviewResolvedSincePaint = false;
    // The canvas drop shadow, baked once per geometry. Its fourteen
    // antialiased passes are cheap along the axes and expensive at an angle,
    // so redrawing them every repaint made a rotated canvas crawl.
    QPixmap m_shadowCache;
    QPolygonF m_shadowCacheOutline;
    qreal m_shadowCacheRatio = 0.0;
    IncrementalStrokeRenderer m_incrementalStrokeRenderer;
    QImage m_composedPreviewFrame;
    QRect m_composedSelectionPreviewRegion;
    qint64 m_composedPreviewBaseKey = 0;
    // Regional edits to the displayed frame (stroke tail patches, selection
    // preview regions) accumulate here until resolveDisplayedFrame consumes
    // them. Painting bumps a QImage's cacheKey even in place, so each edit
    // also records the post-edit key; a resolved image carrying that key
    // changed only inside the accumulated bounds, while any other new key
    // means the frame was replaced wholesale.
    QRect m_displayedFrameDirtyAccum;
    qint64 m_displayedFrameKey = 0;
    qint64 m_displayedFramePatchedKey = 0;
    QSize m_displayedFrameSize;
    QTimer m_animationTimer;
    QTimer m_selectionAnimationTimer;
    QTimer m_zoomRenderTimer;
    Stroke m_activeStroke;
    QUuid m_activeStrokeLayer;
    bool m_activeStrokeUsesTabletPressure = true;
    bool m_drawing = false;
    bool m_panning = false;
    bool m_spacePressed = false;
    bool m_shiftPressed = false;
    bool m_tabletSequence = false;
    bool m_tabletPointerEraser = false;
    bool m_zoomDragging = false;
    bool m_rotatingCanvas = false;
    bool m_touchSequence = false;
    bool m_touchGestureSuppressed = false;
    bool m_touchGestureActive = false;
    const QPointingDevice *m_touchDevice = nullptr;
    bool m_pickingColor = false;
    QPointF m_zoomDragStart;
    QPointF m_zoomDragAnchor;
    bool m_zoomDragAnchorInside = false;
    qreal m_zoomDragStartZoom = 1.0;
    QPointF m_rotationDragStart;
    qreal m_rotationDragStartAngle = 0.0;
    int m_touchFirstPointId = -1;
    int m_touchSecondPointId = -1;
    QPointF m_touchGestureLastCenter;
    qreal m_touchGestureLastDistance = 0.0;
    qreal m_touchGestureLastAngle = 0.0;
    QPointF m_lastPanPosition;
    QPointF m_pointerWidgetPosition;
    bool m_pointerInside = false;
    bool m_pointerOverWidget = false;
    QVector<QPointF> m_areaSelectionPoints;
    QPointF m_areaSelectionAnchor;
    QPointF m_areaSelectionCurrent;
    bool m_areaSelectionActive = false;
    // Move mode requires the async visibility evaluation to confirm content,
    // so copySelection arms it here and the evaluation result applies it.
    // The arm is scoped to the selection it was created for; a completion for
    // any other selection retires it instead of consuming it.
    bool m_armSelectionMoveMode = false;
    QUuid m_armSelectionMoveLayer;
    qint64 m_armSelectionMoveMaskKey = 0;
    SelectionCombine m_areaSelectionCombine = SelectionCombine::Replace;
    SelectionState m_selectionBeforeArea;
    bool m_hasSelectionBeforeArea = false;
    QSet<QUuid> m_selectedStrokes;
    quint64 m_selectionVisibilityGeneration = 0;
    // An evaluation renders the selected layer across the whole animation, so
    // one that has been superseded stops rather than holding a pool thread
    // against its own replacement. The flag outlives the widget through the
    // running task's copy of the pointer.
    std::shared_ptr<std::atomic_bool> m_selectionVisibilityCancellation;
    QUuid m_selectionLayer;
    QImage m_selectionMask;
    mutable QVector<QUuid> m_editableStrokeIds;
    mutable qint64 m_editableStrokeMaskKey = 0;
    mutable QUuid m_editableStrokeLayer;
    mutable int m_editableStrokeFrame = -1;
    QPainterPath m_selectionOutline;
    qreal m_selectionDashOffset = 0.0;
    bool m_movingSelection = false;
    QPointF m_moveStartPosition;
    QTransform m_moveBaseTransform;
    bool m_moveStartedTransformSession = false;
    FloatingTransformSession m_selectionTransformSession;
    QPointer<SelectionActionBar> m_selectionActionBar;
    bool m_selectionMoveMode = false;
    CanvasFrameView *m_frameView = nullptr;
    CanvasOverlayView *m_overlayView = nullptr;
};

}

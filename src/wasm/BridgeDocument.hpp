// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#pragma once

// State and helpers shared by the bridge translation units. The C ABI itself
// is split by subject: document properties, strokes, selection, layers,
// render and export.

#include "document/Document.hpp"
#include "document/DocumentController.hpp"
#include "document/SelectionOperation.hpp"
#include "input/StrokeStabilizer.hpp"
#include "render/FloodFillMask.hpp"
#include "render/IncrementalStrokeRenderer.hpp"
#include "render/RenderEngine.hpp"

#include <QByteArray>
#include <QImage>
#include <QPointF>
#include <QRect>
#include <QSize>
#include <QTransform>
#include <QUuid>
#include <QVector>

#include <cstdint>
#include <emscripten/emscripten.h>
#include <memory>
#include <optional>

namespace ugurugu::wasm
{

struct BridgeDocument
{
    std::unique_ptr<ugurugu::DocumentController> controller =
        std::make_unique<ugurugu::DocumentController>();
    ugurugu::Stroke brushTemplate;
    bool brushAntialiasing = false;
    ugurugu::StrokeStabilizer stabilizer;
    ugurugu::Stroke activeStroke;
    bool strokeInProgress = false;
    int strokeFrame = 0;
    QPointF lastRawPosition;
    quint64 lastTimestamp = 0;
    // Survives a committed stroke: ugu_stroke_end bakes the finished stroke
    // into layerBase, so the next stroke on the same layer and frame skips
    // renderLayerSplit entirely. splitFrame/-Layer identify what it holds.
    ugurugu::RenderEngine::LayerSplitFrame split;
    QUuid splitLayerId;
    int splitFrame = -1;
    bool splitUsable = false;
    ugurugu::IncrementalStrokeRenderer incremental;
    QUuid strokeLayerId;
    bool incrementalActive = false;
    QImage renderedFrame;
    // Which frame renderedFrame shows, and whether it shows it without a live
    // stroke drawn over it.
    int renderedFrameIndex = -1;
    bool renderedFrameCommitted = false;
    QRect dirty;
    QImage thumbnail;
    QByteArray serialized;
    QByteArray exportBytes;
    QByteArray scratchText;
    // Separate from scratchText so a caller can read a layer's name and its
    // identity without the second call invalidating the first pointer.
    QByteArray scratchLayerId;
    // Selection state, held exactly the way CanvasWidget holds it: a
    // Grayscale8 mask the size of the canvas plus the layer it was made on.
    // Strokes clip to it while it belongs to the layer being painted.
    QImage selectionMask;
    QUuid selectionLayerId;
    // Bumped on every selection change so the shell can skip re-reading an
    // outline it already has.
    int selectionRevision = 0;
    QVector<float> outlinePoints;
    // Floating selection transform, held the way CanvasWidget holds its
    // FloatingTransformSession: the mask and the operation are captured once
    // when the session opens and every update only replaces the matrix, so the
    // committed transform is the one the preview drew.
    bool transformActive = false;
    QUuid transformLayerId;
    QImage transformSourceMask;
    ugurugu::PixelSelectionOp transformOperation;
    QTransform transformMatrix;
    int transformFrame = -1;
    // History position when the session opened. Anything that pushes, undoes
    // or redoes a command makes the captured pixels describe a document that
    // no longer exists, and the commit has to refuse rather than guess.
    int transformHistoryIndex = 0;
    // The committed frame each preview patches over, plus the region the last
    // preview touched so the next one knows what to restore.
    QImage transformBaseFrame;
    QRect transformPreviewRegion;
    ugurugu::FloodFillMask::Comparison fillComparison =
        ugurugu::FloodFillMask::Comparison::AlphaBoundary;
    int fillTolerance = 32;
    int fillReference = 0;
    bool bucketAntialiasing = true;
};

// Mirrors the UguStatus values documented on ugu_last_error_code(). Kept as
// plain ints because the C ABI carries them as ints anyway.
enum BridgeStatus
{
    StatusOk = 0,
    StatusInvalidArgument = 1,
    StatusOutOfMemory = 2,
    StatusDocumentInvalid = 3,
    StatusNoPaintLayer = 4,
    StatusStrokeRejected = 5,
    StatusRenderFailed = 6,
    StatusExportTooLarge = 7,
    StatusExportFailed = 8,
    StatusNoSelection = 9,
    StatusEmptyRegion = 10,
    StatusLayerNotDrawable = 11
};

// Selection combine modes, matching CanvasSelectionCombine.
enum BridgeCombine
{
    CombineReplace = 0,
    CombineAdd = 1,
    CombineSubtract = 2
};

// Selection shapes, matching CanvasSelectionShape.
enum BridgeShape
{
    ShapeFreehand = 0,
    ShapeRectangle = 1,
    ShapeEllipse = 2
};

// Flood-fill reference sources, matching CanvasWandReference.
enum BridgeReference
{
    ReferenceActiveLayer = 0,
    ReferenceMarkedLayers = 1,
    ReferenceAllVisibleLayers = 2
};

QByteArray &lastError();
int &lastErrorCode();
void setError(int code, QByteArray message);
void clearError();

QUuid paintTargetLayer(const ugurugu::Document &document);
const ugurugu::Layer *layerAtIndex(const BridgeDocument *handle, int index);
ugurugu::StrokePoint canvasPoint(
    const BridgeDocument *handle, const QPointF &position, double pressure);

void invalidateSplit(BridgeDocument *handle);
bool ensureLayerSplit(BridgeDocument *handle, const QUuid &layerId, int frame);
void renderCommittedFrame(BridgeDocument *handle, int frameIndex);
void renderFullStrokePreview(BridgeDocument *handle);
bool updateIncrementalStroke(BridgeDocument *handle, QRect *dirtyOut);

bool selectionAppliesTo(const BridgeDocument *handle, const QUuid &layerId);
bool selectionMaskUsable(
    const BridgeDocument *handle, const QImage &mask, const QUuid &layerId);
void installSelection(
    BridgeDocument *handle, QImage mask, const QUuid &layerId);
void pushSelectionChange(BridgeDocument *handle,
    const QImage &nextMask,
    const QUuid &nextLayerId,
    const char *text);
QImage combinedSelectionMask(
    const QImage &base, const QImage &addition, int combine);
void applySelectionCombine(BridgeDocument *handle,
    const QImage &mask,
    const QUuid &layerId,
    int combine);
void attachSelectionHistory(BridgeDocument *handle);

void clearSelectionTransform(BridgeDocument *handle);
void restoreSelectionTransformBase(BridgeDocument *handle);
void renderSelectionTransformPreview(BridgeDocument *handle);

QImage referenceImage(BridgeDocument *handle, int frame, const QUuid &layerId);
QImage maskFromShape(
    const QSize &size, int shape, const double *points, int count);
int commitFrozenFill(BridgeDocument *handle, const QImage &coverage, int frame);

void afterCanvasResize(BridgeDocument *handle);
std::optional<ugurugu::MotionSettings> motionFromValues(int style,
    int poseCount,
    int detail,
    double linked,
    double randomness,
    int brokenLine,
    double breakAmount,
    double breakRange);

}

// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "document/DocumentController.hpp"

#include "document/DocumentBudget.hpp"
#include "document/DocumentLimits.hpp"
#include "document/LayerHierarchy.hpp"
#include "document/SelectionOperation.hpp"
#include "document/SelectionVisibility.hpp"
#include "document/StrokeMask.hpp"
#include "document/history/DocumentDelta.hpp"
#include "document/history/HistoryEffects.hpp"
#include "document/history/HistoryMemory.hpp"
#include "document/history/LogicalHistoryCommand.hpp"
#include "io/DocumentSerializer.hpp"
#include "render/RasterAssetCache.hpp"
#include "render/RenderEngine.hpp"

#include <QHash>
#include <QPainter>
#include <QPointer>
#include <QScopedValueRollback>
#include <QSet>

#include <algorithm>
#include <cmath>
#include <limits>
#include <memory>
#include <new>
#include <type_traits>
#include <utility>
#include <variant>
#include <vector>

namespace ugurugu
{

using DocumentBudget::distinctClipMaskBytes;
using DocumentBudget::totalStrokeCount;
using history::accountByteStorage;
using history::accountImageStorage;
using history::accountPackedMask;
using history::accountVectorStorage;
using history::animationFramesMergeId;
using history::breakAmountMergeId;
using history::breakRangeMergeId;
using history::brokenLineMergeId;
using history::DocumentDelta;
using history::framesPerSecondMergeId;
using history::layerOpacityMergeId;
using history::LogicalHistoryCommand;
using history::MemoryFootprint;
using history::motionDetailMergeId;
using history::motionLinkedMergeId;
using history::motionPoseCountMergeId;
using history::motionRandomnessMergeId;
using history::motionStyleMergeId;
using history::wobbleAmountMergeId;

namespace
{

void pruneUnreferencedRasterAssets(Document &document)
{
    QSet<QString> referenced;
    for (const Layer &layer : std::as_const(document.layers))
    {
        for (const Stroke &stroke : layer.strokes)
        {
            if (stroke.imageOp)
            {
                referenced.insert(stroke.imageOp->assetId);
            }
        }
    }
    for (auto asset = document.rasterAssets.begin();
        asset != document.rasterAssets.end();)
    {
        if (!referenced.contains(asset.key()))
        {
            asset = document.rasterAssets.erase(asset);
        }
        else
        {
            ++asset;
        }
    }
}

bool layerCanProducePixels(const Layer &layer)
{
    return std::any_of(layer.strokes.cbegin(),
        layer.strokes.cend(),
        [](const Stroke &stroke)
        {
            return stroke.mode == StrokeMode::Paint
                   || stroke.mode == StrokeMode::Fill
                   || stroke.mode == StrokeMode::Image;
        });
}

}

struct DocumentController::MacroTransaction
{
    QString text;
    int depth = 1;
    bool failed = false;
    PreparedState startState;
    PreparedState workingState;
    HistoryEffects effects;
};

class DocumentController::DocumentCommand final : public LogicalHistoryCommand
{
public:
    DocumentCommand(DocumentController *owner,
        QString text,
        DocumentDelta delta,
        PreparedState initialRedo,
        ActiveLayerPolicy activeLayerPolicy,
        std::shared_ptr<const HistoryEffects> effects,
        DocumentSerializer::ImmutableBackingLease beforeLease,
        DocumentSerializer::ImmutableBackingLease afterLease,
        int mergeId,
        QUuid mergeScope,
        quint64 beforeNode,
        quint64 afterNode,
        quint64 beforeRevision,
        quint64 afterRevision,
        qint64 beforeCompactSize,
        qint64 afterCompactSize)
        : LogicalHistoryCommand(std::move(text))
        , m_owner(owner)
        , m_delta(std::move(delta))
        , m_initialRedo(std::move(initialRedo))
        , m_activeLayerPolicy(activeLayerPolicy)
        , m_effects(std::move(effects))
        , m_beforeLease(std::move(beforeLease))
        , m_afterLease(std::move(afterLease))
        , m_mergeId(mergeId)
        , m_mergeScope(mergeScope)
        , m_beforeNode(beforeNode)
        , m_afterNode(afterNode)
        , m_beforeRevision(beforeRevision)
        , m_afterRevision(afterRevision)
        , m_beforeCompactSize(beforeCompactSize)
        , m_afterCompactSize(afterCompactSize)
    {
    }

    int id() const override
    {
        return m_mergeId;
    }

    bool mergeWith(const LogicalHistoryCommand *other) override
    {
        const auto *command = dynamic_cast<const DocumentCommand *>(other);
        if (!command || command->m_owner != m_owner
            || command->m_mergeId != m_mergeId
            || command->m_mergeScope != m_mergeScope
            || command->m_activeLayerPolicy != m_activeLayerPolicy
            || m_afterNode != command->m_beforeNode || command->m_firstRedo
            || command->m_initialRedo || command->m_staged
            || !m_delta.mergeScalar(command->m_delta, m_mergeId, m_mergeScope))
        {
            return false;
        }
        m_afterNode = command->m_afterNode;
        m_afterRevision = command->m_afterRevision;
        m_afterCompactSize = command->m_afterCompactSize;
        m_afterLease = command->m_afterLease;
        m_effects = command->m_effects;
        m_delta.normalizeMergedChanges();
        if (m_delta.isEmpty())
        {
            setObsolete(true);
            m_owner->normalizeMergedNoOp(m_beforeNode, m_beforeRevision);
        }
        return true;
    }

    void redo() override
    {
        PreparedState target;
        ActiveLayerPolicy policy = ActiveLayerPolicy::UsePrepared;
        if (m_firstRedo)
        {
            m_firstRedo = false;
            target = std::exchange(m_initialRedo, {});
            policy = m_activeLayerPolicy;
        }
        else
        {
            target = std::exchange(m_staged, {});
        }
        requireReady(target, m_beforeNode, "redo");
        m_owner->applyPreparedState(target,
            policy,
            *m_effects,
            CommitDirection::Forward,
            m_afterNode,
            m_afterRevision);
    }

    void undo() override
    {
        PreparedState target = std::exchange(m_staged, {});
        requireReady(target, m_afterNode, "undo");
        m_owner->applyPreparedState(target,
            ActiveLayerPolicy::UsePrepared,
            *m_effects,
            CommitDirection::Reverse,
            m_beforeNode,
            m_beforeRevision);
    }

    bool preflight(bool forward) override
    {
        if (!m_owner || !m_owner->m_currentState || m_staged)
        {
            return false;
        }
        PreparedState cursor = m_owner->m_currentState;
        quint64 cursorNode = m_owner->m_currentHistoryNode;
        const quint64 sourceNode = forward ? m_beforeNode : m_afterNode;
        if (cursorNode != sourceNode)
        {
            return false;
        }
        Document candidate = cursor->document();
        if (!m_delta.apply(candidate, forward))
        {
            return false;
        }
        if (m_activeLayerPolicy == ActiveLayerPolicy::PreserveCurrentIfPresent)
        {
            const QUuid active = cursor->document().activeLayerId;
            const bool canPreserve = candidate.layers.isEmpty()
                                         ? active.isNull()
                                         : candidate.layer(active) != nullptr;
            if (canPreserve)
            {
                candidate.activeLayerId = active;
            }
        }

        PreparedState prepared = m_owner->prepareState(std::move(candidate),
            cursor.get(),
            forward ? &m_afterLease : &m_beforeLease,
            true);
        const qint64 expectedSize =
            forward ? m_afterCompactSize : m_beforeCompactSize;
        if (!prepared || prepared->compactSize() != expectedSize)
        {
            return false;
        }
        m_staged = prepared;
        return true;
    }

    void clearPreflight() override
    {
        m_staged.reset();
    }

    DocumentUndoStack::StorageStats storageStats() const override
    {
        DocumentUndoStack::StorageStats stats = m_delta.storageStats();
        stats.retainedPreparedDocuments =
            (m_initialRedo ? 1 : 0) + (m_staged ? 1 : 0);
        stats.stagedPreparedDocuments = m_staged ? 1 : 0;
        return stats;
    }

    void accountStorage(MemoryFootprint &footprint) const override
    {
        m_delta.accountStorage(footprint);
        if (m_effects)
        {
            m_effects->accountStorage(footprint);
        }
        footprint.addOwned(sizeof(DocumentCommand));
        footprint.addOwned(static_cast<qint64>(text().capacity())
                           * static_cast<qint64>(sizeof(QChar)));
    }

private:
    void requireReady(const PreparedState &target,
        quint64 expectedNode,
        const char *operation) const
    {
        if (!m_owner || !target || !target->isValid()
            || m_owner->m_currentHistoryNode != expectedNode)
        {
            qFatal("Document history invariant failed during %s", operation);
        }
    }

    DocumentController *m_owner = nullptr;
    DocumentDelta m_delta;
    PreparedState m_initialRedo;
    PreparedState m_staged;
    ActiveLayerPolicy m_activeLayerPolicy =
        ActiveLayerPolicy::PreserveCurrentIfPresent;
    std::shared_ptr<const HistoryEffects> m_effects;
    DocumentSerializer::ImmutableBackingLease m_beforeLease;
    DocumentSerializer::ImmutableBackingLease m_afterLease;
    int m_mergeId = -1;
    QUuid m_mergeScope;
    quint64 m_beforeNode = 0;
    quint64 m_afterNode = 0;
    quint64 m_beforeRevision = 0;
    quint64 m_afterRevision = 0;
    qint64 m_beforeCompactSize = 0;
    qint64 m_afterCompactSize = 0;
    bool m_firstRedo = true;
};

class DocumentController::TransientCommand final : public LogicalHistoryCommand
{
public:
    TransientCommand(DocumentController *owner,
        QString text,
        std::shared_ptr<const HistoryEffects> effects)
        : LogicalHistoryCommand(std::move(text))
        , m_owner(owner)
        , m_effects(std::move(effects))
    {
    }

    bool preflight(bool) override
    {
        return m_owner && m_effects;
    }

    void clearPreflight() override
    {
    }

    void redo() override
    {
        if (m_owner && m_effects)
        {
            m_owner->dispatchHistoryEffects(
                *m_effects, CommitDirection::Forward, false);
        }
    }

    void undo() override
    {
        if (m_owner && m_effects)
        {
            m_owner->dispatchHistoryEffects(
                *m_effects, CommitDirection::Reverse, false);
        }
    }

    DocumentUndoStack::StorageStats storageStats() const override
    {
        return {};
    }

    void accountStorage(MemoryFootprint &footprint) const override
    {
        if (m_effects)
        {
            m_effects->accountStorage(footprint);
        }
        footprint.addOwned(sizeof(TransientCommand));
        footprint.addOwned(static_cast<qint64>(text().capacity())
                           * static_cast<qint64>(sizeof(QChar)));
    }

private:
    DocumentController *m_owner = nullptr;
    std::shared_ptr<const HistoryEffects> m_effects;
};

DocumentController::DocumentController(QObject *parent)
    : QObject(parent)
    , m_undoStack(this)
{
    m_currentState =
        prepareState(Document::createDefault(QSize(1024, 768), tr("Layer 1")));
    Q_ASSERT(m_currentState);
    m_undoStack.setUndoLimit(64);
    m_undoStack.setClean();
}

DocumentController::~DocumentController() = default;

const Document &DocumentController::document() const
{
    static const Document empty;
    const PreparedState &state = editableState();
    return state ? state->document() : empty;
}

DocumentUndoStack *DocumentController::undoStack()
{
    return &m_undoStack;
}

bool DocumentController::isModified() const
{
    return m_currentContentRevision != m_savedContentRevision;
}

void DocumentController::releaseTransientCaches()
{
    m_serializationCache.clear();
    RasterAssetCache::clear();
}

bool DocumentController::selectionHasVisibleLayerPixels(
    const QUuid &layerId, const QImage &selectionMask, int preferredFrame) const
{
    const Document &current = document();
    const Layer *layer = current.layer(layerId);
    if (!layer || selectionMask.isNull() || selectionMask.size() != current.size
        || selectionMask.format() != QImage::Format_Grayscale8)
    {
        return false;
    }
    if (const std::optional<bool> cached =
            cachedSelectionVisibility(layerId, selectionMask))
    {
        return *cached;
    }

    const SelectionVisibility::Result visibility =
        SelectionVisibility::evaluate(
            current, *layer, selectionMask, preferredFrame);
    m_selectionVisibilityCacheValid = visibility.renderSucceeded;
    m_selectionVisibilityLayerId = layerId;
    m_selectionVisibilityMaskKey = selectionMask.cacheKey();
    m_selectionVisibilityCacheResult = visibility.hasVisiblePixels;
    return visibility.hasVisiblePixels;
}

std::optional<bool> DocumentController::cachedSelectionVisibility(
    const QUuid &layerId, const QImage &selectionMask) const
{
    if (!m_selectionVisibilityCacheValid || selectionMask.isNull()
        || m_selectionVisibilityLayerId != layerId
        || m_selectionVisibilityMaskKey != selectionMask.cacheKey())
    {
        return std::nullopt;
    }
    return m_selectionVisibilityCacheResult;
}

void DocumentController::cacheSelectionVisibility(
    const QUuid &layerId, const QImage &selectionMask, bool visible)
{
    const Document &current = document();
    if (!current.layer(layerId) || selectionMask.isNull()
        || selectionMask.size() != current.size
        || selectionMask.format() != QImage::Format_Grayscale8)
    {
        return;
    }
    m_selectionVisibilityCacheValid = true;
    m_selectionVisibilityLayerId = layerId;
    m_selectionVisibilityMaskKey = selectionMask.cacheKey();
    m_selectionVisibilityCacheResult = visible;
}

void DocumentController::pushSelectionStateCommand(const QString &text,
    const QUuid &beforeLayerId,
    const QImage &beforeMask,
    const QUuid &afterLayerId,
    const QImage &afterMask)
{
    if (m_undoStack.m_moving)
    {
        return;
    }
    const auto snapshot =
        [](const QImage &mask) -> std::optional<PackedMaskRegion>
    {
        return mask.isNull() ? std::optional<PackedMaskRegion>()
                             : packBinaryMask(mask);
    };
    const std::optional<PackedMaskRegion> before = snapshot(beforeMask);
    const std::optional<PackedMaskRegion> after = snapshot(afterMask);
    if ((!beforeMask.isNull() && !before) || (!afterMask.isNull() && !after))
    {
        failHistoryMacro();
        return;
    }
    auto effects = std::make_shared<HistoryEffects>();
    effects->selectionState = HistoryEffects::SelectionStateTransition{
        {beforeLayerId, before}, {afterLayerId, after}};
    if (!effects->hasSelectionTransition())
    {
        return;
    }
    auto frozenEffects =
        std::make_shared<const HistoryEffects>(effects->frozenCopy());
    if (m_macroTransaction)
    {
        if (!m_macroTransaction->failed)
        {
            m_macroTransaction->effects.append(*frozenEffects);
        }
        return;
    }
    m_undoStack.push(
        new TransientCommand(this, text, std::move(frozenEffects)));
}

bool DocumentController::newDocument(const QSize &size, QString *error)
{
    const QSize normalized(std::clamp(size.width(),
                               DocumentLimits::minimumCanvasEdge,
                               DocumentLimits::maximumCanvasEdge),
        std::clamp(size.height(),
            DocumentLimits::minimumCanvasEdge,
            DocumentLimits::maximumCanvasEdge));
    Document document;
    try
    {
        document = Document::createDefault(normalized, tr("Layer 1"));
    }
    catch (const std::bad_alloc &)
    {
        if (error)
        {
            *error = tr("There is not enough memory to prepare the document.");
        }
        return false;
    }
    return replaceDocument(
        std::move(document), DocumentReplacementDisposition::Clean, error);
}

bool DocumentController::loadDocument(Document document, QString *error)
{
    return replaceDocument(
        std::move(document), DocumentReplacementDisposition::Clean, error);
}

bool DocumentController::loadRecoveredDocument(
    Document document, QString *error)
{
    return replaceDocument(
        std::move(document), DocumentReplacementDisposition::Recovered, error);
}

bool DocumentController::replaceDocument(Document document,
    DocumentReplacementDisposition disposition,
    QString *error)
{
    if (error)
    {
        error->clear();
    }
    if (m_undoStack.m_moving || hasOpenHistoryMacro())
    {
        if (error)
        {
            *error =
                tr("Cannot replace a document during a history transaction.");
        }
        return false;
    }
    if (m_documentReplacementInProgress)
    {
        if (error)
        {
            *error = tr("Cannot replace a document during another document "
                        "transition.");
        }
        return false;
    }

    QScopedValueRollback<bool> replacement(
        m_documentReplacementInProgress, true);
    PreparedState prepared;
    try
    {
        ensureActiveLayer(document);
        if (m_failNextDocumentReplacementPreparationForTesting)
        {
            m_failNextDocumentReplacementPreparationForTesting = false;
            if (error)
            {
                *error = tr("The document could not be prepared.");
            }
            return false;
        }
        prepared =
            prepareState(std::move(document), nullptr, nullptr, false, error);
    }
    catch (const std::bad_alloc &)
    {
        if (error)
        {
            *error = tr("There is not enough memory to prepare the document.");
        }
        return false;
    }
    if (!prepared)
    {
        if (error && error->isEmpty())
        {
            *error = tr("The document could not be prepared.");
        }
        return false;
    }

    const bool wasModified = isModified();
    m_currentState = prepared;
    const bool recovered =
        disposition == DocumentReplacementDisposition::Recovered;
    m_currentContentRevision = recovered ? 1 : 0;
    m_savedContentRevision = 0;
    m_nextContentRevision = recovered ? 1 : 0;
    m_currentHistoryNode = 0;
    m_nextHistoryNode = 0;
    m_selectionVisibilityCacheValid = false;
    m_undoStack.clear();
    m_undoStack.setClean();
    emit documentReplaced();
    emit documentChanged();
    emit layerThumbnailsReset();
    emit activeLayerChanged(this->document().activeLayerId);
    const bool modified = isModified();
    if (modified != wasModified)
    {
        emit modifiedChanged(modified);
    }
    return true;
}

bool DocumentController::saveDocument(const QString &filePath, QString *error)
{
    if (m_undoStack.m_moving || hasOpenHistoryMacro())
    {
        failHistoryMacro();
        if (error)
        {
            *error = tr("Cannot save an unfinished history transaction.");
        }
        return rejectHistoryMutation();
    }
    return m_currentState
           && DocumentSerializer::save(
               filePath, *m_currentState, m_serializationCache, error);
}

QByteArray DocumentController::serializeDocument(
    const QJsonObject &additionalRootFields, QString *error)
{
    if (error)
    {
        error->clear();
    }
    if (m_undoStack.m_moving || hasOpenHistoryMacro())
    {
        if (error)
        {
            *error = tr("Cannot save an unfinished history transaction.");
        }
        return {};
    }
    return m_currentState ? DocumentSerializer::toJson(*m_currentState,
                                m_serializationCache,
                                additionalRootFields)
                          : QByteArray();
}

std::optional<DocumentSerializer::PreparedDocument>
DocumentController::serializationSnapshot(QString *error) const
{
    if (error)
    {
        error->clear();
    }
    if (m_undoStack.m_moving || hasOpenHistoryMacro() || !m_currentState)
    {
        if (error)
        {
            *error = tr("Cannot save an unfinished history transaction.");
        }
        return std::nullopt;
    }
    return *m_currentState;
}

void DocumentController::markSaved()
{
    if (m_undoStack.m_moving || hasOpenHistoryMacro())
    {
        failHistoryMacro();
        return;
    }
    const bool wasModified = isModified();
    m_savedContentRevision = m_currentContentRevision;
    m_undoStack.setClean();
    if (wasModified)
    {
        emit modifiedChanged(false);
    }
}

bool DocumentController::resizeImage(const QSize &size)
{
    const Document &current = document();
    if (size == current.size || size.width() < DocumentLimits::minimumCanvasEdge
        || size.height() < DocumentLimits::minimumCanvasEdge
        || size.width() > DocumentLimits::maximumCanvasEdge
        || size.height() > DocumentLimits::maximumCanvasEdge)
    {
        return rejectHistoryMutation();
    }

    Document resized = current;
    const qreal horizontalScale =
        static_cast<qreal>(size.width()) / current.size.width();
    const qreal verticalScale =
        static_cast<qreal>(size.height()) / current.size.height();
    QTransform transform;
    transform.scale(horizontalScale, verticalScale);
    resized.size = size;
    for (Layer &layer : resized.layers)
    {
        if (!layerCanProducePixels(layer))
        {
            layer.strokes.clear();
            layer.initialCanvasSize = size;
            continue;
        }
        if (!layer.initialCanvasSize.isValid())
        {
            layer.initialCanvasSize = current.size;
        }
        if (layer.strokes.size() >= DocumentLimits::maximumStrokesPerLayer)
        {
            return rejectHistoryMutation();
        }
        Stroke reframe;
        reframe.mode = StrokeMode::Reframe;
        reframe.reframeOp = ReframeOp{ReframeMode::Image,
            SamplingMode::Smooth,
            current.size,
            size,
            QPoint()};
        reframe.points.clear();
        layer.strokes.append(std::move(reframe));
    }
    if (totalStrokeCount(resized) > DocumentLimits::maximumTotalStrokes
        || distinctClipMaskBytes(resized)
               > DocumentLimits::maximumDistinctClipMaskBytes)
    {
        return rejectHistoryMutation();
    }

    bool invertible = false;
    const QTransform inverse = transform.inverted(&invertible);
    if (!invertible)
    {
        return rejectHistoryMutation();
    }
    auto effects = std::make_shared<HistoryEffects>();
    const QSize previousSize = current.size;
    effects->beforeDocumentChanged.append(
        HistoryEffects::CanvasResize{previousSize, size, transform, inverse});
    effects->afterDocumentChanged.append(
        HistoryEffects::LayerThumbnailsReset{});
    return tryCommitCandidate(
        tr("Resize image"), std::move(resized), std::move(effects));
}

bool DocumentController::resizeCanvas(
    const QSize &size, const QPoint &contentOffset)
{
    const Document &current = document();
    if ((size == current.size && contentOffset.isNull())
        || size.width() < DocumentLimits::minimumCanvasEdge
        || size.height() < DocumentLimits::minimumCanvasEdge
        || size.width() > DocumentLimits::maximumCanvasEdge
        || size.height() > DocumentLimits::maximumCanvasEdge
        || std::abs(static_cast<qreal>(contentOffset.x()))
               > DocumentLimits::maximumStoredCoordinateMagnitude
        || std::abs(static_cast<qreal>(contentOffset.y()))
               > DocumentLimits::maximumStoredCoordinateMagnitude)
    {
        return rejectHistoryMutation();
    }

    QTransform transform;
    transform.translate(contentOffset.x(), contentOffset.y());
    bool invertible = false;
    const QTransform inverse = transform.inverted(&invertible);
    if (!invertible)
    {
        return rejectHistoryMutation();
    }

    Document resized = current;
    resized.size = size;

    for (Layer &layer : resized.layers)
    {
        if (!layerCanProducePixels(layer))
        {
            layer.strokes.clear();
            layer.initialCanvasSize = size;
            continue;
        }
        if (!layer.initialCanvasSize.isValid())
        {
            layer.initialCanvasSize = current.size;
        }
        if (layer.strokes.size() >= DocumentLimits::maximumStrokesPerLayer)
        {
            return rejectHistoryMutation();
        }
        Stroke reframe;
        reframe.mode = StrokeMode::Reframe;
        reframe.reframeOp = ReframeOp{ReframeMode::Canvas,
            SamplingMode::Nearest,
            current.size,
            size,
            contentOffset};
        reframe.points.clear();
        layer.strokes.append(std::move(reframe));
    }
    if (totalStrokeCount(resized) > DocumentLimits::maximumTotalStrokes
        || distinctClipMaskBytes(resized)
               > DocumentLimits::maximumDistinctClipMaskBytes)
    {
        return rejectHistoryMutation();
    }

    auto effects = std::make_shared<HistoryEffects>();
    const QSize previousSize = current.size;
    effects->beforeDocumentChanged.append(
        HistoryEffects::CanvasResize{previousSize, size, transform, inverse});
    effects->afterDocumentChanged.append(
        HistoryEffects::LayerThumbnailsReset{});
    return tryCommitCandidate(
        tr("Resize canvas"), std::move(resized), std::move(effects));
}

void DocumentController::setActiveLayer(const QUuid &id)
{
    if (hasOpenHistoryMacro())
    {
        failHistoryMacro();
        return;
    }
    const Document &current = document();
    const Layer *layer = current.layer(id);
    if (current.activeLayerId == id || !layer
        || layer->kind != LayerKind::Paint)
    {
        return;
    }
    std::optional<PreparedDocument> rebound =
        DocumentSerializer::rebindActiveLayer(*m_currentState, id);
    if (!rebound)
    {
        return;
    }
    m_currentState =
        std::make_shared<const PreparedDocument>(std::move(*rebound));
    emit activeLayerChanged(id);
}

void DocumentController::setBackground(const QColor &color)
{
    const Document &current = document();
    if (!color.isValid() || current.background == color)
    {
        failHistoryMacro();
        return;
    }
    Document candidate = current;
    candidate.background = color;
    tryCommitCandidate(tr("Change background"), std::move(candidate));
}

void DocumentController::setWobbleAmount(qreal amount)
{
    if (!std::isfinite(amount))
    {
        failHistoryMacro();
        return;
    }
    const qreal normalized = std::clamp(amount,
        DocumentLimits::minimumWobbleAmount,
        DocumentLimits::maximumWobbleAmount);
    const Document &current = document();
    if (qFuzzyCompare(current.wobbleAmount, normalized))
    {
        failHistoryMacro();
        return;
    }
    Document candidate = current;
    candidate.wobbleAmount = normalized;
    tryCommitCandidate(tr("Change wobble"),
        std::move(candidate),
        {},
        ActiveLayerPolicy::PreserveCurrentIfPresent,
        wobbleAmountMergeId);
}

void DocumentController::setMotionStyle(MotionStyle style)
{
    if (!isValidMotionStyle(style))
    {
        failHistoryMacro();
        return;
    }
    const Document &current = document();
    if (current.motion.style == style)
    {
        failHistoryMacro();
        return;
    }
    Document candidate = current;
    candidate.motion.style = style;
    if (style != MotionStyle::Classic)
    {
        candidate.motion.poseCount =
            std::min(candidate.motion.poseCount, candidate.animationFrames);
    }
    tryCommitCandidate(tr("Change motion style"),
        std::move(candidate),
        {},
        ActiveLayerPolicy::PreserveCurrentIfPresent,
        motionStyleMergeId);
}

void DocumentController::setMotionPoseCount(int count)
{
    const Document &current = document();
    const int maximum = current.motion.style == MotionStyle::Classic
                            ? DocumentLimits::maximumMotionPoseCount
                            : current.animationFrames;
    const int normalized =
        std::clamp(count, DocumentLimits::minimumMotionPoseCount, maximum);
    if (current.motion.poseCount == normalized)
    {
        failHistoryMacro();
        return;
    }
    Document candidate = current;
    candidate.motion.poseCount = normalized;
    tryCommitCandidate(tr("Change motion pose count"),
        std::move(candidate),
        {},
        ActiveLayerPolicy::PreserveCurrentIfPresent,
        motionPoseCountMergeId);
}

void DocumentController::setMotionDetail(int detail)
{
    const int normalized = std::clamp(detail,
        DocumentLimits::minimumMotionDetail,
        DocumentLimits::maximumMotionDetail);
    const Document &current = document();
    if (current.motion.detail == normalized)
    {
        failHistoryMacro();
        return;
    }
    Document candidate = current;
    candidate.motion.detail = normalized;
    tryCommitCandidate(tr("Change motion detail"),
        std::move(candidate),
        {},
        ActiveLayerPolicy::PreserveCurrentIfPresent,
        motionDetailMergeId);
}

void DocumentController::setMotionLinked(qreal linked)
{
    if (!std::isfinite(linked))
    {
        failHistoryMacro();
        return;
    }
    const qreal normalized = std::clamp(linked, 0.0, 1.0);
    const Document &current = document();
    if (qFuzzyCompare(current.motion.linked, normalized))
    {
        failHistoryMacro();
        return;
    }
    Document candidate = current;
    candidate.motion.linked = normalized;
    tryCommitCandidate(tr("Change linked motion"),
        std::move(candidate),
        {},
        ActiveLayerPolicy::PreserveCurrentIfPresent,
        motionLinkedMergeId);
}

void DocumentController::setMotionRandomness(qreal randomness)
{
    if (!std::isfinite(randomness))
    {
        failHistoryMacro();
        return;
    }
    const qreal normalized = std::clamp(randomness, 0.0, 1.0);
    const Document &current = document();
    if (qFuzzyCompare(current.motion.randomness, normalized))
    {
        failHistoryMacro();
        return;
    }
    Document candidate = current;
    candidate.motion.randomness = normalized;
    tryCommitCandidate(tr("Change motion randomness"),
        std::move(candidate),
        {},
        ActiveLayerPolicy::PreserveCurrentIfPresent,
        motionRandomnessMergeId);
}

void DocumentController::setBrokenLineEnabled(bool enabled)
{
    const Document &current = document();
    if (current.motion.brokenLine == enabled)
    {
        failHistoryMacro();
        return;
    }
    Document candidate = current;
    candidate.motion.brokenLine = enabled;
    tryCommitCandidate(tr("Toggle broken line"),
        std::move(candidate),
        {},
        ActiveLayerPolicy::PreserveCurrentIfPresent,
        brokenLineMergeId);
}

void DocumentController::setBreakAmount(qreal amount)
{
    if (!std::isfinite(amount))
    {
        failHistoryMacro();
        return;
    }
    const qreal normalized = std::clamp(amount, 0.0, 1.0);
    const Document &current = document();
    if (qFuzzyCompare(current.motion.breakAmount, normalized))
    {
        failHistoryMacro();
        return;
    }
    Document candidate = current;
    candidate.motion.breakAmount = normalized;
    tryCommitCandidate(tr("Change break amount"),
        std::move(candidate),
        {},
        ActiveLayerPolicy::PreserveCurrentIfPresent,
        breakAmountMergeId);
}

void DocumentController::setBreakRange(qreal range)
{
    if (!std::isfinite(range))
    {
        failHistoryMacro();
        return;
    }
    const qreal normalized = std::clamp(range,
        DocumentLimits::minimumBreakRange,
        DocumentLimits::maximumBreakRange);
    const Document &current = document();
    if (qFuzzyCompare(current.motion.breakRange, normalized))
    {
        failHistoryMacro();
        return;
    }
    Document candidate = current;
    candidate.motion.breakRange = normalized;
    tryCommitCandidate(tr("Change break range"),
        std::move(candidate),
        {},
        ActiveLayerPolicy::PreserveCurrentIfPresent,
        breakRangeMergeId);
}

bool DocumentController::applyMotionPreset(
    qreal wobbleAmount, MotionSettings motion)
{
    if (!std::isfinite(wobbleAmount) || !isValidMotionStyle(motion.style))
    {
        return false;
    }
    const Document &current = document();
    motion.poseCount = std::clamp(motion.poseCount,
        DocumentLimits::minimumMotionPoseCount,
        motion.style == MotionStyle::Classic
            ? DocumentLimits::maximumMotionPoseCount
            : current.animationFrames);
    if (!isValidMotionSettings(motion, current.animationFrames))
    {
        return false;
    }
    Document candidate = current;
    candidate.wobbleAmount = std::clamp(wobbleAmount,
        DocumentLimits::minimumWobbleAmount,
        DocumentLimits::maximumWobbleAmount);
    candidate.motion = motion;
    if (candidate.wobbleAmount == current.wobbleAmount
        && candidate.motion == current.motion)
    {
        return true;
    }
    return tryCommitCandidate(tr("Apply WWP preset"), std::move(candidate));
}

void DocumentController::setAnimationFrames(int frames)
{
    const int normalized = std::clamp(frames,
        DocumentLimits::minimumAnimationFrames,
        DocumentLimits::maximumAnimationFrames);
    const Document &current = document();
    if (current.animationFrames == normalized)
    {
        failHistoryMacro();
        return;
    }
    Document candidate = current;
    candidate.animationFrames = normalized;
    int mergeId = animationFramesMergeId;
    if (candidate.motion.style != MotionStyle::Classic
        && candidate.motion.poseCount > normalized)
    {
        candidate.motion.poseCount = normalized;
        mergeId = 0;
    }
    for (Layer &layer : candidate.layers)
    {
        if (layer.motion && layer.motion->style != MotionStyle::Classic
            && layer.motion->poseCount > normalized)
        {
            layer.motion->poseCount = normalized;
            mergeId = 0;
        }
    }
    tryCommitCandidate(tr("Change animation frames"),
        std::move(candidate),
        {},
        ActiveLayerPolicy::PreserveCurrentIfPresent,
        mergeId);
}

void DocumentController::setFramesPerSecond(qreal fps)
{
    if (!std::isfinite(fps))
    {
        failHistoryMacro();
        return;
    }
    const qreal normalized = std::clamp(fps,
        DocumentLimits::minimumFramesPerSecond,
        DocumentLimits::maximumFramesPerSecond);
    const Document &current = document();
    if (qFuzzyCompare(current.framesPerSecond, normalized))
    {
        failHistoryMacro();
        return;
    }
    Document candidate = current;
    candidate.framesPerSecond = normalized;
    tryCommitCandidate(tr("Change animation speed"),
        std::move(candidate),
        {},
        ActiveLayerPolicy::PreserveCurrentIfPresent,
        framesPerSecondMergeId);
}

bool DocumentController::tryCommitCandidate(QString text,
    Document candidate,
    std::shared_ptr<const HistoryEffects> effects,
    ActiveLayerPolicy activeLayerPolicy,
    int mergeId,
    const QUuid &mergeScope)
{
    const PreparedState before = editableState();
    if (!before || m_undoStack.m_moving
        || (m_macroTransaction && m_macroTransaction->failed))
    {
        return false;
    }
    const LayerHierarchyAnalysis currentHierarchy =
        analyzeLayerHierarchy(before->document());
    pruneUnreferencedRasterAssets(candidate);
    const LayerHierarchyAnalysis candidateHierarchy =
        analyzeLayerHierarchy(candidate);
    if (!isLayerHierarchyDepthChangeAllowed(
            currentHierarchy, candidateHierarchy))
    {
        failHistoryMacro();
        return false;
    }
    const PreparedState after =
        prepareState(std::move(candidate), before.get());
    if (!after)
    {
        failHistoryMacro();
        return false;
    }
    return tryCommitPreparedCandidate(std::move(text),
        before,
        after,
        std::move(effects),
        activeLayerPolicy,
        mergeId,
        mergeScope);
}

bool DocumentController::tryCommitPreparedCandidate(QString text,
    const PreparedState &before,
    PreparedState after,
    std::shared_ptr<const HistoryEffects> effects,
    ActiveLayerPolicy activeLayerPolicy,
    int mergeId,
    const QUuid &mergeScope,
    const QUuid &appendedStrokeLayerId)
{
    if (!before || !after || before != editableState() || m_undoStack.m_moving
        || (m_macroTransaction && m_macroTransaction->failed))
    {
        failHistoryMacro();
        return false;
    }
    if (!effects)
    {
        effects = std::make_shared<const HistoryEffects>();
    }
    else
    {
        effects = std::make_shared<const HistoryEffects>(effects->frozenCopy());
    }
    DocumentDelta delta =
        appendedStrokeLayerId.isNull()
            ? DocumentDelta::between(before->document(), after->document())
            : DocumentDelta::appendedStroke(
                  before->document(), after->document(), appendedStrokeLayerId);
    if (delta.isEmpty())
    {
        failHistoryMacro();
        return false;
    }

    if (m_macroTransaction)
    {
        m_macroTransaction->workingState = std::move(after);
        m_macroTransaction->effects.append(*effects);
        return true;
    }

    const quint64 beforeNode = m_currentHistoryNode;
    const quint64 beforeRevision = m_currentContentRevision;
    const QVector<Stroke> beforePayload = delta.payloadStrokes(false);
    const QVector<Stroke> afterPayload = delta.payloadStrokes(true);
    DocumentSerializer::ImmutableBackingLease beforeLease =
        DocumentSerializer::retainImmutableBackings(*before, beforePayload);
    DocumentSerializer::ImmutableBackingLease afterLease =
        DocumentSerializer::retainImmutableBackings(*after, afterPayload);
    if (!beforeLease.isValid() || !afterLease.isValid())
    {
        failHistoryMacro();
        return false;
    }
    const quint64 afterNode = m_nextHistoryNode + 1;
    const quint64 afterRevision = m_nextContentRevision + 1;
    const qint64 afterCompactSize = after->compactSize();
    m_nextHistoryNode = afterNode;
    m_nextContentRevision = afterRevision;
    m_undoStack.push(new DocumentCommand(this,
        std::move(text),
        std::move(delta),
        std::move(after),
        activeLayerPolicy,
        std::move(effects),
        std::move(beforeLease),
        std::move(afterLease),
        mergeId,
        mergeScope,
        beforeNode,
        afterNode,
        beforeRevision,
        afterRevision,
        before->compactSize(),
        afterCompactSize));
    return true;
}

DocumentController::PreparedState DocumentController::prepareState(
    Document document,
    const PreparedDocument *base,
    const DocumentSerializer::ImmutableBackingLease *trusted,
    bool historyPreflight,
    QString *error)
{
    if (historyPreflight && m_historyPrepareFailureCountdownForTesting == 0)
    {
        m_historyPrepareFailureCountdownForTesting = -1;
        return {};
    }
    if (historyPreflight && m_historyPrepareFailureCountdownForTesting > 0)
    {
        --m_historyPrepareFailureCountdownForTesting;
    }
    const PreparedDocument *effectiveBase = base;
    if (!effectiveBase && m_currentState)
    {
        effectiveBase = m_currentState.get();
    }
    std::optional<PreparedDocument> prepared =
        DocumentSerializer::prepare(std::move(document),
            m_serializationCache,
            effectiveBase,
            trusted,
            DocumentLimits::maximumProjectBytes,
            error);
    if (!prepared)
    {
        return {};
    }
    return std::make_shared<const PreparedDocument>(std::move(*prepared));
}

void DocumentController::applyPreparedState(const PreparedState &state,
    ActiveLayerPolicy activeLayerPolicy,
    const HistoryEffects &effects,
    CommitDirection direction,
    quint64 historyNode,
    quint64 contentRevision)
{
    Q_ASSERT(state && state->isValid());
    if (!state || !state->isValid())
    {
        return;
    }

    PreparedState appliedState = state;
    if (activeLayerPolicy == ActiveLayerPolicy::PreserveCurrentIfPresent
        && m_currentState)
    {
        const QUuid currentActive = document().activeLayerId;
        const Document &target = state->document();
        const bool canPreserve = target.layers.isEmpty()
                                     ? currentActive.isNull()
                                     : target.layer(currentActive) != nullptr;
        if (canPreserve && target.activeLayerId != currentActive)
        {
            std::optional<PreparedDocument> rebound =
                DocumentSerializer::rebindActiveLayer(*state, currentActive);
            Q_ASSERT(rebound.has_value());
            if (rebound)
            {
                appliedState = std::make_shared<const PreparedDocument>(
                    std::move(*rebound));
            }
        }
    }

    const bool wasModified = isModified();
    // Install the complete observable controller state before any callback.
    // Slots may query or modify the controller synchronously from the
    // command's effect and documentChanged signals.
    m_currentState = std::move(appliedState);
    m_currentHistoryNode = historyNode;
    m_currentContentRevision = contentRevision;
    m_selectionVisibilityCacheValid = false;
    dispatchHistoryEffects(effects, direction, true);
    const bool modified = isModified();
    if (modified != wasModified)
    {
        emit modifiedChanged(modified);
    }
}

bool DocumentController::preflightHistoryMovement(
    const LogicalHistoryCommand *command, bool forward)
{
    if (!command || !m_currentState)
    {
        return false;
    }
    return const_cast<LogicalHistoryCommand *>(command)->preflight(forward);
}

void DocumentController::applyHistoryMovement(
    LogicalHistoryCommand *command, bool forward)
{
    if (!command)
    {
        return;
    }
    emit historyMovementStarting();
    if (forward)
    {
        command->redo();
    }
    else
    {
        command->undo();
    }
}

void DocumentController::clearHistoryPreflight(
    const LogicalHistoryCommand *command)
{
    if (command)
    {
        const_cast<LogicalHistoryCommand *>(command)->clearPreflight();
    }
}

DocumentUndoStack::StorageStats DocumentController::historyStorageStats(
    const LogicalHistoryCommand *command) const
{
    DocumentUndoStack::StorageStats total;
    if (!command)
    {
        return total;
    }
    return command->storageStats();
}

void DocumentController::dispatchHistoryEffects(const HistoryEffects &effects,
    CommitDirection direction,
    bool changedDocument)
{
    const bool forward = direction == CommitDirection::Forward;
    for (const HistoryEffects::BeforeEvent &event :
        effects.beforeDocumentChanged)
    {
        std::visit(
            [this, forward](const auto &value)
            {
                using T = std::decay_t<decltype(value)>;
                if constexpr (std::is_same_v<T, HistoryEffects::CanvasResize>)
                {
                    emit canvasResized(
                        forward ? value.beforeSize : value.afterSize,
                        forward ? value.afterSize : value.beforeSize,
                        forward ? value.forwardTransform
                                : value.reverseTransform);
                }
                else if constexpr (std::is_same_v<T,
                                       HistoryEffects::StrokeTransform>)
                {
                    emit strokesTransformed(value.layerId,
                        value.strokeIds,
                        forward ? value.forwardTransform
                                : value.reverseTransform);
                }
                else if constexpr (std::is_same_v<T,
                                       HistoryEffects::StrokeDuplicate>)
                {
                    emit strokesDuplicated(value.layerId,
                        value.sourceIds,
                        value.duplicateIds,
                        value.delta,
                        forward);
                }
                else if constexpr (std::is_same_v<T,
                                       HistoryEffects::SelectionOverlay>)
                {
                    const auto unpack = [](const auto &mask)
                    {
                        return mask ? unpackBinaryMask(*mask) : QImage();
                    };
                    emit selectionOverlayTransition(value.layerId,
                        forward ? value.beforeIds : value.afterIds,
                        forward ? value.afterIds : value.beforeIds,
                        unpack(forward ? value.beforeMask : value.afterMask),
                        unpack(forward ? value.afterMask : value.beforeMask));
                }
                else if constexpr (std::is_same_v<T,
                                       HistoryEffects::StrokePresence>)
                {
                    emit strokePresenceChanged(value.layerId,
                        value.strokeId,
                        value.clipMask ? unpackBinaryMask(*value.clipMask)
                                       : QImage(),
                        forward);
                }
            },
            event);
    }
    if (changedDocument)
    {
        notifyDocumentChanged();
    }
    for (const HistoryEffects::AfterEvent &event : effects.afterDocumentChanged)
    {
        std::visit(
            [this](const auto &value)
            {
                using T = std::decay_t<decltype(value)>;
                if constexpr (std::is_same_v<T, HistoryEffects::LayerThumbnail>)
                {
                    emit layerThumbnailChanged(value.layerId);
                }
                else if constexpr (std::is_same_v<T,
                                       HistoryEffects::LayerThumbnailsReset>)
                {
                    emit layerThumbnailsReset();
                }
                else if constexpr (std::is_same_v<T,
                                       HistoryEffects::ActiveLayer>)
                {
                    emit activeLayerChanged(document().activeLayerId);
                }
            },
            event);
    }
    if (effects.selectionState)
    {
        const HistoryEffects::SelectionState &state =
            forward ? effects.selectionState->after
                    : effects.selectionState->before;
        emit selectionHistoryStateRequested(state.layerId,
            state.mask ? unpackBinaryMask(*state.mask) : QImage());
    }
}

void DocumentController::beginHistoryMacro(const QString &text)
{
    if (m_undoStack.m_moving || !m_currentState)
    {
        return;
    }
    if (m_macroTransaction)
    {
        ++m_macroTransaction->depth;
        return;
    }
    m_macroTransaction = std::make_unique<MacroTransaction>();
    m_macroTransaction->text = text;
    m_macroTransaction->startState = m_currentState;
    m_macroTransaction->workingState = m_currentState;
}

void DocumentController::endHistoryMacro()
{
    if (!m_macroTransaction || m_undoStack.m_moving)
    {
        return;
    }
    if (--m_macroTransaction->depth > 0)
    {
        return;
    }
    std::unique_ptr<MacroTransaction> transaction =
        std::move(m_macroTransaction);
    if (transaction->failed || !transaction->startState
        || !transaction->workingState)
    {
        return;
    }

    DocumentDelta delta =
        DocumentDelta::between(transaction->startState->document(),
            transaction->workingState->document());
    if (delta.isEmpty())
    {
        transaction->effects.discardDocumentEffects();
        if (!transaction->effects.hasSelectionTransition())
        {
            return;
        }
        auto effects = std::make_shared<const HistoryEffects>(
            transaction->effects.frozenCopy());
        m_undoStack.push(
            new TransientCommand(this, transaction->text, std::move(effects)));
        return;
    }

    const quint64 beforeNode = m_currentHistoryNode;
    const quint64 beforeRevision = m_currentContentRevision;
    const QVector<Stroke> beforePayload = delta.payloadStrokes(false);
    const QVector<Stroke> afterPayload = delta.payloadStrokes(true);
    auto beforeLease = DocumentSerializer::retainImmutableBackings(
        *transaction->startState, beforePayload);
    auto afterLease = DocumentSerializer::retainImmutableBackings(
        *transaction->workingState, afterPayload);
    if (!beforeLease.isValid() || !afterLease.isValid())
    {
        return;
    }
    const quint64 afterNode = m_nextHistoryNode + 1;
    const quint64 afterRevision = m_nextContentRevision + 1;
    m_nextHistoryNode = afterNode;
    m_nextContentRevision = afterRevision;
    auto effects = std::make_shared<const HistoryEffects>(
        transaction->effects.frozenCopy());
    m_undoStack.push(new DocumentCommand(this,
        transaction->text,
        std::move(delta),
        transaction->workingState,
        ActiveLayerPolicy::UsePrepared,
        std::move(effects),
        std::move(beforeLease),
        std::move(afterLease),
        -1,
        {},
        beforeNode,
        afterNode,
        beforeRevision,
        afterRevision,
        transaction->startState->compactSize(),
        transaction->workingState->compactSize()));
}

void DocumentController::failHistoryMacro()
{
    if (m_macroTransaction)
    {
        m_macroTransaction->failed = true;
    }
}

bool DocumentController::rejectHistoryMutation()
{
    failHistoryMacro();
    return false;
}

bool DocumentController::hasOpenHistoryMacro() const
{
    return static_cast<bool>(m_macroTransaction);
}

qsizetype DocumentController::macroPreparedDocumentCount() const
{
    if (!m_macroTransaction)
    {
        return 0;
    }
    return (m_macroTransaction->startState ? 1 : 0)
           + (m_macroTransaction->workingState ? 1 : 0);
}

const DocumentController::PreparedState &
DocumentController::editableState() const
{
    if (m_macroTransaction && m_macroTransaction->workingState)
    {
        return m_macroTransaction->workingState;
    }
    return m_currentState;
}

void DocumentController::normalizeMergedNoOp(
    quint64 historyNode, quint64 contentRevision)
{
    const bool wasModified = isModified();
    m_currentHistoryNode = historyNode;
    m_currentContentRevision = contentRevision;
    const bool modified = isModified();
    if (modified != wasModified)
    {
        emit modifiedChanged(modified);
    }
}

void DocumentController::notifyDocumentChanged()
{
    m_selectionVisibilityCacheValid = false;
    emit documentChanged();
}

void DocumentController::ensureActiveLayer(Document &document)
{
    const Layer *active = document.layer(document.activeLayerId);
    if (active && active->kind == LayerKind::Paint)
    {
        return;
    }
    if (document.layers.isEmpty())
    {
        document.activeLayerId = QUuid();
        return;
    }
    for (auto layer = document.layers.crbegin();
        layer != document.layers.crend();
        ++layer)
    {
        if (layer->kind == LayerKind::Paint)
        {
            document.activeLayerId = layer->id;
            return;
        }
    }
    document.activeLayerId = QUuid();
}

QString DocumentController::nextLayerName() const
{
    const Document &current = document();
    // Groups carry their own numbering, so counting them here made the next
    // paint layer skip a number: Layer 1, Layer 2 and Group 1 produced
    // Layer 4. The loop below still steps past any name already taken.
    int number = static_cast<int>(std::count_if(current.layers.cbegin(),
                     current.layers.cend(),
                     [](const Layer &layer)
                     {
                         return layer.kind == LayerKind::Paint;
                     }))
                 + 1;
    while (true)
    {
        const QString candidate = tr("Layer %1").arg(number);
        bool exists = false;
        for (const Layer &layer : current.layers)
        {
            if (layer.name == candidate)
            {
                exists = true;
                break;
            }
        }
        if (!exists)
        {
            return candidate;
        }
        ++number;
    }
}

}

// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#pragma once

#include "app/RecoveryStore.hpp"
#include "brush/BrushPreset.hpp"
#include "document/Document.hpp"
#include "document/DocumentController.hpp"
#include "document/DocumentLimits.hpp"
#include "document/LayerHierarchy.hpp"
#include "document/SelectionOperation.hpp"
#include "io/DocumentSerializer.hpp"
#include "render/RenderEngine.hpp"
#include "support/DocumentControllerTestAccess.hpp"

#include <QAction>
#include <QElapsedTimer>
#include <QFile>
#include <QFileInfo>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QSaveFile>
#include <QScopeGuard>
#include <QTemporaryDir>
#include <QtTest>

#include <algorithm>
#include <limits>
#include <memory>
#include <new>

namespace ugurugu
{

inline QByteArray pointArray(int count)
{
    QByteArray points = QByteArrayLiteral("[0,0],").repeated(count);
    if (!points.isEmpty())
    {
        points.chop(1);
    }
    points.prepend('[');
    points.append(']');
    return points;
}

struct DocumentTransitionSnapshot
{
    QByteArray document;
    const void *stateIdentity = nullptr;
    DocumentUndoStack::StorageStats historyStorage;
    int historyCount = 0;
    int historyIndex = 0;
    int historyLimit = 0;
    bool canUndo = false;
    bool canRedo = false;
    bool historyClean = false;
    bool modified = false;
    quint64 historyNode = 0;
    quint64 nextHistoryNode = 0;
    quint64 contentRevision = 0;
    quint64 savedContentRevision = 0;
    quint64 nextContentRevision = 0;
};

inline DocumentTransitionSnapshot documentTransitionSnapshot(
    DocumentController &controller)
{
    DocumentTransitionSnapshot snapshot;
    snapshot.document = DocumentSerializer::toJson(controller.document());
    snapshot.stateIdentity =
        DocumentControllerTestAccess::stateIdentity(controller);
    snapshot.historyStorage = controller.undoStack()->storageStats();
    snapshot.historyCount = controller.undoStack()->count();
    snapshot.historyIndex = controller.undoStack()->index();
    snapshot.historyLimit = controller.undoStack()->undoLimit();
    snapshot.canUndo = controller.undoStack()->canUndo();
    snapshot.canRedo = controller.undoStack()->canRedo();
    snapshot.historyClean = controller.undoStack()->isClean();
    snapshot.modified = controller.isModified();
    snapshot.historyNode =
        DocumentControllerTestAccess::historyNode(controller);
    snapshot.nextHistoryNode =
        DocumentControllerTestAccess::nextHistoryNode(controller);
    snapshot.contentRevision =
        DocumentControllerTestAccess::contentRevision(controller);
    snapshot.savedContentRevision =
        DocumentControllerTestAccess::savedContentRevision(controller);
    snapshot.nextContentRevision =
        DocumentControllerTestAccess::nextContentRevision(controller);
    return snapshot;
}

inline void compareDocumentTransitionSnapshot(
    DocumentController &controller, const DocumentTransitionSnapshot &expected)
{
    QCOMPARE(
        DocumentSerializer::toJson(controller.document()), expected.document);
    QCOMPARE(DocumentControllerTestAccess::stateIdentity(controller),
        expected.stateIdentity);
    const DocumentUndoStack::StorageStats actualStorage =
        controller.undoStack()->storageStats();
    QCOMPARE(
        actualStorage.retainedLayers, expected.historyStorage.retainedLayers);
    QCOMPARE(
        actualStorage.retainedStrokes, expected.historyStorage.retainedStrokes);
    QCOMPARE(actualStorage.retainedPreparedDocuments,
        expected.historyStorage.retainedPreparedDocuments);
    QCOMPARE(actualStorage.entryCount, expected.historyStorage.entryCount);
    QCOMPARE(actualStorage.stagedPreparedDocuments,
        expected.historyStorage.stagedPreparedDocuments);
    QCOMPARE(actualStorage.peakTransientPreparedDocuments,
        expected.historyStorage.peakTransientPreparedDocuments);
    QCOMPARE(actualStorage.macroPreparedDocuments,
        expected.historyStorage.macroPreparedDocuments);
    QCOMPARE(
        actualStorage.retainedBytes, expected.historyStorage.retainedBytes);
    QCOMPARE(actualStorage.residentBudgetSoftExceeded,
        expected.historyStorage.residentBudgetSoftExceeded);
    QCOMPARE(controller.undoStack()->count(), expected.historyCount);
    QCOMPARE(controller.undoStack()->index(), expected.historyIndex);
    QCOMPARE(controller.undoStack()->undoLimit(), expected.historyLimit);
    QCOMPARE(controller.undoStack()->canUndo(), expected.canUndo);
    QCOMPARE(controller.undoStack()->canRedo(), expected.canRedo);
    QCOMPARE(controller.undoStack()->isClean(), expected.historyClean);
    QCOMPARE(controller.isModified(), expected.modified);
    QCOMPARE(DocumentControllerTestAccess::historyNode(controller),
        expected.historyNode);
    QCOMPARE(DocumentControllerTestAccess::nextHistoryNode(controller),
        expected.nextHistoryNode);
    QCOMPARE(DocumentControllerTestAccess::contentRevision(controller),
        expected.contentRevision);
    QCOMPARE(DocumentControllerTestAccess::savedContentRevision(controller),
        expected.savedContentRevision);
    QCOMPARE(DocumentControllerTestAccess::nextContentRevision(controller),
        expected.nextContentRevision);
}

inline void prepareDocumentTransitionFailureState(
    DocumentController &controller)
{
    controller.newDocument(QSize(96, 96));
    const QUuid layerId = controller.document().activeLayerId;
    controller.renameLayer(layerId, QStringLiteral("Saved layer"));
    controller.markSaved();
    controller.setLayerOpacity(layerId, 0.5);
    controller.setLayerVisible(layerId, false);
    controller.undoStack()->undo();
}

template <typename Transition>
inline void verifyDocumentTransitionPreparationFailure(
    DocumentController &controller, Transition transition)
{
    const DocumentTransitionSnapshot before =
        documentTransitionSnapshot(controller);
    QSignalSpy documentReplacedSpy(
        &controller, &DocumentController::documentReplaced);
    QSignalSpy documentChangedSpy(
        &controller, &DocumentController::documentChanged);
    QSignalSpy thumbnailsResetSpy(
        &controller, &DocumentController::layerThumbnailsReset);
    QSignalSpy activeLayerChangedSpy(
        &controller, &DocumentController::activeLayerChanged);
    QSignalSpy modifiedChangedSpy(
        &controller, &DocumentController::modifiedChanged);

    DocumentControllerTestAccess::failNextDocumentReplacementPreparation(
        controller);
    QString error;
    QVERIFY(!transition(&error));
    QVERIFY(!error.isEmpty());
    compareDocumentTransitionSnapshot(controller, before);
    QCOMPARE(documentReplacedSpy.count(), 0);
    QCOMPARE(documentChangedSpy.count(), 0);
    QCOMPARE(thumbnailsResetSpy.count(), 0);
    QCOMPARE(activeLayerChangedSpy.count(), 0);
    QCOMPARE(modifiedChangedSpy.count(), 0);

    const QUuid layerId = controller.document().activeLayerId;
    controller.undoStack()->redo();
    QVERIFY(!controller.document().layer(layerId)->visible);
    controller.undoStack()->undo();
    QVERIFY(controller.document().layer(layerId)->visible);
    QCOMPARE(
        DocumentSerializer::toJson(controller.document()), before.document);
    QCOMPARE(controller.undoStack()->index(), before.historyIndex);
    QCOMPARE(controller.isModified(), before.modified);
}

inline Document documentWithStrokeCount(int count)
{
    Document document = Document::createDefault(QSize(64, 64));
    Layer &layer = document.layers.first();
    layer.strokes.reserve(count);
    for (int index = 0; index < count; ++index)
    {
        Stroke stroke;
        stroke.seed = static_cast<quint64>(index) + 1;
        stroke.points = {{QPointF(1.0 + static_cast<qreal>(index % 62),
                              1.0 + static_cast<qreal>((index / 62) % 62)),
            1.0}};
        layer.strokes.append(std::move(stroke));
    }
    return document;
}

inline Document documentWithNestedPaintAtDepth(int depth)
{
    Document document = Document::createDefault(QSize(64, 64));
    QUuid parentId;
    for (int index = 0; index < depth; ++index)
    {
        Layer group;
        group.name = QStringLiteral("Group %1").arg(index + 1);
        group.kind = LayerKind::Group;
        group.parentGroupId = parentId;
        group.initialCanvasSize = document.size;
        parentId = group.id;
        document.layers.append(std::move(group));
    }
    document.layers.first().parentGroupId = parentId;
    return document;
}

inline Document documentWithGroupAtDepth(int depth)
{
    Document document = Document::createDefault(QSize(64, 64));
    QUuid parentId;
    for (int index = 0; index <= depth; ++index)
    {
        Layer group;
        group.name = QStringLiteral("Group %1").arg(index + 1);
        group.kind = LayerKind::Group;
        group.parentGroupId = parentId;
        group.initialCanvasSize = document.size;
        parentId = group.id;
        document.layers.append(std::move(group));
    }
    return document;
}

inline QUuid layerIdAtDepth(const Document &document, int depth, LayerKind kind)
{
    const LayerHierarchyAnalysis hierarchy = analyzeLayerHierarchy(document);
    if (!hierarchy.isValid())
    {
        return {};
    }
    for (int index = 0; index < document.layers.size(); ++index)
    {
        if (document.layers[index].kind == kind
            && hierarchy.depths()[index] == depth)
        {
            return document.layers[index].id;
        }
    }
    return {};
}

template <typename Mutation>
inline void verifyRejectedHierarchyMutation(
    DocumentController &controller, Mutation mutation)
{
    const DocumentTransitionSnapshot before =
        documentTransitionSnapshot(controller);
    QSignalSpy documentChangedSpy(
        &controller, &DocumentController::documentChanged);
    QSignalSpy activeLayerChangedSpy(
        &controller, &DocumentController::activeLayerChanged);
    QSignalSpy modifiedChangedSpy(
        &controller, &DocumentController::modifiedChanged);
    QSignalSpy thumbnailsResetSpy(
        &controller, &DocumentController::layerThumbnailsReset);

    mutation();

    compareDocumentTransitionSnapshot(controller, before);
    QCOMPARE(documentChangedSpy.count(), 0);
    QCOMPARE(activeLayerChangedSpy.count(), 0);
    QCOMPARE(modifiedChangedSpy.count(), 0);
    QCOMPARE(thumbnailsResetSpy.count(), 0);
}

}

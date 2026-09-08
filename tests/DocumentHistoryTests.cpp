// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "document/history/DocumentDelta.hpp"
#include "support/DocumentTestHelpers.hpp"
#include "support/DocumentTestSuites.hpp"

namespace ugurugu
{

class DocumentHistoryTests final : public QObject
{
    Q_OBJECT

private slots:
    void undoesAndRedoesStrokeWithCleanState()
    {
        DocumentController controller;
        controller.newDocument(QSize(320, 240));
        const QUuid layerId = controller.document().activeLayerId;

        QVERIFY(!controller.isModified());
        QVERIFY(controller.undoStack()->isClean());

        Stroke stroke;
        stroke.id =
            QUuid(QStringLiteral("{11111111-1111-1111-1111-111111111111}"));
        stroke.seed = 42;
        stroke.color = QColor(20, 40, 60);
        stroke.width = 8.0;
        stroke.points = {
            {QPointF(10.0, 12.0), 0.5}, {QPointF(40.0, 52.0), 1.0}};

        controller.addStroke(layerId, stroke);
        QCOMPARE(controller.document().layer(layerId)->strokes.size(), 1);
        QCOMPARE(controller.document().layer(layerId)->strokes.first().id,
            stroke.id);
        QVERIFY(controller.isModified());
        QVERIFY(controller.undoStack()->canUndo());

        controller.undoStack()->undo();
        QVERIFY(controller.document().layer(layerId)->strokes.isEmpty());
        QVERIFY(!controller.isModified());
        QVERIFY(controller.undoStack()->isClean());

        controller.undoStack()->redo();
        QCOMPARE(controller.document().layer(layerId)->strokes.size(), 1);
        QVERIFY(controller.isModified());

        controller.markSaved();
        QVERIFY(!controller.isModified());
        QVERIFY(controller.undoStack()->isClean());

        controller.undoStack()->undo();
        QVERIFY(controller.document().layer(layerId)->strokes.isEmpty());
        QVERIFY(controller.isModified());

        controller.undoStack()->redo();
        QCOMPARE(controller.document().layer(layerId)->strokes.size(), 1);
        QVERIFY(!controller.isModified());
        QVERIFY(controller.undoStack()->isClean());
    }

    void installsTargetStateBeforeHistoryEffectSignals()
    {
        DocumentController controller;
        QVERIFY(controller.newDocument(QSize(100, 100)));
        QVERIFY(controller.resizeCanvas(QSize(130, 100), QPoint(5, 0)));
        QCOMPARE(controller.document().size, QSize(130, 100));

        QSize documentSizeAtEffect;
        bool documentChangedAfterEffect = false;
        bool effectSeen = false;
        connect(&controller,
            &DocumentController::canvasResized,
            &controller,
            [&controller, &documentSizeAtEffect, &effectSeen](
                const QSize &, const QSize &, const QTransform &)
            {
                documentSizeAtEffect = controller.document().size;
                effectSeen = true;
            });
        connect(&controller,
            &DocumentController::documentChanged,
            &controller,
            [&documentChangedAfterEffect, &effectSeen]()
            {
                documentChangedAfterEffect = effectSeen;
            });

        controller.undoStack()->undo();
        QVERIFY(effectSeen);
        QCOMPARE(documentSizeAtEffect, QSize(100, 100));
        QVERIFY(documentChangedAfterEffect);
    }

    void reentrantHistoryMovementPublishesTheStateItLandedOn()
    {
        DocumentController controller;
        QVERIFY(controller.newDocument(QSize(64, 64)));
        const QUuid layerId = controller.document().activeLayerId;
        for (int index = 0; index < 2; ++index)
        {
            Stroke stroke;
            stroke.width = 4.0;
            stroke.points = {{QPointF(8.0 + index, 8.0), 1.0},
                {QPointF(40.0 + index, 40.0), 1.0}};
            QCOMPARE(controller.addStroke(layerId, std::move(stroke)),
                DocumentController::AddStrokeResult::Added);
        }
        DocumentUndoStack *stack = controller.undoStack();
        QCOMPARE(stack->count(), 2);

        // Moving history again from inside a history signal is what a shortcut
        // held down through a slot does; every payload published afterwards
        // has to describe where history actually ended up.
        QString lastUndoText;
        QString lastRedoText;
        bool reentered = false;
        connect(stack,
            &DocumentUndoStack::undoTextChanged,
            stack,
            [&lastUndoText](const QString &text)
            {
                lastUndoText = text;
            });
        connect(stack,
            &DocumentUndoStack::redoTextChanged,
            stack,
            [&lastRedoText](const QString &text)
            {
                lastRedoText = text;
            });
        connect(stack,
            &DocumentUndoStack::canRedoChanged,
            stack,
            [stack, &reentered](bool)
            {
                if (!std::exchange(reentered, true))
                {
                    stack->undo();
                }
            });

        stack->undo();
        QVERIFY(reentered);
        QCOMPARE(stack->index(), 0);
        QCOMPARE(lastUndoText, stack->undoText());
        QCOMPARE(lastRedoText, stack->redoText());
    }

    void undoesAndRedoesTransparentBackground()
    {
        DocumentController controller;
        controller.newDocument(QSize(64, 48));
        const QColor original = controller.document().background;
        QVERIFY(original.alpha() == 255);

        controller.setBackground(QColor(0, 0, 0, 0));
        QCOMPARE(controller.document().background.alpha(), 0);
        QVERIFY(controller.isModified());
        QVERIFY(controller.undoStack()->canUndo());

        controller.undoStack()->undo();
        QCOMPARE(controller.document().background, original);

        controller.undoStack()->redo();
        QCOMPARE(controller.document().background.alpha(), 0);
    }

    void rejectsBackgroundChangeThatChangesNothing()
    {
        DocumentController controller;
        controller.newDocument(QSize(64, 48));
        const int entries = controller.undoStack()->count();

        controller.setBackground(controller.document().background);
        QCOMPARE(controller.undoStack()->count(), entries);

        controller.setBackground(QColor());
        QCOMPARE(controller.undoStack()->count(), entries);
    }

    void transientCommandsDoNotModifyDocument()
    {
        DocumentController controller;
        controller.newDocument(QSize(320, 240));
        int transientState = 0;
        QObject::connect(&controller,
            &DocumentController::selectionHistoryStateRequested,
            &controller,
            [&transientState](const QUuid &, const QImage &mask)
            {
                transientState = mask.isNull() ? 0 : mask.width();
            });

        QImage firstState(1, 1, QImage::Format_Grayscale8);
        firstState.fill(255);

        controller.pushSelectionStateCommand(
            QStringLiteral("Transient"), {}, {}, {}, firstState);
        QCOMPARE(transientState, 1);
        QVERIFY(!controller.isModified());

        controller.undoStack()->undo();
        QCOMPARE(transientState, 0);
        QVERIFY(!controller.isModified());

        controller.undoStack()->redo();
        QCOMPARE(transientState, 1);
        QVERIFY(!controller.isModified());

        const QUuid layerId = controller.document().activeLayerId;
        Stroke stroke;
        stroke.points = {
            {QPointF(10.0, 12.0), 1.0}, {QPointF(40.0, 52.0), 1.0}};
        controller.addStroke(layerId, stroke);
        QVERIFY(controller.isModified());
        controller.markSaved();
        QVERIFY(!controller.isModified());

        QImage secondState(2, 1, QImage::Format_Grayscale8);
        secondState.fill(255);
        controller.pushSelectionStateCommand(
            QStringLiteral("Transient 2"), {}, firstState, {}, secondState);
        QCOMPARE(transientState, 2);
        QVERIFY(!controller.isModified());

        controller.undoStack()->undo();
        QCOMPARE(transientState, 1);
        QVERIFY(!controller.isModified());
        controller.undoStack()->undo();
        QVERIFY(controller.isModified());
        controller.undoStack()->redo();
        QVERIFY(!controller.isModified());
    }

    void limitsUndoHistory()
    {
        DocumentController controller;
        int value = 0;
        QObject::connect(&controller,
            &DocumentController::selectionHistoryStateRequested,
            &controller,
            [&value](const QUuid &, const QImage &mask)
            {
                value = mask.isNull() ? 0 : mask.width();
            });
        for (int index = 1; index <= 80; ++index)
        {
            QImage before;
            if (index > 1)
            {
                before = QImage(index - 1, 1, QImage::Format_Grayscale8);
                before.fill(255);
            }
            QImage after(index, 1, QImage::Format_Grayscale8);
            after.fill(255);
            controller.pushSelectionStateCommand(
                QStringLiteral("Transient"), {}, before, {}, after);
        }

        QCOMPARE(value, 80);
        QCOMPARE(controller.undoStack()->undoLimit(), 64);
        QCOMPARE(controller.undoStack()->count(), 64);
    }

    void removesMergedScalarRoundTripFromHistory()
    {
        DocumentController controller;
        controller.newDocument(QSize(96, 96));
        const qreal original = controller.document().wobbleAmount;
        const quint64 originalRevision =
            DocumentControllerTestAccess::contentRevision(controller);
        const quint64 originalNode =
            DocumentControllerTestAccess::historyNode(controller);

        controller.setWobbleAmount(original + 1.0);
        QCOMPARE(controller.undoStack()->count(), 1);
        QVERIFY(controller.isModified());

        controller.setWobbleAmount(original);
        QCOMPARE(controller.undoStack()->count(), 0);
        QCOMPARE(controller.undoStack()->index(), 0);
        QVERIFY(!controller.undoStack()->canUndo());
        QVERIFY(!controller.undoStack()->canRedo());
        QVERIFY(controller.undoStack()->isClean());
        QVERIFY(!controller.isModified());
        QCOMPARE(DocumentControllerTestAccess::contentRevision(controller),
            originalRevision);
        QCOMPARE(DocumentControllerTestAccess::historyNode(controller),
            originalNode);
    }

    void discardsFailedMacroWithoutUndoingEarlierHistory()
    {
        DocumentController controller;
        controller.newDocument(QSize(96, 96));
        const QUuid layerId = controller.document().activeLayerId;
        controller.setWobbleAmount(2.0);
        const int beforeCount = controller.undoStack()->count();
        const int beforeIndex = controller.undoStack()->index();
        const quint64 beforeRevision =
            DocumentControllerTestAccess::contentRevision(controller);
        const quint64 beforeNode =
            DocumentControllerTestAccess::historyNode(controller);
        QSignalSpy documentChangedSpy(
            &controller, &DocumentController::documentChanged);

        controller.undoStack()->beginMacro(
            QStringLiteral("Rejected canvas transaction"));
        controller.renameLayer(layerId, QStringLiteral("Temporary"));
        QVERIFY(!controller.resizeCanvas(QSize(96, 96), QPoint()));
        controller.undoStack()->endMacro();

        QCOMPARE(controller.undoStack()->count(), beforeCount);
        QCOMPARE(controller.undoStack()->index(), beforeIndex);
        QCOMPARE(controller.document().layer(layerId)->name,
            QStringLiteral("Layer 1"));
        QCOMPARE(controller.document().wobbleAmount, 2.0);
        QCOMPARE(documentChangedSpy.count(), 0);
        QCOMPARE(DocumentControllerTestAccess::contentRevision(controller),
            beforeRevision);
        QCOMPARE(
            DocumentControllerTestAccess::historyNode(controller), beforeNode);

        controller.undoStack()->undo();
        QCOMPARE(controller.document().wobbleAmount,
            Document::createDefault(QSize(96, 96)).wobbleAmount);
    }

    void publishesNestedMacroOnce()
    {
        DocumentController controller;
        controller.newDocument(QSize(96, 96));
        const QUuid layerId = controller.document().activeLayerId;
        QSignalSpy documentChangedSpy(
            &controller, &DocumentController::documentChanged);

        controller.undoStack()->beginMacro(QStringLiteral("Outer"));
        controller.renameLayer(layerId, QStringLiteral("Nested"));
        controller.undoStack()->beginMacro(QStringLiteral("Inner"));
        controller.setLayerVisible(layerId, false);
        controller.undoStack()->endMacro();
        QCOMPARE(controller.undoStack()->count(), 0);
        QCOMPARE(documentChangedSpy.count(), 0);
        controller.undoStack()->endMacro();

        QCOMPARE(controller.undoStack()->count(), 1);
        QCOMPARE(controller.undoStack()->index(), 1);
        QCOMPARE(documentChangedSpy.count(), 1);
        QCOMPARE(controller.document().layer(layerId)->name,
            QStringLiteral("Nested"));
        QVERIFY(!controller.document().layer(layerId)->visible);

        controller.undoStack()->undo();
        QCOMPARE(controller.document().layer(layerId)->name,
            QStringLiteral("Layer 1"));
        QVERIFY(controller.document().layer(layerId)->visible);
    }

    void appendsMultipleStrokesIncrementallyInOneMacro()
    {
        DocumentController controller;
        controller.newDocument(QSize(96, 96));
        const QUuid layerId = controller.document().activeLayerId;
        DocumentControllerTestAccess::resetSerializationStats(controller);

        QVector<QUuid> strokeIds;
        controller.undoStack()->beginMacro(QStringLiteral("Stroke batch"));
        for (int index = 0; index < 4; ++index)
        {
            Stroke stroke;
            stroke.seed = static_cast<quint64>(index) + 1;
            stroke.points = {{QPointF(8.0 + index, 12.0 + index), 1.0},
                {QPointF(48.0 + index, 52.0 + index), 0.75}};
            strokeIds.append(stroke.id);
            QCOMPARE(controller.addStroke(layerId, std::move(stroke)),
                DocumentController::AddStrokeResult::Added);
        }
        QCOMPARE(controller.undoStack()->count(), 0);
        controller.undoStack()->endMacro();

        QCOMPARE(controller.undoStack()->count(), 1);
        QCOMPARE(controller.undoStack()->index(), 1);
        const Layer *appendedLayer = controller.document().layer(layerId);
        QVERIFY(appendedLayer);
        QCOMPARE(appendedLayer->strokes.size(), strokeIds.size());
        for (qsizetype index = 0; index < strokeIds.size(); ++index)
        {
            QCOMPARE(appendedLayer->strokes.at(index).id, strokeIds.at(index));
        }
        const auto appendedStats =
            DocumentControllerTestAccess::serializationStats(controller);
        QCOMPARE(appendedStats.incrementalStrokeAppends, 4ULL);
        QCOMPARE(appendedStats.fullDocumentPreparations, 0ULL);
        QCOMPARE(appendedStats.strokeSerializations, 4ULL);
        QCOMPARE(appendedStats.clipMaskContentHashes, 0ULL);
        QCOMPARE(appendedStats.binaryMaskContentHashes, 0ULL);

        controller.undoStack()->undo();
        QCOMPARE(controller.undoStack()->index(), 0);
        QCOMPARE(controller.document().layer(layerId)->strokes.size(), 0);
        controller.undoStack()->redo();
        QCOMPARE(controller.undoStack()->index(), 1);
        appendedLayer = controller.document().layer(layerId);
        QVERIFY(appendedLayer);
        QCOMPARE(appendedLayer->strokes.size(), strokeIds.size());
        for (qsizetype index = 0; index < strokeIds.size(); ++index)
        {
            QCOMPARE(appendedLayer->strokes.at(index).id, strokeIds.at(index));
        }
        const auto replayStats =
            DocumentControllerTestAccess::serializationStats(controller);
        QCOMPARE(replayStats.incrementalStrokeAppends,
            appendedStats.incrementalStrokeAppends);
        QCOMPARE(replayStats.fullDocumentPreparations, 2ULL);
        QCOMPARE(replayStats.strokeSerializations,
            appendedStats.strokeSerializations
                + static_cast<quint64>(strokeIds.size()));
    }

    void rollsBackIncrementalStrokeMacroAfterSecondAppendFails()
    {
        DocumentController controller;
        controller.newDocument(QSize(96, 96));
        const QUuid layerId = controller.document().activeLayerId;
        controller.setWobbleAmount(2.0);
        const QByteArray beforeJson =
            DocumentSerializer::toJson(controller.document());
        QVERIFY(!beforeJson.isEmpty());
        const int beforeCount = controller.undoStack()->count();
        const int beforeIndex = controller.undoStack()->index();
        const quint64 beforeRevision =
            DocumentControllerTestAccess::contentRevision(controller);
        const quint64 beforeNode =
            DocumentControllerTestAccess::historyNode(controller);
        QSignalSpy documentChangedSpy(
            &controller, &DocumentController::documentChanged);
        DocumentControllerTestAccess::resetSerializationStats(controller);

        Stroke first;
        first.points = {{QPointF(8.0, 12.0), 1.0}, {QPointF(48.0, 52.0), 0.75}};
        const QUuid duplicateId = first.id;
        Stroke duplicate;
        duplicate.id = duplicateId;
        duplicate.points = {
            {QPointF(16.0, 20.0), 0.5}, {QPointF(56.0, 60.0), 1.0}};

        controller.undoStack()->beginMacro(
            QStringLiteral("Rejected stroke batch"));
        QCOMPARE(controller.addStroke(layerId, std::move(first)),
            DocumentController::AddStrokeResult::Added);
        QCOMPARE(controller.document().layer(layerId)->strokes.size(), 1);
        QCOMPARE(controller.addStroke(layerId, std::move(duplicate)),
            DocumentController::AddStrokeResult::RejectedInvalidStroke);
        controller.undoStack()->endMacro();

        QCOMPARE(controller.undoStack()->count(), beforeCount);
        QCOMPARE(controller.undoStack()->index(), beforeIndex);
        QCOMPARE(controller.document().layer(layerId)->strokes.size(), 0);
        QCOMPARE(DocumentSerializer::toJson(controller.document()), beforeJson);
        QCOMPARE(documentChangedSpy.count(), 0);
        QCOMPARE(DocumentControllerTestAccess::contentRevision(controller),
            beforeRevision);
        QCOMPARE(
            DocumentControllerTestAccess::historyNode(controller), beforeNode);
        const auto stats =
            DocumentControllerTestAccess::serializationStats(controller);
        QCOMPARE(stats.incrementalStrokeAppends, 1ULL);
        QCOMPARE(stats.fullDocumentPreparations, 0ULL);
        QCOMPARE(stats.strokeSerializations, 1ULL);
        QCOMPARE(stats.clipMaskContentHashes, 0ULL);
        QCOMPARE(stats.clipMaskCompressions, 0ULL);

        controller.undoStack()->undo();
        QCOMPARE(controller.document().wobbleAmount,
            Document::createDefault(QSize(96, 96)).wobbleAmount);
    }

    void evictsFarthestHistoryAndMaintainsActions()
    {
        DocumentController controller;
        controller.newDocument(QSize(96, 96));
        const QUuid layerId = controller.document().activeLayerId;
        controller.undoStack()->setUndoLimit(3);
        QSignalSpy undoAvailability(
            controller.undoStack(), &DocumentUndoStack::canUndoChanged);
        QSignalSpy redoAvailability(
            controller.undoStack(), &DocumentUndoStack::canRedoChanged);
        QVERIFY(!controller.undoStack()->canUndo());
        QVERIFY(!controller.undoStack()->canRedo());

        for (int index = 0; index < 4; ++index)
        {
            controller.setLayerVisible(layerId, (index % 2) != 0);
        }
        QCOMPARE(controller.undoStack()->count(), 3);
        QCOMPARE(controller.undoStack()->index(), 3);
        QVERIFY(controller.undoStack()->canUndo());
        QVERIFY(!controller.undoStack()->canRedo());
        QVERIFY(!undoAvailability.isEmpty());

        while (controller.undoStack()->canUndo())
        {
            controller.undoStack()->undo();
        }
        QCOMPARE(controller.undoStack()->index(), 0);
        QVERIFY(!controller.undoStack()->isClean());
        QVERIFY(!controller.undoStack()->canUndo());
        QVERIFY(controller.undoStack()->canRedo());
        QVERIFY(!redoAvailability.isEmpty());

        controller.undoStack()->redo();
        QVERIFY(controller.undoStack()->canRedo());
        controller.renameLayer(layerId, QStringLiteral("New branch"));
        QVERIFY(!controller.undoStack()->canRedo());
    }

    void evictsRedoTailWhenLimitShrinksNearCursor()
    {
        DocumentController controller;
        controller.newDocument(QSize(96, 96));
        const QUuid layerId = controller.document().activeLayerId;

        for (int index = 0; index < 6; ++index)
        {
            controller.setLayerVisible(layerId, (index % 2) != 0);
        }
        controller.markSaved();
        QVERIFY(controller.undoStack()->isClean());
        QCOMPARE(controller.undoStack()->count(), 6);

        for (int index = 0; index < 4; ++index)
        {
            controller.undoStack()->undo();
        }
        QCOMPARE(controller.undoStack()->index(), 2);
        QVERIFY(controller.document().layer(layerId)->visible);

        controller.undoStack()->setUndoLimit(3);

        QCOMPARE(controller.undoStack()->count(), 3);
        QCOMPARE(controller.undoStack()->index(), 1);
        QVERIFY(!controller.undoStack()->isClean());
        QVERIFY(controller.undoStack()->canUndo());
        QVERIFY(controller.undoStack()->canRedo());
        QVERIFY(controller.document().layer(layerId)->visible);

        controller.undoStack()->undo();
        QVERIFY(!controller.document().layer(layerId)->visible);
        QVERIFY(!controller.undoStack()->canUndo());

        controller.undoStack()->redo();
        controller.undoStack()->redo();
        controller.undoStack()->redo();
        QVERIFY(!controller.undoStack()->canRedo());
        QVERIFY(controller.document().layer(layerId)->visible);
        QVERIFY(!controller.undoStack()->isClean());
        QVERIFY(controller.isModified());
    }

    void invalidatesCleanMarkerInsideDeletedRedoTail()
    {
        DocumentController controller;
        controller.newDocument(QSize(96, 96));
        const QUuid layerId = controller.document().activeLayerId;

        controller.renameLayer(layerId, QStringLiteral("First"));
        controller.setLayerVisible(layerId, false);
        controller.markSaved();
        QVERIFY(controller.undoStack()->isClean());
        QVERIFY(!controller.isModified());
        QCOMPARE(controller.undoStack()->count(), 2);

        controller.undoStack()->undo();
        QVERIFY(!controller.undoStack()->isClean());
        QVERIFY(controller.isModified());

        controller.setLayerOpacity(layerId, 0.5);
        QCOMPARE(controller.undoStack()->count(), 2);
        QCOMPARE(controller.undoStack()->index(), 2);
        QVERIFY(!controller.undoStack()->canRedo());
        QVERIFY(!controller.undoStack()->isClean());
        QVERIFY(controller.isModified());

        controller.undoStack()->undo();
        QVERIFY(!controller.undoStack()->isClean());
        QVERIFY(controller.isModified());
        controller.undoStack()->undo();
        QCOMPARE(controller.document().layer(layerId)->name,
            QStringLiteral("Layer 1"));
        QVERIFY(!controller.undoStack()->isClean());
        QVERIFY(controller.isModified());
        controller.undoStack()->redo();
        controller.undoStack()->redo();
        QCOMPARE(controller.document().layer(layerId)->name,
            QStringLiteral("First"));
        QCOMPARE(controller.document().layer(layerId)->opacity, 0.5);
        QVERIFY(!controller.undoStack()->isClean());
        QVERIFY(controller.isModified());

        controller.markSaved();
        QVERIFY(controller.undoStack()->isClean());
        QVERIFY(!controller.isModified());
        controller.undoStack()->undo();
        QVERIFY(controller.isModified());
        controller.undoStack()->redo();
        QVERIFY(controller.undoStack()->isClean());
        QVERIFY(!controller.isModified());
    }

    void reusesTrustedBackingsWhenRestoringClearedAndRemovedLayers()
    {
        Document source = Document::createDefault(QSize(64, 64));
        Stroke stroke;
        stroke.points = {
            {QPointF(10.0, 10.0), 1.0}, {QPointF(20.0, 20.0), 1.0}};
        stroke.clipMask = QImage(source.size, QImage::Format_Grayscale8);
        stroke.clipMask.fill(0);
        for (int y = 8; y < 24; ++y)
        {
            std::fill_n(stroke.clipMask.scanLine(y) + 8, 16, 255);
        }
        source.layers.first().strokes.append(stroke);

        QImage selection(source.size, QImage::Format_Grayscale8);
        selection.fill(0);
        for (int y = 16; y < 32; ++y)
        {
            std::fill_n(selection.scanLine(y) + 16, 8, 255);
        }
        const std::optional<PixelSelectionOp> selectionOp =
            makePixelSelectionOp(
                selection, QTransform::fromTranslate(4.0, 2.0), true, true);
        QVERIFY(selectionOp.has_value());
        Stroke selectionStroke;
        selectionStroke.mode = StrokeMode::PixelSelection;
        selectionStroke.pixelSelectionOp = *selectionOp;
        source.layers.first().strokes.append(selectionStroke);

        DocumentController controller;
        controller.loadDocument(source);
        const QUuid layerId = controller.document().activeLayerId;
        const Stroke &prepared =
            controller.document().layer(layerId)->strokes.first();
        const qint64 maskKey = prepared.clipMask.cacheKey();
        const StrokePoint *pointBacking = prepared.points.constData();
        const Stroke &preparedSelection =
            controller.document().layer(layerId)->strokes.last();
        QVERIFY(preparedSelection.pixelSelectionOp.has_value());
        const char *selectionBacking =
            preparedSelection.pixelSelectionOp->packedMask.constData();

        controller.clearLayer(layerId);
        controller.undoStack()->undo();
        const Stroke &clearRestored =
            controller.document().layer(layerId)->strokes.first();
        QCOMPARE(clearRestored.clipMask.cacheKey(), maskKey);
        QCOMPARE(clearRestored.points.constData(), pointBacking);
        const Stroke &clearRestoredSelection =
            controller.document().layer(layerId)->strokes.last();
        QVERIFY(clearRestoredSelection.pixelSelectionOp.has_value());
        QCOMPARE(static_cast<const void *>(clearRestoredSelection
                         .pixelSelectionOp->packedMask.constData()),
            static_cast<const void *>(selectionBacking));

        controller.loadDocument(source);
        const Stroke &removePrepared =
            controller.document().layer(layerId)->strokes.first();
        const qint64 removeMaskKey = removePrepared.clipMask.cacheKey();
        const StrokePoint *removePointBacking =
            removePrepared.points.constData();
        QVERIFY(controller.document()
                .layer(layerId)
                ->strokes.last()
                .pixelSelectionOp.has_value());
        const char *removeSelectionBacking =
            controller.document()
                .layer(layerId)
                ->strokes.last()
                .pixelSelectionOp->packedMask.constData();
        controller.removeLayer(layerId);
        QVERIFY(controller.document().layers.isEmpty());
        controller.undoStack()->undo();
        const Stroke &removeRestored =
            controller.document().layer(layerId)->strokes.first();
        QCOMPARE(removeRestored.clipMask.cacheKey(), removeMaskKey);
        QCOMPARE(removeRestored.points.constData(), removePointBacking);
        QVERIFY(controller.document()
                .layer(layerId)
                ->strokes.last()
                .pixelSelectionOp.has_value());
        QCOMPARE(static_cast<const void *>(controller.document()
                         .layer(layerId)
                         ->strokes.last()
                         .pixelSelectionOp->packedMask.constData()),
            static_cast<const void *>(removeSelectionBacking));
    }

    void refusesTrustedLeaseForExternalAliases()
    {
        Document source = Document::createDefault(QSize(64, 64));
        Stroke stroke;
        stroke.points = {{QPointF(4.0, 4.0), 1.0}, {QPointF(40.0, 40.0), 1.0}};
        stroke.clipMask = QImage(source.size, QImage::Format_Grayscale8);
        stroke.clipMask.fill(0);
        for (int y = 4; y < 20; ++y)
        {
            std::fill_n(stroke.clipMask.scanLine(y) + 4, 16, 255);
        }
        source.layers.first().strokes.append(stroke);

        DocumentSerializer::SerializationCache cache;
        const std::optional<DocumentSerializer::PreparedDocument> prepared =
            DocumentSerializer::prepare(source, cache);
        QVERIFY(prepared.has_value());
        const Stroke &trusted =
            prepared->document().layers.first().strokes.first();

        QVERIFY(
            DocumentSerializer::retainImmutableBackings(*prepared, {trusted})
                .isValid());

        Stroke pointAlias = trusted;
        pointAlias.points.detach();
        QVERIFY(!DocumentSerializer::retainImmutableBackings(
            *prepared, {pointAlias})
                .isValid());

        Stroke maskAlias = trusted;
        maskAlias.clipMask = trusted.clipMask.copy();
        QVERIFY(
            !DocumentSerializer::retainImmutableBackings(*prepared, {maskAlias})
                .isValid());

        Stroke foreign = trusted;
        foreign.id = QUuid::createUuid();
        QVERIFY(
            !DocumentSerializer::retainImmutableBackings(*prepared, {foreign})
                .isValid());

        QVERIFY(!DocumentSerializer::ImmutableBackingLease().isValid());
    }

    void commitsLargeDocumentMacroAsOnePreparedStage()
    {
        DocumentController controller;
        controller.loadDocument(documentWithStrokeCount(20000));
        const QUuid layerId = controller.document().activeLayerId;
        QSignalSpy documentChangedSpy(
            &controller, &DocumentController::documentChanged);

        controller.undoStack()->beginMacro(
            QStringLiteral("Large metadata transaction"));
        for (int index = 0; index < 64; ++index)
        {
            controller.renameLayer(
                layerId, QStringLiteral("Large layer %1").arg(index));
            QVERIFY(
                controller.undoStack()->storageStats().macroPreparedDocuments
                <= qsizetype(2));
        }
        QCOMPARE(documentChangedSpy.count(), 0);
        QCOMPARE(controller.undoStack()->storageStats().macroPreparedDocuments,
            qsizetype(2));
        controller.undoStack()->endMacro();

        QCOMPARE(controller.undoStack()->count(), 1);
        QCOMPARE(documentChangedSpy.count(), 1);
        DocumentUndoStack::StorageStats stats =
            controller.undoStack()->storageStats();
        QCOMPARE(stats.entryCount, qsizetype(1));
        QCOMPARE(stats.macroPreparedDocuments, qsizetype(0));
        QCOMPARE(stats.retainedPreparedDocuments, qsizetype(0));
        QCOMPARE(stats.stagedPreparedDocuments, qsizetype(0));
        QCOMPARE(stats.peakTransientPreparedDocuments, qsizetype(0));

        controller.undoStack()->undo();
        stats = controller.undoStack()->storageStats();
        QCOMPARE(stats.stagedPreparedDocuments, qsizetype(0));
        QCOMPARE(stats.peakTransientPreparedDocuments, qsizetype(1));
        QCOMPARE(controller.document().layer(layerId)->name,
            QStringLiteral("Layer 1"));
    }

    void boundsSixtyFourUniqueFourKSelectionSnapshots()
    {
        constexpr int edge = 4096;
        constexpr int rows = 64;
        constexpr int editCount = 64;
        constexpr qint64 testResidentLimit = 2LL * 1024LL * 1024LL;
        QImage state(QSize(edge, rows), QImage::Format_Grayscale8);
        QVERIFY(!state.isNull());
        state.fill(255);

        DocumentController controller;
        controller.newDocument(state.size());
        DocumentControllerTestAccess::setHistoryResidentLimit(
            controller, testResidentLimit);
        const QUuid layerId = controller.document().activeLayerId;
        int observedLastPixel = -1;
        QObject::connect(&controller,
            &DocumentController::selectionHistoryStateRequested,
            &controller,
            [&observedLastPixel](const QUuid &, const QImage &mask)
            {
                observedLastPixel =
                    mask.isNull() ? -1 : mask.constScanLine(0)[editCount - 1];
            });

        for (int index = 0; index < editCount; ++index)
        {
            const QImage before = state;
            state.detach();
            state.scanLine(0)[index] = 0;
            controller.pushSelectionStateCommand(
                QStringLiteral("4K selection transform %1").arg(index),
                layerId,
                before,
                layerId,
                state);
        }

        const DocumentUndoStack::StorageStats stats =
            controller.undoStack()->storageStats();
        QCOMPARE(stats.entryCount, qsizetype(controller.undoStack()->count()));
        QVERIFY(stats.retainedBytes <= testResidentLimit);
        QVERIFY(!stats.residentBudgetSoftExceeded);
        QVERIFY(controller.undoStack()->count() < editCount);
        QCOMPARE(observedLastPixel, 0);

        const int retainedCount = controller.undoStack()->count();
        controller.undoStack()->undo();
        QCOMPARE(observedLastPixel, 255);
        controller.undoStack()->redo();
        QCOMPARE(observedLastPixel, 0);
        QCOMPARE(controller.undoStack()->index(), retainedCount);
        QCOMPARE(observedLastPixel, 0);
    }

    void boundsProductionBudgetForSixtyFourUniqueFourKMasks()
    {
        constexpr int edge = 4096;
        constexpr int editCount = 64;
        QImage state(edge, edge, QImage::Format_Grayscale8);
        QVERIFY(!state.isNull());
        state.fill(255);

        DocumentController controller;
        controller.newDocument(state.size());
        const QUuid layerId = controller.document().activeLayerId;
        int observedLastPixel = -1;
        QObject::connect(&controller,
            &DocumentController::selectionHistoryStateRequested,
            &controller,
            [&observedLastPixel](const QUuid &, const QImage &mask)
            {
                observedLastPixel =
                    mask.isNull() ? -1
                                  : mask.constScanLine(edge - 1)[editCount - 1];
            });

        for (int index = 0; index < editCount; ++index)
        {
            const QImage before = state;
            state.detach();
            state.scanLine(edge - 1)[index] = 0;
            controller.pushSelectionStateCommand(
                QStringLiteral("Native 4K selection %1").arg(index),
                layerId,
                before,
                layerId,
                state);
        }

        const DocumentUndoStack::StorageStats stats =
            controller.undoStack()->storageStats();
        QCOMPARE(stats.entryCount, qsizetype(controller.undoStack()->count()));
        QVERIFY(stats.retainedBytes <= DocumentUndoStack::maximumResidentBytes);
        QVERIFY(!stats.residentBudgetSoftExceeded);
        QVERIFY(controller.undoStack()->count() >= editCount / 2);
        QCOMPARE(
            controller.undoStack()->index(), controller.undoStack()->count());
        QCOMPARE(observedLastPixel, 0);

        const int retainedCount = controller.undoStack()->count();
        controller.undoStack()->undo();
        QCOMPARE(observedLastPixel, 255);
        controller.undoStack()->redo();
        QCOMPARE(observedLastPixel, 0);
        QCOMPARE(controller.undoStack()->index(), retainedCount);
    }

    void softRetainsSingleOversizedHistoryEntry()
    {
        DocumentController controller;
        controller.newDocument(QSize(256, 256));
        DocumentControllerTestAccess::setHistoryResidentLimit(
            controller, 4LL * 1024LL);
        const QUuid layerId = controller.document().activeLayerId;
        int observedWidth = -2;
        QObject::connect(&controller,
            &DocumentController::selectionHistoryStateRequested,
            &controller,
            [&observedWidth](const QUuid &, const QImage &mask)
            {
                observedWidth = mask.isNull() ? -1 : mask.width();
            });

        QImage oversized(256, 256, QImage::Format_Grayscale8);
        oversized.fill(255);
        controller.pushSelectionStateCommand(
            QStringLiteral("Oversized selection"),
            layerId,
            {},
            layerId,
            oversized);

        DocumentUndoStack::StorageStats stats =
            controller.undoStack()->storageStats();
        QCOMPARE(controller.undoStack()->count(), 1);
        QVERIFY(stats.retainedBytes > 4LL * 1024LL);
        QVERIFY(stats.residentBudgetSoftExceeded);
        QVERIFY(controller.undoStack()->canUndo());

        controller.undoStack()->undo();
        QCOMPARE(observedWidth, -1);
        controller.undoStack()->redo();
        QCOMPARE(observedWidth, 256);

        QImage second(192, 256, QImage::Format_Grayscale8);
        second.fill(255);
        controller.pushSelectionStateCommand(
            QStringLiteral("Second oversized selection"),
            layerId,
            oversized,
            layerId,
            second);
        stats = controller.undoStack()->storageStats();
        QCOMPARE(controller.undoStack()->count(), 1);
        QCOMPARE(controller.undoStack()->index(), 1);
        QVERIFY(stats.residentBudgetSoftExceeded);
        controller.undoStack()->undo();
        QCOMPARE(observedWidth, 256);
    }

    void retainsOnlyChangedPayloadForLargeDocumentHistory()
    {
        constexpr int strokeLimit = DocumentLimits::maximumTotalStrokes;

        DocumentController controller;
        controller.loadDocument(documentWithStrokeCount(strokeLimit));
        const QUuid layerId = controller.document().activeLayerId;
        QCOMPARE(
            controller.document().layer(layerId)->strokes.size(), strokeLimit);

        QElapsedTimer timer;
        timer.start();
        for (int index = 0; index < 64; ++index)
        {
            controller.renameLayer(
                layerId, QStringLiteral("Large layer %1").arg(index));
        }
        const qint64 renameMs = timer.elapsed();
        const DocumentUndoStack::StorageStats renameStorage =
            controller.undoStack()->storageStats();
        QCOMPARE(controller.undoStack()->count(), 64);
        QCOMPARE(renameStorage.retainedLayers, qsizetype(0));
        QCOMPARE(renameStorage.retainedStrokes, qsizetype(0));
        QCOMPARE(renameStorage.retainedPreparedDocuments, qsizetype(0));

        Document addBase = controller.document();
        addBase.layers.first().strokes.resize(strokeLimit - 64);
        controller.loadDocument(std::move(addBase));
        const QUuid addLayerId = controller.document().activeLayerId;
        DocumentControllerTestAccess::resetSerializationStats(controller);
        timer.restart();
        for (int index = 0; index < 64; ++index)
        {
            Stroke stroke;
            stroke.seed =
                static_cast<quint64>(strokeLimit) + static_cast<quint64>(index);
            stroke.points = {
                {QPointF(1.0 + static_cast<qreal>(index % 62), 32.0), 1.0}};
            QCOMPARE(controller.addStroke(addLayerId, std::move(stroke)),
                DocumentController::AddStrokeResult::Added);
        }
        const qint64 addMs = timer.elapsed();
        const DocumentUndoStack::StorageStats addStorage =
            controller.undoStack()->storageStats();
        QCOMPARE(controller.undoStack()->count(), 64);
        QCOMPARE(addStorage.retainedLayers, qsizetype(0));
        QCOMPARE(addStorage.retainedStrokes, qsizetype(64));
        QCOMPARE(addStorage.retainedPreparedDocuments, qsizetype(0));
        QCOMPARE(controller.document().layer(addLayerId)->strokes.size(),
            strokeLimit);
        const auto appendSerialization =
            DocumentControllerTestAccess::serializationStats(controller);
        QCOMPARE(appendSerialization.incrementalStrokeAppends, 64ULL);
        QCOMPARE(appendSerialization.fullDocumentPreparations, 0ULL);
        QCOMPARE(appendSerialization.strokeSerializations, 64ULL);
        QCOMPARE(appendSerialization.clipMaskContentHashes, 0ULL);
        QCOMPARE(appendSerialization.binaryMaskContentHashes, 0ULL);
        controller.undoStack()->undo();
        QCOMPARE(controller.document().layer(addLayerId)->strokes.size(),
            strokeLimit - 1);
        controller.undoStack()->redo();
        QCOMPARE(controller.document().layer(addLayerId)->strokes.size(),
            strokeLimit);

        controller.loadDocument(controller.document());
        const qreal originalWobble = controller.document().wobbleAmount;
        timer.restart();
        for (int index = 0; index < 64; ++index)
        {
            controller.setWobbleAmount(2.0 + static_cast<qreal>(index) / 100.0);
        }
        const qint64 scalarMs = timer.elapsed();
        const qreal finalWobble = controller.document().wobbleAmount;
        const DocumentUndoStack::StorageStats scalarStorage =
            controller.undoStack()->storageStats();
        QCOMPARE(controller.undoStack()->count(), 1);
        QCOMPARE(scalarStorage.retainedLayers, qsizetype(0));
        QCOMPARE(scalarStorage.retainedStrokes, qsizetype(0));
        QCOMPARE(scalarStorage.retainedPreparedDocuments, qsizetype(0));
        controller.undoStack()->undo();
        QCOMPARE(controller.document().wobbleAmount, originalWobble);
        controller.undoStack()->redo();
        QCOMPARE(controller.document().wobbleAmount, finalWobble);

        controller.loadDocument(controller.document());
        const QUuid clearLayerId = controller.document().activeLayerId;
        timer.restart();
        controller.clearLayer(clearLayerId);
        const qint64 clearMs = timer.elapsed();
        QCOMPARE(controller.undoStack()->storageStats().retainedStrokes,
            qsizetype(strokeLimit));
        QCOMPARE(controller.document().layer(clearLayerId)->strokes.size(), 0);
        timer.restart();
        controller.undoStack()->undo();
        const qint64 restoreClearMs = timer.elapsed();
        QCOMPARE(controller.document().layer(clearLayerId)->strokes.size(),
            strokeLimit);

        controller.loadDocument(controller.document());
        const QUuid removedLayerId = controller.document().activeLayerId;
        timer.restart();
        controller.removeLayer(removedLayerId);
        const qint64 removeMs = timer.elapsed();
        QCOMPARE(controller.document().layers.size(), 0);
        QCOMPARE(controller.undoStack()->storageStats().retainedStrokes,
            qsizetype(strokeLimit));
        timer.restart();
        controller.undoStack()->undo();
        const qint64 restoreRemoveMs = timer.elapsed();
        QCOMPARE(controller.document().layer(removedLayerId)->strokes.size(),
            strokeLimit);

        qInfo().nospace() << "large history latency ms: rename=" << renameMs
                          << ", add=" << addMs << ", scalar=" << scalarMs
                          << ", clear=" << clearMs
                          << ", restore-clear=" << restoreClearMs
                          << ", remove=" << removeMs
                          << ", restore-remove=" << restoreRemoveMs;
    }

    void preflightsWholeMacroBeforeMovingHistory()
    {
        DocumentController controller;
        controller.newDocument(QSize(128, 128));
        const QUuid layerId = controller.document().activeLayerId;
        int transientState = 0;
        QObject::connect(&controller,
            &DocumentController::selectionHistoryStateRequested,
            &controller,
            [&transientState](const QUuid &, const QImage &mask)
            {
                transientState = mask.isNull() ? 0 : 1;
            });
        QImage selected(1, 1, QImage::Format_Grayscale8);
        selected.fill(255);

        controller.undoStack()->beginMacro(QStringLiteral("Atomic macro"));
        controller.pushSelectionStateCommand(
            QStringLiteral("Selection"), {}, {}, layerId, selected);
        controller.renameLayer(layerId, QStringLiteral("Renamed"));
        controller.setLayerVisible(layerId, false);
        controller.undoStack()->endMacro();
        controller.markSaved();

        const int beforeIndex = controller.undoStack()->index();
        const int beforeCount = controller.undoStack()->count();
        const quint64 beforeRevision =
            DocumentControllerTestAccess::contentRevision(controller);
        const quint64 beforeNode =
            DocumentControllerTestAccess::historyNode(controller);
        QSignalSpy documentChangedSpy(
            &controller, &DocumentController::documentChanged);
        QSignalSpy modifiedChangedSpy(
            &controller, &DocumentController::modifiedChanged);

        // The aggregate command has one prepared target. Rejecting that
        // target must leave the entire document/selection transaction still.
        DocumentControllerTestAccess::failHistoryPrepareAfter(controller, 0);
        controller.undoStack()->undo();

        QCOMPARE(controller.undoStack()->index(), beforeIndex);
        QCOMPARE(controller.undoStack()->count(), beforeCount);
        QCOMPARE(transientState, 1);
        QCOMPARE(controller.document().layer(layerId)->name,
            QStringLiteral("Renamed"));
        QVERIFY(!controller.document().layer(layerId)->visible);
        QVERIFY(!controller.isModified());
        QCOMPARE(DocumentControllerTestAccess::contentRevision(controller),
            beforeRevision);
        QCOMPARE(
            DocumentControllerTestAccess::historyNode(controller), beforeNode);
        QCOMPARE(documentChangedSpy.count(), 0);
        QCOMPARE(modifiedChangedSpy.count(), 0);
        QCOMPARE(
            controller.undoStack()->storageStats().retainedPreparedDocuments,
            qsizetype(0));

        controller.undoStack()->undo();
        QCOMPARE(controller.undoStack()->index(), 0);
        QCOMPARE(transientState, 0);
        QCOMPARE(controller.document().layer(layerId)->name,
            QStringLiteral("Layer 1"));
        QVERIFY(controller.document().layer(layerId)->visible);
        QVERIFY(controller.isModified());

        controller.undoStack()->redo();
        QCOMPARE(controller.undoStack()->index(), 1);
        QCOMPARE(transientState, 1);
        QCOMPARE(controller.document().layer(layerId)->name,
            QStringLiteral("Renamed"));
        QVERIFY(!controller.document().layer(layerId)->visible);
        QVERIFY(!controller.isModified());
    }

    void blocksHistoryMutationDuringSynchronousCallbacks()
    {
        Document document = Document::createDefault(QSize(160, 120));
        Layer secondLayer;
        secondLayer.name = QStringLiteral("Layer 2");
        secondLayer.initialCanvasSize = document.size;
        const QUuid firstLayerId = document.activeLayerId;
        const QUuid secondLayerId = secondLayer.id;
        document.layers.append(secondLayer);

        DocumentController controller;
        controller.loadDocument(std::move(document));
        QTemporaryDir tempDir;
        QVERIFY(tempDir.isValid());
        const QString rejectedSavePath =
            tempDir.filePath(QStringLiteral("rejected-save.ugu"));
        int phase = 0;
        int transientState = 0;
        bool pushStateWasAtomic = false;
        bool undoStateWasAtomic = false;
        bool reentrantSaveSucceeded = true;
        QObject::connect(&controller,
            &DocumentController::documentChanged,
            &controller,
            [&]()
            {
                if (phase == 1)
                {
                    phase = 0;
                    pushStateWasAtomic =
                        controller.document()
                                .layer(firstLayerId)
                                ->strokes.size()
                            == 1
                        && controller.isModified()
                        && DocumentControllerTestAccess::contentRevision(
                               controller)
                               == 1
                        && DocumentControllerTestAccess::historyNode(controller)
                               != 0;
                    controller.undoStack()->undo();
                    controller.undoStack()->redo();
                    controller.renameLayer(
                        firstLayerId, QStringLiteral("Rejected nested rename"));
                    QImage selected(1, 1, QImage::Format_Grayscale8);
                    selected.fill(255);
                    controller.pushSelectionStateCommand(
                        QStringLiteral("Rejected transient"),
                        {},
                        {},
                        firstLayerId,
                        selected);
                    controller.newDocument(QSize(32, 32));
                    controller.loadDocument(
                        Document::createDefault(QSize(48, 48)));
                    controller.loadRecoveredDocument(
                        Document::createDefault(QSize(48, 48)));
                    reentrantSaveSucceeded =
                        controller.saveDocument(rejectedSavePath);
                    controller.markSaved();
                    // Active-layer selection is deliberately non-history UI
                    // state and remains legal after the target is installed.
                    controller.setActiveLayer(secondLayerId);
                }
                else if (phase == 2)
                {
                    phase = 0;
                    undoStateWasAtomic =
                        controller.document()
                            .layer(firstLayerId)
                            ->strokes.isEmpty()
                        && !controller.isModified()
                        && DocumentControllerTestAccess::contentRevision(
                               controller)
                               == 0
                        && DocumentControllerTestAccess::historyNode(controller)
                               == 0;
                    controller.undoStack()->undo();
                    controller.undoStack()->redo();
                    controller.renameLayer(
                        firstLayerId, QStringLiteral("Also rejected"));
                }
            });

        Stroke stroke;
        stroke.points = {
            {QPointF(20.0, 20.0), 1.0}, {QPointF(40.0, 40.0), 1.0}};
        phase = 1;
        controller.addStroke(firstLayerId, std::move(stroke));
        QVERIFY(pushStateWasAtomic);
        QCOMPARE(controller.undoStack()->count(), 1);
        QCOMPARE(controller.undoStack()->index(), 1);
        QCOMPARE(controller.document().size, QSize(160, 120));
        QCOMPARE(controller.document().layer(firstLayerId)->name,
            QStringLiteral("Layer 1"));
        QCOMPARE(controller.document().layer(firstLayerId)->strokes.size(), 1);
        QCOMPARE(controller.document().activeLayerId, secondLayerId);
        QCOMPARE(transientState, 0);
        QVERIFY(controller.isModified());
        QVERIFY(!reentrantSaveSucceeded);
        QVERIFY(!QFile::exists(rejectedSavePath));

        phase = 2;
        controller.undoStack()->undo();
        QVERIFY(undoStateWasAtomic);
        QCOMPARE(controller.undoStack()->index(), 0);
        QVERIFY(controller.document().layer(firstLayerId)->strokes.isEmpty());
        QCOMPARE(controller.document().layer(firstLayerId)->name,
            QStringLiteral("Layer 1"));
        QCOMPARE(controller.document().activeLayerId, secondLayerId);
        QVERIFY(!controller.isModified());
    }

    void commitsAccumulatedSelectionTransformAsOneCommand()
    {
        Document document = Document::createDefault(QSize(240, 180));
        Stroke stroke;
        stroke.width = 8.0;
        stroke.points = {
            {QPointF(70.0, 70.0), 0.5}, {QPointF(110.0, 80.0), 1.0}};
        const QUuid layerId = document.activeLayerId;
        const QUuid strokeId = stroke.id;
        document.layers.first().strokes.append(stroke);

        DocumentController controller;
        controller.loadDocument(document);
        QSignalSpy transformedSpy(
            &controller, &DocumentController::strokesTransformed);
        QTransform accumulated;
        accumulated.translate(12.0, 6.0);
        accumulated.translate(90.0, 75.0);
        accumulated.rotate(15.0);
        accumulated.scale(1.25, 1.25);
        accumulated.translate(-90.0, -75.0);

        QVERIFY(
            controller.transformSelection(layerId, {strokeId}, accumulated));
        QCOMPARE(controller.undoStack()->count(), 1);
        QCOMPARE(controller.undoStack()->index(), 1);
        const Stroke transformed =
            controller.document().layer(layerId)->strokes.first();
        QCOMPARE(transformed.points.first().position,
            accumulated.map(stroke.points.first().position));
        QCOMPARE(transformed.points.last().position,
            accumulated.map(stroke.points.last().position));
        QVERIFY(qAbs(transformed.width - 10.0) < 0.000001);
        QCOMPARE(transformedSpy.count(), 1);
        QCOMPARE(qvariant_cast<QTransform>(transformedSpy[0][2]), accumulated);

        QVERIFY(
            !controller.transformSelection(layerId, {strokeId}, QTransform()));
        QCOMPARE(controller.undoStack()->count(), 1);

        QTransform perspective;
        perspective.setMatrix(1.0, 0.0, 0.001, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0);
        QVERIFY(!perspective.isAffine());
        QVERIFY(
            !controller.transformSelection(layerId, {strokeId}, perspective));
        QCOMPARE(controller.undoStack()->count(), 1);
        QVERIFY(!controller.transformSelection(layerId,
            {strokeId},
            QTransform::fromTranslate(1.0, 0.0),
            {},
            static_cast<SamplingMode>(99)));
        QCOMPARE(controller.undoStack()->count(), 1);
        QCOMPARE(controller.document().layer(layerId)->strokes.first().points,
            transformed.points);

        controller.undoStack()->undo();
        QCOMPARE(controller.undoStack()->index(), 0);
        const Stroke restored =
            controller.document().layer(layerId)->strokes.first();
        QCOMPARE(restored.points, stroke.points);
        QCOMPARE(restored.width, stroke.width);
        QCOMPARE(transformedSpy.count(), 2);
        bool invertible = false;
        const QTransform inverse = accumulated.inverted(&invertible);
        QVERIFY(invertible);
        QCOMPARE(qvariant_cast<QTransform>(transformedSpy[1][2]), inverse);

        controller.undoStack()->redo();
        QCOMPARE(controller.undoStack()->index(), 1);
        QCOMPARE(controller.document().layer(layerId)->strokes.first().points,
            transformed.points);
    }

    void roundTripsLayerAndStrokeSequenceDeltas()
    {
        Document layers = Document::createDefault(QSize(96, 96));
        layers.layers.first().name = QStringLiteral("A");
        Layer second;
        second.name = QStringLiteral("B");
        second.initialCanvasSize = layers.size;
        Layer third;
        third.name = QStringLiteral("C");
        third.initialCanvasSize = layers.size;
        const QUuid firstId = layers.layers.first().id;
        const QUuid secondId = second.id;
        const QUuid thirdId = third.id;
        layers.layers.append(second);
        layers.layers.append(third);
        layers.activeLayerId = secondId;

        const auto layerOrder = [](const Document &document)
        {
            QVector<QUuid> ids;
            for (const Layer &layer : document.layers)
            {
                ids.append(layer.id);
            }
            return ids;
        };

        DocumentController controller;
        controller.loadDocument(std::move(layers));
        controller.moveLayer(firstId, 2);
        QCOMPARE(layerOrder(controller.document()),
            QVector<QUuid>({secondId, thirdId, firstId}));
        QCOMPARE(controller.document().activeLayerId, secondId);

        controller.setActiveLayer(thirdId);
        controller.undoStack()->undo();
        QCOMPARE(layerOrder(controller.document()),
            QVector<QUuid>({firstId, secondId, thirdId}));
        QCOMPARE(controller.document().activeLayerId, thirdId);
        controller.undoStack()->redo();
        QCOMPARE(layerOrder(controller.document()),
            QVector<QUuid>({secondId, thirdId, firstId}));
        QCOMPARE(controller.document().activeLayerId, thirdId);

        Document strokes = Document::createDefault(QSize(96, 96));
        QVector<QUuid> originalIds;
        for (int index = 0; index < 5; ++index)
        {
            Stroke item;
            item.seed = static_cast<quint64>(index);
            item.points = {{QPointF(10.0 + index * 10.0, 40.0), 1.0}};
            originalIds.append(item.id);
            strokes.layers.first().strokes.append(std::move(item));
        }
        const QUuid strokeLayerId = strokes.activeLayerId;
        const auto strokeOrder = [strokeLayerId](const Document &document)
        {
            QVector<QUuid> ids;
            for (const Stroke &item : document.layer(strokeLayerId)->strokes)
            {
                ids.append(item.id);
            }
            return ids;
        };
        controller.loadDocument(std::move(strokes));
        controller.removeStrokes(
            strokeLayerId, {originalIds[1], originalIds[3]});
        QCOMPARE(strokeOrder(controller.document()),
            QVector<QUuid>({originalIds[0], originalIds[2], originalIds[4]}));
        controller.undoStack()->undo();
        QCOMPARE(strokeOrder(controller.document()), originalIds);
        controller.undoStack()->redo();
        QCOMPARE(strokeOrder(controller.document()),
            QVector<QUuid>({originalIds[0], originalIds[2], originalIds[4]}));
    }

    void mergesScalarCommandsFromEarliestToLatestValue()
    {
        DocumentController controller;
        controller.newDocument(QSize(96, 96));
        const QUuid layerId = controller.document().activeLayerId;

        controller.setWobbleAmount(2.0);
        controller.setWobbleAmount(3.0);
        controller.setAnimationFrames(12);
        controller.setAnimationFrames(18);
        controller.setFramesPerSecond(10.0);
        controller.setFramesPerSecond(20.0);
        controller.setLayerOpacity(layerId, 0.4);
        controller.setLayerOpacity(layerId, 0.7);
        QCOMPARE(controller.undoStack()->count(), 4);
        QCOMPARE(controller.undoStack()->storageStats().retainedStrokes,
            qsizetype(0));

        controller.undoStack()->undo();
        QCOMPARE(controller.document().layer(layerId)->opacity, 1.0);
        controller.undoStack()->undo();
        QCOMPARE(controller.document().framesPerSecond, 25.0);
        controller.undoStack()->undo();
        QCOMPARE(controller.document().animationFrames, 30);
        controller.undoStack()->undo();
        QCOMPARE(controller.document().wobbleAmount, 1.6);

        controller.undoStack()->redo();
        QCOMPARE(controller.document().wobbleAmount, 3.0);
        controller.undoStack()->redo();
        QCOMPARE(controller.document().animationFrames, 18);
        controller.undoStack()->redo();
        QCOMPARE(controller.document().framesPerSecond, 20.0);
        controller.undoStack()->redo();
        QCOMPARE(controller.document().layer(layerId)->opacity, 0.7);
    }

    void mergesOnlyTheSelectedScalarField()
    {
        Document base = Document::createDefault(QSize(96, 96));
        Document wobbleTwo = base;
        wobbleTwo.wobbleAmount = 2.0;
        Document wobbleThree = wobbleTwo;
        wobbleThree.wobbleAmount = 3.0;

        history::DocumentDelta wobbleDelta =
            history::DocumentDelta::between(base, wobbleTwo);
        const history::DocumentDelta nextWobbleDelta =
            history::DocumentDelta::between(wobbleTwo, wobbleThree);
        QVERIFY(wobbleDelta.mergeScalar(
            nextWobbleDelta, history::wobbleAmountMergeId, QUuid()));
        QVERIFY(wobbleDelta.wobbleAmount.has_value());
        QCOMPARE(wobbleDelta.wobbleAmount->before, base.wobbleAmount);
        QCOMPARE(wobbleDelta.wobbleAmount->after, 3.0);

        const QUuid layerId = base.activeLayerId;
        Document opacityPointFour = base;
        opacityPointFour.layer(layerId)->opacity = 0.4;
        Document opacityPointSeven = opacityPointFour;
        opacityPointSeven.layer(layerId)->opacity = 0.7;
        history::DocumentDelta opacityDelta =
            history::DocumentDelta::between(base, opacityPointFour);
        const history::DocumentDelta nextOpacityDelta =
            history::DocumentDelta::between(
                opacityPointFour, opacityPointSeven);
        QVERIFY(opacityDelta.mergeScalar(
            nextOpacityDelta, history::layerOpacityMergeId, layerId));
        QCOMPARE(opacityDelta.changedLayers.size(), 1);
        QVERIFY(opacityDelta.changedLayers.first().opacity.has_value());
        QCOMPARE(opacityDelta.changedLayers.first().opacity->before, 1.0);
        QCOMPARE(opacityDelta.changedLayers.first().opacity->after, 0.7);
    }

    void keepsMotionFieldsInDistinctUndoCommands()
    {
        DocumentController controller;
        controller.newDocument(QSize(96, 96));

        controller.setMotionStyle(MotionStyle::Smooth);
        controller.setMotionPoseCount(4);
        controller.setMotionPoseCount(5);
        controller.setMotionDetail(14);
        controller.setMotionDetail(18);
        controller.setMotionLinked(0.8);
        controller.setMotionLinked(0.6);
        controller.setMotionRandomness(0.2);
        controller.setMotionRandomness(0.5);
        controller.setBrokenLineEnabled(true);
        controller.setBreakAmount(0.5);
        controller.setBreakAmount(0.7);
        controller.setBreakRange(36.0);
        controller.setBreakRange(48.0);

        QCOMPARE(controller.undoStack()->count(), 8);
        QCOMPARE(controller.undoStack()->storageStats().retainedStrokes,
            qsizetype(0));
        QCOMPARE(controller.document().motion.poseCount, 5);
        QCOMPARE(controller.document().motion.detail, 18);
        QCOMPARE(controller.document().motion.linked, 0.6);
        QCOMPARE(controller.document().motion.randomness, 0.5);
        QCOMPARE(controller.document().motion.breakAmount, 0.7);
        QCOMPARE(controller.document().motion.breakRange, 48.0);

        for (int index = 0; index < 8; ++index)
        {
            controller.undoStack()->undo();
        }
        QVERIFY(controller.document().motion == MotionSettings());
    }

    void rejectsMixedOrMismatchedScalarMerges()
    {
        Document base = Document::createDefault(QSize(96, 96));
        Document wobbleTwo = base;
        wobbleTwo.wobbleAmount = 2.0;
        history::DocumentDelta wobbleDelta =
            history::DocumentDelta::between(base, wobbleTwo);

        Document mixedDocument = wobbleTwo;
        mixedDocument.wobbleAmount = 3.0;
        mixedDocument.background = QColor(30, 40, 50);
        const history::DocumentDelta mixedDelta =
            history::DocumentDelta::between(wobbleTwo, mixedDocument);
        QVERIFY(!wobbleDelta.mergeScalar(
            mixedDelta, history::wobbleAmountMergeId, QUuid()));
        QCOMPARE(wobbleDelta.wobbleAmount->after, 2.0);

        Document frameDocument = wobbleTwo;
        frameDocument.animationFrames = 12;
        const history::DocumentDelta frameDelta =
            history::DocumentDelta::between(wobbleTwo, frameDocument);
        QVERIFY(!wobbleDelta.mergeScalar(
            frameDelta, history::wobbleAmountMergeId, QUuid()));
        QCOMPARE(wobbleDelta.wobbleAmount->after, 2.0);

        const QUuid layerId = base.activeLayerId;
        Document opacityPointFour = base;
        opacityPointFour.layer(layerId)->opacity = 0.4;
        Document opacityAndName = opacityPointFour;
        opacityAndName.layer(layerId)->opacity = 0.7;
        opacityAndName.layer(layerId)->name = QStringLiteral("Renamed");
        history::DocumentDelta opacityDelta =
            history::DocumentDelta::between(base, opacityPointFour);
        const history::DocumentDelta mixedLayerDelta =
            history::DocumentDelta::between(opacityPointFour, opacityAndName);
        QVERIFY(!opacityDelta.mergeScalar(
            mixedLayerDelta, history::layerOpacityMergeId, layerId));
        QCOMPARE(opacityDelta.changedLayers.first().opacity->after, 0.4);

        Document opacityPointSeven = opacityPointFour;
        opacityPointSeven.layer(layerId)->opacity = 0.7;
        const history::DocumentDelta nextOpacityDelta =
            history::DocumentDelta::between(
                opacityPointFour, opacityPointSeven);
        QVERIFY(!opacityDelta.mergeScalar(nextOpacityDelta,
            history::layerOpacityMergeId,
            QUuid::createUuid()));
        QCOMPARE(opacityDelta.changedLayers.first().opacity->after, 0.4);
    }
};

int runDocumentHistoryTests(int argc, char **argv)
{
    DocumentHistoryTests tests;
    return QTest::qExec(&tests, argc, argv);
}

}

#include "DocumentHistoryTests.moc"

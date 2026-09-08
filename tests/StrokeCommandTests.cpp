// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "support/DocumentTestHelpers.hpp"
#include "support/DocumentTestSuites.hpp"

namespace ugurugu
{

class StrokeCommandTests final : public QObject
{
    Q_OBJECT

private slots:
    void transformsAndDuplicatesStrokesUndoably()
    {
        Document document = Document::createDefault(QSize(100, 100));
        Stroke stroke;
        stroke.width = 6.0;
        stroke.points = {
            {QPointF(40.0, 50.0), 1.0}, {QPointF(60.0, 50.0), 1.0}};
        const QUuid strokeId = stroke.id;
        const QUuid layerId = document.activeLayerId;
        document.layers.first().strokes.append(stroke);

        DocumentController controller;
        controller.loadDocument(document);
        QVERIFY(controller.scaleStrokes(
            layerId, {strokeId}, QPointF(50.0, 50.0), 2.0));
        const Stroke &scaled =
            controller.document().layers.first().strokes.first();
        QCOMPARE(scaled.points.first().position, QPointF(30.0, 50.0));
        QCOMPARE(scaled.points.last().position, QPointF(70.0, 50.0));
        QCOMPARE(scaled.width, 12.0);

        controller.undoStack()->undo();
        QCOMPARE(controller.document().layers.first().strokes.first().points,
            stroke.points);

        QVERIFY(controller.rotateStrokes(
            layerId, {strokeId}, QPointF(50.0, 50.0), 90.0));
        const Stroke &rotated =
            controller.document().layers.first().strokes.first();
        QVERIFY(qAbs(rotated.points.first().position.x() - 50.0) < 0.0001);
        QVERIFY(qAbs(rotated.points.last().position.x() - 50.0) < 0.0001);
        QVERIFY(qAbs(rotated.points.first().position.y()
                     - rotated.points.last().position.y())
                > 19.999);

        controller.undoStack()->undo();
        QVERIFY(controller.duplicateStrokes(
            layerId, {strokeId}, QPointF(10.0, 5.0)));
        QCOMPARE(controller.document().layers.first().strokes.size(), 2);
        const Stroke &copy =
            controller.document().layers.first().strokes.last();
        QVERIFY(copy.id != strokeId);
        QCOMPARE(copy.points.first().position, QPointF(50.0, 55.0));
        QCOMPARE(copy.points.last().position, QPointF(70.0, 55.0));

        controller.undoStack()->undo();
        QCOMPARE(controller.document().layers.first().strokes.size(), 1);
        controller.undoStack()->redo();
        QCOMPARE(controller.document().layers.first().strokes.size(), 2);

        QVERIFY(!controller.scaleStrokes(
            layerId, {strokeId}, QPointF(50.0, 50.0), 6.0));
        QCOMPARE(controller.document().layers.first().strokes.size(), 2);
    }

    void rotatesOnlyTheSelectedPartOfAnIntersectingStroke()
    {
        Document document = Document::createDefault(QSize(100, 100));
        document.background = Qt::transparent;
        document.wobbleAmount = 0.0;
        Stroke stroke;
        stroke.width = 4.0;
        stroke.points = {
            {QPointF(10.0, 50.0), 1.0}, {QPointF(90.0, 50.0), 1.0}};
        const QUuid strokeId = stroke.id;
        const QUuid layerId = document.activeLayerId;
        document.layers.first().strokes.append(stroke);

        QImage selection(document.size, QImage::Format_Grayscale8);
        selection.fill(0);
        for (int y = 40; y <= 60; ++y)
        {
            uchar *line = selection.scanLine(y);
            std::fill(line + 40, line + 61, 255);
        }

        const QImage before = RenderEngine::render(document, 0);
        DocumentController controller;
        controller.loadDocument(document);
        QVERIFY(controller.rotateStrokes(
            layerId, {strokeId}, QPointF(50.0, 50.0), 90.0, selection));

        const Layer &rotatedLayer = controller.document().layers.first();
        QCOMPARE(rotatedLayer.strokes.size(), 2);
        QCOMPARE(rotatedLayer.strokes.first().id, strokeId);
        QCOMPARE(rotatedLayer.strokes.first().points, stroke.points);
        const Stroke &operation = rotatedLayer.strokes.last();
        QCOMPARE(operation.mode, StrokeMode::PixelSelection);
        QVERIFY(operation.pixelSelectionOp.has_value());
        QVERIFY(operation.pixelSelectionOp->clearSource);
        QVERIFY(operation.pixelSelectionOp->drawDestination);
        QCOMPARE(operation.pixelSelectionOp->sampling, SamplingMode::Nearest);
        const QImage after = RenderEngine::render(controller.document(), 0);
        QVERIFY(after != before);

        controller.undoStack()->undo();
        const Layer &restoredLayer = controller.document().layers.first();
        QCOMPARE(restoredLayer.strokes.size(), 1);
        QCOMPARE(restoredLayer.strokes.first().id, strokeId);
        QCOMPARE(restoredLayer.strokes.first().points, stroke.points);
        QVERIFY(restoredLayer.strokes.first().clipMask.isNull());
        QCOMPARE(RenderEngine::render(controller.document(), 0), before);

        controller.undoStack()->redo();
        QCOMPARE(controller.document().layers.first().strokes.size(), 2);
        QCOMPARE(RenderEngine::render(controller.document(), 0), after);
    }

    void movesOnlyTheSelectedPartOfAnIntersectingStroke()
    {
        Document document = Document::createDefault(QSize(100, 100));
        document.background = Qt::transparent;
        document.wobbleAmount = 0.0;
        Stroke stroke;
        stroke.width = 4.0;
        stroke.points = {
            {QPointF(10.0, 50.0), 1.0}, {QPointF(90.0, 50.0), 1.0}};
        const QUuid strokeId = stroke.id;
        const QUuid layerId = document.activeLayerId;
        document.layers.first().strokes.append(stroke);

        QImage selection(document.size, QImage::Format_Grayscale8);
        selection.fill(0);
        for (int y = 40; y <= 60; ++y)
        {
            uchar *line = selection.scanLine(y);
            std::fill(line + 40, line + 61, 255);
        }

        const QImage before = RenderEngine::render(document, 0);
        DocumentController controller;
        controller.loadDocument(document);
        QSignalSpy transitionSpy(
            &controller, &DocumentController::selectionOverlayTransition);
        QVERIFY(controller.moveStrokes(
            layerId, {strokeId}, QPointF(0.0, 20.0), selection));

        const Layer &movedLayer = controller.document().layers.first();
        QCOMPARE(movedLayer.strokes.size(), 2);
        QCOMPARE(movedLayer.strokes.first().id, strokeId);
        QCOMPARE(movedLayer.strokes.first().points, stroke.points);
        const Stroke &operation = movedLayer.strokes.last();
        QCOMPARE(operation.mode, StrokeMode::PixelSelection);
        QVERIFY(operation.pixelSelectionOp.has_value());
        QVERIFY(operation.pixelSelectionOp->clearSource);
        QVERIFY(operation.pixelSelectionOp->drawDestination);
        QCOMPARE(operation.pixelSelectionOp->transform.map(QPointF(50.0, 50.0)),
            QPointF(50.0, 70.0));
        const QImage after = RenderEngine::render(controller.document(), 0);
        QVERIFY(after != before);
        QCOMPARE(transitionSpy.size(), 1);
        QCOMPARE(qvariant_cast<QImage>(transitionSpy[0][3]), selection);
        const QImage expectedNextMask = transformedSelectionSupport(selection,
            document.size,
            operation.pixelSelectionOp->transform,
            operation.pixelSelectionOp->sampling);
        QCOMPARE(qvariant_cast<QImage>(transitionSpy[0][4]), expectedNextMask);
        const std::optional<PackedMaskRegion> packedSource =
            packBinaryMask(selection);
        const std::optional<PackedMaskRegion> packedNext =
            packBinaryMask(expectedNextMask);
        QVERIFY(packedSource.has_value());
        QVERIFY(packedNext.has_value());
        QVERIFY(packedSource->packedMask.size() + packedNext->packedMask.size()
                < selection.sizeInBytes() + expectedNextMask.sizeInBytes());

        controller.undoStack()->undo();
        const Layer &restoredLayer = controller.document().layers.first();
        QCOMPARE(restoredLayer.strokes.size(), 1);
        QCOMPARE(restoredLayer.strokes.first().points, stroke.points);
        QVERIFY(restoredLayer.strokes.first().clipMask.isNull());
        QCOMPARE(RenderEngine::render(controller.document(), 0), before);
        QCOMPARE(transitionSpy.size(), 2);
        QCOMPARE(qvariant_cast<QImage>(transitionSpy[1][3]), expectedNextMask);
        QCOMPARE(qvariant_cast<QImage>(transitionSpy[1][4]), selection);
        controller.undoStack()->redo();
        QCOMPARE(RenderEngine::render(controller.document(), 0), after);
        QCOMPARE(transitionSpy.size(), 3);
    }

    void duplicatesOnlyTheSelectedPartOfAnIntersectingStroke()
    {
        Document document = Document::createDefault(QSize(100, 100));
        document.background = Qt::transparent;
        document.wobbleAmount = 0.0;
        Stroke stroke;
        stroke.width = 4.0;
        stroke.points = {
            {QPointF(10.0, 50.0), 1.0}, {QPointF(90.0, 50.0), 1.0}};
        const QUuid strokeId = stroke.id;
        const QUuid layerId = document.activeLayerId;
        document.layers.first().strokes.append(stroke);

        QImage selection(document.size, QImage::Format_Grayscale8);
        selection.fill(0);
        for (int y = 40; y <= 60; ++y)
        {
            std::fill(
                selection.scanLine(y) + 40, selection.scanLine(y) + 61, 255);
        }

        const QImage before = RenderEngine::render(document, 0);
        DocumentController controller;
        controller.loadDocument(document);
        QVERIFY(controller.duplicateStrokes(
            layerId, {strokeId}, QPointF(0.0, 20.0), selection));

        const Layer &duplicatedLayer = controller.document().layers.first();
        QCOMPARE(duplicatedLayer.strokes.size(), 2);
        const Stroke &original = duplicatedLayer.strokes.first();
        const Stroke &copyOperation = duplicatedLayer.strokes.last();
        QCOMPARE(original.id, strokeId);
        QCOMPARE(original.points, stroke.points);
        QVERIFY(original.clipMask.isNull());
        QCOMPARE(copyOperation.mode, StrokeMode::PixelSelection);
        QVERIFY(copyOperation.pixelSelectionOp.has_value());
        QVERIFY(!copyOperation.pixelSelectionOp->clearSource);
        QVERIFY(copyOperation.pixelSelectionOp->drawDestination);
        QCOMPARE(
            copyOperation.pixelSelectionOp->transform.map(QPointF(50.0, 50.0)),
            QPointF(50.0, 70.0));
        const QUuid copyId = copyOperation.id;
        const QImage after = RenderEngine::render(controller.document(), 0);
        QVERIFY(after != before);

        controller.undoStack()->undo();
        const Layer &restoredLayer = controller.document().layers.first();
        QCOMPARE(restoredLayer.strokes.size(), 1);
        QCOMPARE(restoredLayer.strokes.first().id, strokeId);
        QCOMPARE(restoredLayer.strokes.first().points, stroke.points);
        QVERIFY(restoredLayer.strokes.first().clipMask.isNull());
        QCOMPARE(RenderEngine::render(controller.document(), 0), before);

        controller.undoStack()->redo();
        const Layer &redoneLayer = controller.document().layers.first();
        QCOMPARE(redoneLayer.strokes.size(), 2);
        QCOMPARE(redoneLayer.strokes.first().id, strokeId);
        QCOMPARE(redoneLayer.strokes.first().points, stroke.points);
        QVERIFY(redoneLayer.strokes.first().clipMask.isNull());
        QCOMPARE(redoneLayer.strokes.last().id, copyId);
        QCOMPARE(redoneLayer.strokes.last().mode, StrokeMode::PixelSelection);
        QCOMPARE(RenderEngine::render(controller.document(), 0), after);
    }

    void removesOnlySelectedContentFromAnIntersectingStroke()
    {
        Document document = Document::createDefault(QSize(100, 100));
        document.background = Qt::transparent;
        document.wobbleAmount = 0.0;
        Stroke stroke;
        stroke.width = 4.0;
        stroke.points = {
            {QPointF(10.0, 50.0), 1.0}, {QPointF(90.0, 50.0), 1.0}};
        const QUuid strokeId = stroke.id;
        const QUuid layerId = document.activeLayerId;
        document.layers.first().strokes.append(stroke);

        QImage selection(document.size, QImage::Format_Grayscale8);
        selection.fill(0);
        for (int y = 40; y <= 60; ++y)
        {
            std::fill(
                selection.scanLine(y) + 40, selection.scanLine(y) + 61, 255);
        }

        const QImage before = RenderEngine::render(document, 0);
        DocumentController controller;
        controller.loadDocument(document);
        QVERIFY(
            controller.removeSelectedContent(layerId, {strokeId}, selection));

        const Layer &trimmedLayer = controller.document().layers.first();
        QCOMPARE(trimmedLayer.strokes.size(), 2);
        const Stroke &remainder = trimmedLayer.strokes.first();
        QCOMPARE(remainder.id, strokeId);
        QCOMPARE(remainder.points, stroke.points);
        QVERIFY(remainder.clipMask.isNull());
        const Stroke &deleteOperation = trimmedLayer.strokes.last();
        QCOMPARE(deleteOperation.mode, StrokeMode::PixelSelection);
        QVERIFY(deleteOperation.pixelSelectionOp.has_value());
        QVERIFY(deleteOperation.pixelSelectionOp->clearSource);
        QVERIFY(!deleteOperation.pixelSelectionOp->drawDestination);
        const QImage after = RenderEngine::render(controller.document(), 0);
        QVERIFY(after != before);

        controller.undoStack()->undo();
        const Layer &restoredLayer = controller.document().layers.first();
        QCOMPARE(restoredLayer.strokes.size(), 1);
        QCOMPARE(restoredLayer.strokes.first().id, strokeId);
        QCOMPARE(restoredLayer.strokes.first().points, stroke.points);
        QVERIFY(restoredLayer.strokes.first().clipMask.isNull());
        QCOMPARE(RenderEngine::render(controller.document(), 0), before);

        controller.undoStack()->redo();
        const Layer &redoneLayer = controller.document().layers.first();
        QCOMPARE(redoneLayer.strokes.size(), 2);
        QCOMPARE(redoneLayer.strokes.first().id, strokeId);
        QCOMPARE(redoneLayer.strokes.first().points, stroke.points);
        QCOMPARE(redoneLayer.strokes.last().mode, StrokeMode::PixelSelection);
        QCOMPARE(RenderEngine::render(controller.document(), 0), after);
    }

    void flipsOnlySelectedContentHorizontallyAndVertically()
    {
        Document horizontalDocument = Document::createDefault(QSize(100, 100));
        horizontalDocument.background = Qt::transparent;
        horizontalDocument.wobbleAmount = 0.0;
        Stroke horizontalStroke;
        horizontalStroke.width = 4.0;
        horizontalStroke.points = {
            {QPointF(10.0, 45.0), 1.0}, {QPointF(90.0, 55.0), 1.0}};
        const QUuid horizontalId = horizontalStroke.id;
        const QUuid horizontalLayer = horizontalDocument.activeLayerId;
        horizontalDocument.layers.first().strokes.append(horizontalStroke);
        QImage horizontalSelection(
            horizontalDocument.size, QImage::Format_Grayscale8);
        horizontalSelection.fill(0);
        for (int y = 35; y < 65; ++y)
        {
            std::fill(horizontalSelection.scanLine(y) + 20,
                horizontalSelection.scanLine(y) + 41,
                255);
        }

        const QImage horizontalBefore =
            RenderEngine::render(horizontalDocument, 0);
        DocumentController horizontalController;
        horizontalController.loadDocument(horizontalDocument);
        QVERIFY(horizontalController.flipStrokes(horizontalLayer,
            {horizontalId},
            QPointF(50.0, 50.0),
            true,
            horizontalSelection));
        const Layer &horizontallyFlipped =
            horizontalController.document().layers.first();
        QCOMPARE(horizontallyFlipped.strokes.size(), 2);
        QCOMPARE(horizontallyFlipped.strokes.first().points,
            horizontalStroke.points);
        const Stroke &horizontalOperation = horizontallyFlipped.strokes.last();
        QCOMPARE(horizontalOperation.mode, StrokeMode::PixelSelection);
        QVERIFY(horizontalOperation.pixelSelectionOp.has_value());
        QVERIFY(horizontalOperation.pixelSelectionOp->clearSource);
        QVERIFY(horizontalOperation.pixelSelectionOp->drawDestination);
        QCOMPARE(horizontalOperation.pixelSelectionOp->transform.map(
                     QPointF(30.0, 50.0)),
            QPointF(70.0, 50.0));
        const QImage horizontalRendered =
            RenderEngine::render(horizontalController.document(), 0);
        QVERIFY(horizontalRendered != horizontalBefore);
        const QByteArray horizontalAfter =
            DocumentSerializer::toJson(horizontalController.document());
        QVERIFY(!horizontalAfter.isEmpty());

        horizontalController.undoStack()->undo();
        QCOMPARE(
            horizontalController.document().layers.first().strokes.size(), 1);
        QCOMPARE(horizontalController.document()
                     .layers.first()
                     .strokes.first()
                     .points,
            horizontalStroke.points);
        QVERIFY(horizontalController.document()
                .layers.first()
                .strokes.first()
                .clipMask.isNull());
        QCOMPARE(RenderEngine::render(horizontalController.document(), 0),
            horizontalBefore);
        horizontalController.undoStack()->redo();
        QCOMPARE(DocumentSerializer::toJson(horizontalController.document()),
            horizontalAfter);

        Document verticalDocument = Document::createDefault(QSize(100, 100));
        verticalDocument.background = Qt::transparent;
        verticalDocument.wobbleAmount = 0.0;
        Stroke verticalStroke;
        verticalStroke.width = 4.0;
        verticalStroke.points = {
            {QPointF(45.0, 10.0), 1.0}, {QPointF(55.0, 90.0), 1.0}};
        const QUuid verticalId = verticalStroke.id;
        const QUuid verticalLayer = verticalDocument.activeLayerId;
        verticalDocument.layers.first().strokes.append(verticalStroke);
        QImage verticalSelection(
            verticalDocument.size, QImage::Format_Grayscale8);
        verticalSelection.fill(0);
        for (int y = 20; y < 41; ++y)
        {
            std::fill(verticalSelection.scanLine(y) + 35,
                verticalSelection.scanLine(y) + 65,
                255);
        }

        const QImage verticalBefore = RenderEngine::render(verticalDocument, 0);
        DocumentController verticalController;
        verticalController.loadDocument(verticalDocument);
        QVERIFY(verticalController.flipStrokes(verticalLayer,
            {verticalId},
            QPointF(50.0, 50.0),
            false,
            verticalSelection));
        const Layer &verticallyFlipped =
            verticalController.document().layers.first();
        QCOMPARE(verticallyFlipped.strokes.size(), 2);
        QCOMPARE(
            verticallyFlipped.strokes.first().points, verticalStroke.points);
        const Stroke &verticalOperation = verticallyFlipped.strokes.last();
        QCOMPARE(verticalOperation.mode, StrokeMode::PixelSelection);
        QVERIFY(verticalOperation.pixelSelectionOp.has_value());
        QVERIFY(verticalOperation.pixelSelectionOp->clearSource);
        QVERIFY(verticalOperation.pixelSelectionOp->drawDestination);
        QCOMPARE(verticalOperation.pixelSelectionOp->transform.map(
                     QPointF(50.0, 30.0)),
            QPointF(50.0, 70.0));
        const QImage verticalRendered =
            RenderEngine::render(verticalController.document(), 0);
        QVERIFY(verticalRendered != verticalBefore);
        const QByteArray verticalAfter =
            DocumentSerializer::toJson(verticalController.document());
        QVERIFY(!verticalAfter.isEmpty());

        verticalController.undoStack()->undo();
        QCOMPARE(
            verticalController.document().layers.first().strokes.size(), 1);
        QCOMPARE(
            verticalController.document().layers.first().strokes.first().points,
            verticalStroke.points);
        QVERIFY(verticalController.document()
                .layers.first()
                .strokes.first()
                .clipMask.isNull());
        QCOMPARE(RenderEngine::render(verticalController.document(), 0),
            verticalBefore);
        verticalController.undoStack()->redo();
        QCOMPARE(DocumentSerializer::toJson(verticalController.document()),
            verticalAfter);
    }

    void movesClipMaskAlongWithFullyContainedStroke()
    {
        Document document = Document::createDefault(QSize(100, 100));
        Stroke stroke;
        stroke.width = 4.0;
        stroke.points = {
            {QPointF(45.0, 50.0), 1.0}, {QPointF(55.0, 50.0), 1.0}};
        stroke.clipMask = QImage(document.size, QImage::Format_Grayscale8);
        stroke.clipMask.fill(0);
        for (int y = 40; y <= 60; ++y)
        {
            uchar *line = stroke.clipMask.scanLine(y);
            std::fill(line + 40, line + 61, 255);
        }
        const QUuid strokeId = stroke.id;
        const QUuid layerId = document.activeLayerId;
        document.layers.first().strokes.append(stroke);

        QImage selection(document.size, QImage::Format_Grayscale8);
        selection.fill(0);
        for (int y = 30; y <= 70; ++y)
        {
            uchar *line = selection.scanLine(y);
            std::fill(line + 30, line + 71, 255);
        }

        DocumentController controller;
        controller.loadDocument(document);
        QVERIFY(controller.moveStrokes(
            layerId, {strokeId}, QPointF(20.0, 0.0), selection));

        const Layer &movedLayer = controller.document().layers.first();
        QCOMPARE(movedLayer.strokes.size(), 2);
        const Stroke &source = movedLayer.strokes.first();
        QCOMPARE(source.points, stroke.points);
        QCOMPARE(source.clipMask, stroke.clipMask);
        const Stroke &operation = movedLayer.strokes.last();
        QCOMPARE(operation.mode, StrokeMode::PixelSelection);
        QVERIFY(operation.pixelSelectionOp.has_value());
        QCOMPARE(operation.pixelSelectionOp->transform.map(QPointF(50.0, 50.0)),
            QPointF(70.0, 50.0));
    }
};

int runStrokeCommandTests(int argc, char **argv)
{
    StrokeCommandTests tests;
    return QTest::qExec(&tests, argc, argv);
}

}

#include "StrokeCommandTests.moc"

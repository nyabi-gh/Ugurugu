// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "support/RenderTestHelpers.hpp"
#include "support/RenderTestSuites.hpp"

namespace ugurugu
{

class SelectionPreviewTests final : public QObject
{
    Q_OBJECT

private slots:
    void movesFlattenedPaintEraseSelectionOverExistingPixels()
    {
        Document document = Document::createDefault(QSize(96, 48));
        document.background = Qt::transparent;
        document.animationFrames = 4;
        document.wobbleAmount = 5.0;
        Stroke sourcePaint = makeStroke(StrokeMode::Paint,
            QColor(30, 90, 220),
            18.0,
            71,
            {QPointF(14.0, 24.0), QPointF(26.0, 24.0)});
        Stroke sourceErase = makeStroke(
            StrokeMode::Erase, Qt::black, 8.0, 72, {QPointF(20.0, 24.0)});
        Stroke destinationPaint = makeStroke(StrokeMode::Paint,
            QColor(220, 50, 40),
            18.0,
            73,
            {QPointF(54.0, 24.0), QPointF(66.0, 24.0)});
        sourcePaint.brush.antialiasing = false;
        sourceErase.brush.antialiasing = false;
        destinationPaint.brush.antialiasing = false;
        document.layers.first().strokes = {
            sourcePaint, sourceErase, destinationPaint};

        const QImage selection =
            rectangularMask(document.size, QRect(4, 10, 32, 29));
        const QPoint delta(40, 0);
        QVector<QImage> beforeFrames;
        QVector<QImage> expectedFrames;
        for (int frame = 0; frame < document.animationFrames; ++frame)
        {
            const QImage before = activeLayerPixels(document, frame);
            QVERIFY(!before.isNull());
            beforeFrames.append(before);
            expectedFrames.append(
                rasterSelectionResult(before, selection, delta, true));
            QVERIFY(!expectedFrames.last().isNull());
        }
        QVERIFY(beforeFrames[0] != beforeFrames[1]);

        DocumentController controller;
        controller.loadDocument(document);
        QVERIFY(controller.moveStrokes(document.activeLayerId,
            {sourcePaint.id, sourceErase.id},
            delta,
            selection));
        for (int frame = 0; frame < document.animationFrames; ++frame)
        {
            QCOMPARE(activeLayerPixels(controller.document(), frame),
                expectedFrames[frame]);
        }

        controller.undoStack()->undo();
        for (int frame = 0; frame < document.animationFrames; ++frame)
        {
            QCOMPARE(activeLayerPixels(controller.document(), frame),
                beforeFrames[frame]);
        }
        controller.undoStack()->redo();
        for (int frame = 0; frame < document.animationFrames; ++frame)
        {
            QCOMPARE(activeLayerPixels(controller.document(), frame),
                expectedFrames[frame]);
        }
    }

    void duplicatesFlattenedPaintEraseSelectionOverExistingPixels()
    {
        Document document = Document::createDefault(QSize(96, 48));
        document.background = Qt::transparent;
        document.wobbleAmount = 0.0;
        Stroke sourcePaint = makeStroke(StrokeMode::Paint,
            QColor(35, 170, 90),
            18.0,
            81,
            {QPointF(14.0, 24.0), QPointF(26.0, 24.0)});
        Stroke sourceErase = makeStroke(
            StrokeMode::Erase, Qt::black, 8.0, 82, {QPointF(20.0, 24.0)});
        Stroke destinationPaint = makeStroke(StrokeMode::Paint,
            QColor(230, 170, 35),
            18.0,
            83,
            {QPointF(54.0, 24.0), QPointF(66.0, 24.0)});
        sourcePaint.brush.antialiasing = false;
        sourceErase.brush.antialiasing = false;
        destinationPaint.brush.antialiasing = false;
        document.layers.first().strokes = {
            sourcePaint, sourceErase, destinationPaint};

        const QImage selection =
            rectangularMask(document.size, QRect(4, 10, 32, 29));
        const QPoint delta(40, 0);
        const QImage before = activeLayerPixels(document);
        const QImage expected =
            rasterSelectionResult(before, selection, delta, false);
        QVERIFY(!before.isNull());
        QVERIFY(!expected.isNull());

        DocumentController controller;
        controller.loadDocument(document);
        QVERIFY(controller.duplicateStrokes(document.activeLayerId,
            {sourcePaint.id, sourceErase.id},
            delta,
            selection));
        QCOMPARE(activeLayerPixels(controller.document()), expected);
        QCOMPARE(activeLayerPixels(controller.document()).pixelColor(60, 24),
            destinationPaint.color);

        controller.undoStack()->undo();
        QCOMPARE(activeLayerPixels(controller.document()), before);
        controller.undoStack()->redo();
        QCOMPARE(activeLayerPixels(controller.document()), expected);
    }

    void deletesFinalPaintEraseSelectionPixels()
    {
        Document document = Document::createDefault(QSize(96, 48));
        document.background = Qt::transparent;
        document.wobbleAmount = 0.0;
        Stroke sourcePaint = makeStroke(StrokeMode::Paint,
            QColor(40, 130, 225),
            18.0,
            86,
            {QPointF(14.0, 24.0), QPointF(26.0, 24.0)});
        Stroke sourceErase = makeStroke(
            StrokeMode::Erase, Qt::black, 8.0, 87, {QPointF(20.0, 24.0)});
        Stroke outsidePaint = makeStroke(StrokeMode::Paint,
            QColor(230, 80, 120),
            18.0,
            88,
            {QPointF(54.0, 24.0), QPointF(66.0, 24.0)});
        sourcePaint.brush.antialiasing = false;
        sourceErase.brush.antialiasing = false;
        outsidePaint.brush.antialiasing = false;
        document.layers.first().strokes = {
            sourcePaint, sourceErase, outsidePaint};

        const QImage selection =
            rectangularMask(document.size, QRect(4, 10, 32, 29));
        const QImage before = activeLayerPixels(document);
        const QImage expected = clearedSelectionResult(before, selection);
        QVERIFY(!before.isNull());
        QVERIFY(!expected.isNull());

        DocumentController controller;
        controller.loadDocument(document);
        QVERIFY(controller.removeSelectedContent(document.activeLayerId,
            {sourcePaint.id, sourceErase.id},
            selection));
        QCOMPARE(activeLayerPixels(controller.document()), expected);

        controller.undoStack()->undo();
        QCOMPARE(activeLayerPixels(controller.document()), before);
        controller.undoStack()->redo();
        QCOMPARE(activeLayerPixels(controller.document()), expected);
    }

    void movesFrozenFillSelectionAsRenderedPixels()
    {
        Document document = Document::createDefault(QSize(96, 48));
        document.background = Qt::transparent;
        document.wobbleAmount = 0.0;
        Stroke sourceFill;
        sourceFill.mode = StrokeMode::Fill;
        sourceFill.color = QColor(120, 60, 220);
        sourceFill.points = {{QPointF(8.0, 8.0), 1.0}};
        sourceFill.fillMask =
            rectangularMask(document.size, QRect(8, 12, 24, 25));
        Stroke destinationPaint = makeStroke(StrokeMode::Paint,
            QColor(30, 180, 160),
            20.0,
            91,
            {QPointF(52.0, 24.0), QPointF(68.0, 24.0)});
        destinationPaint.brush.antialiasing = false;
        document.layers.first().strokes = {sourceFill, destinationPaint};

        const QImage selection =
            rectangularMask(document.size, QRect(8, 12, 24, 25));
        const QPoint delta(40, 0);
        const QImage before = activeLayerPixels(document);
        const QImage expected =
            rasterSelectionResult(before, selection, delta, true);
        QVERIFY(!before.isNull());
        QVERIFY(!expected.isNull());

        DocumentController controller;
        controller.loadDocument(document);
        QVERIFY(controller.moveStrokes(
            document.activeLayerId, {sourceFill.id}, delta, selection));
        QCOMPARE(activeLayerPixels(controller.document()), expected);

        controller.undoStack()->undo();
        QCOMPARE(activeLayerPixels(controller.document()), before);
        controller.undoStack()->redo();
        QCOMPARE(activeLayerPixels(controller.document()), expected);
    }

    void movesOverlappingSelectionFromAnImmutableSnapshot()
    {
        Document document = Document::createDefault(QSize(80, 48));
        document.background = Qt::transparent;
        document.wobbleAmount = 0.0;
        Stroke paint = makeStroke(StrokeMode::Paint,
            QColor(35, 110, 225),
            20.0,
            101,
            {QPointF(10.0, 24.0), QPointF(44.0, 24.0)});
        Stroke erase = makeStroke(StrokeMode::Erase,
            Qt::black,
            8.0,
            102,
            {QPointF(20.0, 24.0), QPointF(25.0, 24.0)});
        paint.brush.antialiasing = false;
        erase.brush.antialiasing = false;
        document.layers.first().strokes = {paint, erase};

        const QImage selection =
            rectangularMask(document.size, QRect(2, 10, 46, 29));
        const QPoint delta(14, 0);
        const QImage before = activeLayerPixels(document);
        const QImage expected =
            rasterSelectionResult(before, selection, delta, true);
        QVERIFY(!before.isNull());
        QVERIFY(!expected.isNull());

        DocumentController controller;
        controller.loadDocument(document);
        QVERIFY(controller.moveStrokes(
            document.activeLayerId, {paint.id, erase.id}, delta, selection));

        const QImage actual = activeLayerPixels(controller.document());
        QCOMPARE(actual, expected);
        QCOMPARE(actual.pixel(42, 24), expected.pixel(42, 24));
        QCOMPARE(actual.pixelColor(5, 24), QColor(Qt::transparent));

        controller.undoStack()->undo();
        QCOMPARE(activeLayerPixels(controller.document()), before);
        controller.undoStack()->redo();
        QCOMPARE(activeLayerPixels(controller.document()), expected);
    }

    void laterPaintAndEraseStayAboveCommittedSelectionPixels()
    {
        Document document = Document::createDefault(QSize(112, 64));
        document.background = Qt::transparent;
        document.animationFrames = 8;
        document.wobbleAmount = 5.0;
        Stroke sourcePaint = makeStroke(StrokeMode::Paint,
            QColor(40, 110, 225),
            20.0,
            111,
            {QPointF(12.0, 30.0), QPointF(34.0, 25.0)});
        Stroke sourceErase = makeStroke(
            StrokeMode::Erase, Qt::black, 7.0, 112, {QPointF(22.0, 28.0)});
        Stroke existingDestination = makeStroke(StrokeMode::Paint,
            QColor(225, 70, 55),
            22.0,
            113,
            {QPointF(64.0, 30.0), QPointF(88.0, 30.0)});
        Stroke laterPaint = makeStroke(StrokeMode::Paint,
            QColor(250, 205, 45),
            9.0,
            114,
            {QPointF(58.0, 22.0), QPointF(94.0, 38.0)});
        Stroke laterErase = makeStroke(StrokeMode::Erase,
            Qt::black,
            5.0,
            115,
            {QPointF(78.0, 20.0), QPointF(78.0, 42.0)});
        for (Stroke *stroke : {&sourcePaint,
                 &sourceErase,
                 &existingDestination,
                 &laterPaint,
                 &laterErase})
        {
            stroke->brush.antialiasing = false;
        }
        document.layers.first().strokes = {
            sourcePaint, sourceErase, existingDestination};

        const QImage selection =
            rectangularMask(document.size, QRect(2, 10, 42, 41));
        const QPoint delta(52, 0);
        QVector<QImage> beforeFrames;
        QVector<QImage> movedFrames;
        QVector<QImage> expectedFrames;
        beforeFrames.reserve(document.animationFrames);
        movedFrames.reserve(document.animationFrames);
        expectedFrames.reserve(document.animationFrames);
        for (int frame = 0; frame < document.animationFrames; ++frame)
        {
            const QImage before = activeLayerPixels(document, frame);
            const QImage moved =
                rasterSelectionResult(before, selection, delta, true);
            const QImage expected = renderAdditionalStrokes(
                moved, document, {laterPaint, laterErase}, frame);
            QVERIFY(!before.isNull());
            QVERIFY(!moved.isNull());
            QVERIFY(!expected.isNull());
            beforeFrames.append(before);
            movedFrames.append(moved);
            expectedFrames.append(expected);
        }

        DocumentController controller;
        controller.loadDocument(document);
        QVERIFY(controller.moveStrokes(document.activeLayerId,
            {sourcePaint.id, sourceErase.id},
            delta,
            selection));
        controller.addStroke(document.activeLayerId, laterPaint);
        controller.addStroke(document.activeLayerId, laterErase);

        for (int frame = 0; frame < document.animationFrames; ++frame)
        {
            QCOMPARE(activeLayerPixels(controller.document(), frame),
                expectedFrames[frame]);
        }

        controller.undoStack()->undo();
        controller.undoStack()->undo();
        for (int frame = 0; frame < document.animationFrames; ++frame)
        {
            QCOMPARE(activeLayerPixels(controller.document(), frame),
                movedFrames[frame]);
        }
        controller.undoStack()->undo();
        for (int frame = 0; frame < document.animationFrames; ++frame)
        {
            QCOMPARE(activeLayerPixels(controller.document(), frame),
                beforeFrames[frame]);
        }
    }

    void appliesSequentialSelectionTransformsAsSeparateRasterOperations()
    {
        Document document = Document::createDefault(QSize(112, 56));
        document.background = Qt::transparent;
        document.animationFrames = 5;
        document.wobbleAmount = 3.0;
        Stroke blue = makeStroke(StrokeMode::Paint,
            QColor(35, 95, 220),
            15.0,
            121,
            {QPointF(10.0, 18.0), QPointF(30.0, 18.0)});
        Stroke green = makeStroke(StrokeMode::Paint,
            QColor(45, 190, 105),
            9.0,
            122,
            {QPointF(12.0, 34.0), QPointF(22.0, 27.0)});
        Stroke erase = makeStroke(
            StrokeMode::Erase, Qt::black, 5.0, 123, {QPointF(16.0, 18.0)});
        Stroke destination = makeStroke(StrokeMode::Paint,
            QColor(225, 65, 75),
            25.0,
            124,
            {QPointF(60.0, 28.0), QPointF(90.0, 28.0)});
        for (Stroke *stroke : {&blue, &green, &erase, &destination})
        {
            stroke->brush.antialiasing = false;
        }
        document.layers.first().strokes = {blue, green, erase, destination};

        const QRect sourceRect(2, 8, 38, 39);
        const QImage firstSelection =
            rectangularMask(document.size, sourceRect);
        const QPoint delta(52, 0);
        QTransform moveTransform;
        moveTransform.translate(delta.x(), delta.y());
        const QImage secondSelection =
            transformedSelectionMask(firstSelection, moveTransform);
        QVERIFY(!secondSelection.isNull());
        const QPointF flipCenter =
            QRectF(sourceRect.translated(delta)).center();
        QTransform flipTransform;
        flipTransform.translate(flipCenter.x(), flipCenter.y());
        flipTransform.scale(-1.0, 1.0);
        flipTransform.translate(-flipCenter.x(), -flipCenter.y());

        QVector<QImage> beforeFrames;
        QVector<QImage> afterMoveFrames;
        QVector<QImage> afterFlipFrames;
        for (int frame = 0; frame < document.animationFrames; ++frame)
        {
            const QImage before = activeLayerPixels(document, frame);
            const QImage afterMove = rasterSelectionTransformResult(
                before, firstSelection, moveTransform, true);
            const QImage afterFlip = rasterSelectionTransformResult(
                afterMove, secondSelection, flipTransform, true);
            QVERIFY(!before.isNull());
            QVERIFY(!afterMove.isNull());
            QVERIFY(!afterFlip.isNull());
            beforeFrames.append(before);
            afterMoveFrames.append(afterMove);
            afterFlipFrames.append(afterFlip);
        }

        const QVector<QUuid> sourceIds = {blue.id, green.id, erase.id};
        DocumentController controller;
        controller.loadDocument(document);
        QVERIFY(controller.moveStrokes(
            document.activeLayerId, sourceIds, delta, firstSelection));
        QVERIFY(controller.flipStrokes(document.activeLayerId,
            sourceIds,
            flipCenter,
            true,
            secondSelection));
        for (int frame = 0; frame < document.animationFrames; ++frame)
        {
            QCOMPARE(activeLayerPixels(controller.document(), frame),
                afterFlipFrames[frame]);
        }

        controller.undoStack()->undo();
        for (int frame = 0; frame < document.animationFrames; ++frame)
        {
            QCOMPARE(activeLayerPixels(controller.document(), frame),
                afterMoveFrames[frame]);
        }
        controller.undoStack()->undo();
        for (int frame = 0; frame < document.animationFrames; ++frame)
        {
            QCOMPARE(activeLayerPixels(controller.document(), frame),
                beforeFrames[frame]);
        }
        controller.undoStack()->redo();
        controller.undoStack()->redo();
        for (int frame = 0; frame < document.animationFrames; ++frame)
        {
            QCOMPARE(activeLayerPixels(controller.document(), frame),
                afterFlipFrames[frame]);
        }
    }

    void replaysSelectionTransformForEveryConfiguredAnimationFrame()
    {
        Document document = Document::createDefault(QSize(88, 48));
        document.background = Qt::transparent;
        document.animationFrames = DocumentLimits::maximumAnimationFrames;
        document.wobbleAmount = 8.0;
        Stroke animated = makeStroke(StrokeMode::Paint,
            QColor(85, 55, 220),
            13.0,
            131,
            {QPointF(7.0, 13.0), QPointF(18.0, 35.0), QPointF(32.0, 17.0)});
        animated.brush.antialiasing = false;
        animated.brush.animatedJitter = true;
        document.layers.first().strokes = {animated};

        const QImage selection =
            rectangularMask(document.size, QRect(0, 4, 40, 40));
        const QPoint delta(42, 0);
        QVector<QImage> expectedFrames;
        expectedFrames.reserve(document.animationFrames);
        bool sawAnimation = false;
        QImage firstBefore;
        for (int frame = 0; frame < document.animationFrames; ++frame)
        {
            const QImage before = activeLayerPixels(document, frame);
            if (frame == 0)
            {
                firstBefore = before;
            }
            else if (before != firstBefore)
            {
                sawAnimation = true;
            }
            expectedFrames.append(
                rasterSelectionResult(before, selection, delta, true));
            QVERIFY(!expectedFrames.last().isNull());
        }
        QVERIFY(sawAnimation);

        DocumentController controller;
        controller.loadDocument(document);
        QVERIFY(controller.moveStrokes(
            document.activeLayerId, {animated.id}, delta, selection));
        QCOMPARE(controller.document().animationFrames,
            DocumentLimits::maximumAnimationFrames);
        for (int frame = 0; frame < document.animationFrames; ++frame)
        {
            QCOMPARE(activeLayerPixels(controller.document(), frame),
                expectedFrames[frame]);
        }
    }

    void nonUniformImageResizeScalesCommittedLayerPixelsExactly()
    {
        Document document = Document::createDefault(QSize(73, 47));
        document.background = Qt::transparent;
        document.animationFrames = 6;
        document.wobbleAmount = 4.0;
        Stroke paint = makeStroke(StrokeMode::Paint,
            QColor(30, 135, 225),
            11.0,
            141,
            {QPointF(6.0, 8.0), QPointF(31.0, 38.0), QPointF(61.0, 13.0)});
        Stroke erase = makeStroke(StrokeMode::Erase,
            Qt::black,
            5.0,
            142,
            {QPointF(28.0, 29.0), QPointF(42.0, 23.0)});
        Stroke fill;
        fill.mode = StrokeMode::Fill;
        fill.color = QColor(235, 180, 40);
        fill.points = {{QPointF(4.0, 4.0), 1.0}};
        fill.fillMask = rectangularMask(document.size, QRect(2, 2, 12, 9));
        paint.brush.antialiasing = false;
        erase.brush.antialiasing = false;
        document.layers.first().strokes = {paint, erase, fill};

        const QSize targetSize(119, 61);
        QVector<QImage> beforeFrames;
        QVector<QImage> expectedFrames;
        for (int frame = 0; frame < document.animationFrames; ++frame)
        {
            const QImage before = activeLayerPixels(document, frame);
            const QImage expected =
                resizedRasterResult(before, targetSize, true);
            QVERIFY(!before.isNull());
            QVERIFY(!expected.isNull());
            beforeFrames.append(before);
            expectedFrames.append(expected);
        }

        DocumentController controller;
        controller.loadDocument(document);
        QVERIFY(controller.resizeImage(targetSize));
        QCOMPARE(controller.document().size, targetSize);
        for (int frame = 0; frame < document.animationFrames; ++frame)
        {
            QCOMPARE(activeLayerPixels(controller.document(), frame),
                expectedFrames[frame]);
        }

        controller.undoStack()->undo();
        QCOMPARE(controller.document().size, document.size);
        for (int frame = 0; frame < document.animationFrames; ++frame)
        {
            QCOMPARE(activeLayerPixels(controller.document(), frame),
                beforeFrames[frame]);
        }
        controller.undoStack()->redo();
        QCOMPARE(controller.document().size, targetSize);
        for (int frame = 0; frame < document.animationFrames; ++frame)
        {
            QCOMPARE(activeLayerPixels(controller.document(), frame),
                expectedFrames[frame]);
        }
    }

    void canvasCropThenExpandKeepsPreexistingSelectionEditClipped()
    {
        Document document = Document::createDefault(QSize(96, 48));
        document.background = Qt::transparent;
        document.animationFrames = 4;
        document.wobbleAmount = 3.0;
        Stroke paint = makeStroke(StrokeMode::Paint,
            QColor(50, 120, 230),
            17.0,
            151,
            {QPointF(8.0, 24.0), QPointF(30.0, 24.0)});
        Stroke erase = makeStroke(
            StrokeMode::Erase, Qt::black, 5.0, 152, {QPointF(17.0, 24.0)});
        paint.brush.antialiasing = false;
        erase.brush.antialiasing = false;
        document.layers.first().strokes = {paint, erase};

        const QImage selection =
            rectangularMask(document.size, QRect(0, 10, 40, 29));
        const QPoint moveDelta(48, 0);
        const QSize croppedSize(68, 48);
        const QPoint cropOffset(0, 0);
        const QSize expandedSize(96, 48);
        const QPoint expandOffset(0, 0);
        QVector<QImage> movedFrames;
        QVector<QImage> croppedFrames;
        QVector<QImage> expandedFrames;
        for (int frame = 0; frame < document.animationFrames; ++frame)
        {
            const QImage before = activeLayerPixels(document, frame);
            const QImage moved =
                rasterSelectionResult(before, selection, moveDelta, true);
            const QImage cropped =
                reframedRasterResult(moved, croppedSize, cropOffset);
            const QImage expanded =
                reframedRasterResult(cropped, expandedSize, expandOffset);
            QVERIFY(!moved.isNull());
            QVERIFY(!cropped.isNull());
            QVERIFY(!expanded.isNull());
            movedFrames.append(moved);
            croppedFrames.append(cropped);
            expandedFrames.append(expanded);
        }

        DocumentController controller;
        controller.loadDocument(document);
        QVERIFY(controller.moveStrokes(document.activeLayerId,
            {paint.id, erase.id},
            moveDelta,
            selection));
        for (int frame = 0; frame < document.animationFrames; ++frame)
        {
            QCOMPARE(activeLayerPixels(controller.document(), frame),
                movedFrames[frame]);
        }

        QVERIFY(controller.resizeCanvas(croppedSize, cropOffset));
        for (int frame = 0; frame < document.animationFrames; ++frame)
        {
            QCOMPARE(activeLayerPixels(controller.document(), frame),
                croppedFrames[frame]);
        }

        QVERIFY(controller.resizeCanvas(expandedSize, expandOffset));
        for (int frame = 0; frame < document.animationFrames; ++frame)
        {
            const QImage actual =
                activeLayerPixels(controller.document(), frame);
            QCOMPARE(actual, expandedFrames[frame]);
            QCOMPARE(actual.pixelColor(80, 24), QColor(Qt::transparent));
        }

        controller.undoStack()->undo();
        for (int frame = 0; frame < document.animationFrames; ++frame)
        {
            QCOMPARE(activeLayerPixels(controller.document(), frame),
                croppedFrames[frame]);
        }
        controller.undoStack()->undo();
        for (int frame = 0; frame < document.animationFrames; ++frame)
        {
            QCOMPARE(activeLayerPixels(controller.document(), frame),
                movedFrames[frame]);
        }
    }

    void full4kSelectionUsesPackedStorageAndBoundedFrameRendering()
    {
        constexpr int edge = DocumentLimits::maximumCanvasEdge;
        const QSize canvasSize(edge, edge);
        Document document = Document::createDefault(canvasSize);
        document.background = Qt::transparent;
        document.animationFrames = DocumentLimits::maximumAnimationFrames;
        document.wobbleAmount = 5.0;
        Stroke paint = makeStroke(StrokeMode::Paint,
            QColor(45, 105, 225),
            5.0,
            161,
            {QPointF(16.0, edge / 2.0), QPointF(edge - 16.0, edge / 2.0)});
        paint.brush.antialiasing = false;
        document.layers.first().strokes = {paint};

        QImage selection(canvasSize, QImage::Format_Grayscale8);
        QVERIFY(!selection.isNull());
        quint32 random = 0x6d2b79f5U;
        for (int y = 0; y < edge; ++y)
        {
            uchar *line = selection.scanLine(y);
            for (int x = 0; x < edge; ++x)
            {
                random ^= random << 13U;
                random ^= random >> 17U;
                random ^= random << 5U;
                line[x] = (random & 1U) != 0U ? 255 : 0;
            }
        }
        selection.scanLine(0)[0] = 255;
        selection.scanLine(edge - 1)[edge - 1] = 255;

        DocumentController controller;
        controller.loadDocument(document);
        QVERIFY(controller.moveStrokes(
            document.activeLayerId, {paint.id}, QPointF(0.0, 1.0), selection));

        const Layer &layer = controller.document().layers.first();
        QVERIFY(!layer.strokes.isEmpty());
        const Stroke &operation = layer.strokes.last();
        QCOMPARE(operation.mode, StrokeMode::PixelSelection);
        QVERIFY(operation.pixelSelectionOp.has_value());
        const PixelSelectionOp &pixelOperation = *operation.pixelSelectionOp;
        QCOMPARE(pixelOperation.canvasSize, canvasSize);
        QCOMPARE(pixelOperation.sourceBounds, QRect(QPoint(), canvasSize));
        const qsizetype expectedStride = (static_cast<qsizetype>(edge) + 7) / 8;
        const qsizetype expectedPackedBytes = expectedStride * edge;
        QCOMPARE(expectedPackedBytes, qsizetype(2 * 1024 * 1024));
        QCOMPARE(pixelOperation.packedMask.size(), expectedPackedBytes);
        QCOMPARE(packedSelectionBytes(controller.document()),
            quint64(expectedPackedBytes));
        QVERIFY(packedSelectionBytes(controller.document())
                <= DocumentLimits::maximumDistinctClipMaskBytes);

        selection = {};
        const quint64 expectedFrameBytes = static_cast<quint64>(edge)
                                           * static_cast<quint64>(edge)
                                           * sizeof(QRgb);
        QCOMPARE(expectedFrameBytes, quint64(64) * 1024ULL * 1024ULL);
        QImage rendered = activeLayerPixels(controller.document(), 0);
        QVERIFY(!rendered.isNull());
        QCOMPARE(rendered.size(), canvasSize);
        QCOMPARE(
            static_cast<quint64>(rendered.sizeInBytes()), expectedFrameBytes);
        rendered = {};
        rendered = activeLayerPixels(
            controller.document(), DocumentLimits::maximumAnimationFrames - 1);
        QVERIFY(!rendered.isNull());
        QCOMPARE(rendered.size(), canvasSize);
        QCOMPARE(
            static_cast<quint64>(rendered.sizeInBytes()), expectedFrameBytes);
    }
};

int runSelectionPreviewTests(int argc, char **argv)
{
    SelectionPreviewTests tests;
    return QTest::qExec(&tests, argc, argv);
}

}

#include "SelectionPreviewTests.moc"

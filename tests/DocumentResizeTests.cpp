// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "support/DocumentTestHelpers.hpp"
#include "support/DocumentTestSuites.hpp"

namespace ugurugu
{

class DocumentResizeTests final : public QObject
{
    Q_OBJECT

private slots:
    void resizesImageUndoably()
    {
        Document document = Document::createDefault(QSize(100, 50));
        Stroke stroke;
        stroke.width = 10.0;
        stroke.points = {
            {QPointF(10.0, 10.0), 0.5}, {QPointF(90.0, 40.0), 1.0}};
        stroke.clipMask = QImage(document.size, QImage::Format_Grayscale8);
        stroke.clipMask.fill(255);
        document.layers.first().strokes.append(stroke);

        DocumentController controller;
        controller.loadDocument(document);
        QVERIFY(controller.resizeImage(QSize(200, 100)));
        QCOMPARE(controller.document().size, QSize(200, 100));
        const Layer &resizedLayer = controller.document().layers.first();
        QCOMPARE(resizedLayer.strokes.size(), 2);
        QCOMPARE(resizedLayer.strokes.first().points, stroke.points);
        QCOMPARE(resizedLayer.strokes.first().width, stroke.width);
        QCOMPARE(resizedLayer.strokes.first().clipMask, stroke.clipMask);
        const Stroke &resizeOperation = resizedLayer.strokes.last();
        QCOMPARE(resizeOperation.mode, StrokeMode::Reframe);
        QVERIFY(resizeOperation.reframeOp.has_value());
        QCOMPARE(resizeOperation.reframeOp->mode, ReframeMode::Image);
        QCOMPARE(resizeOperation.reframeOp->sourceSize, QSize(100, 50));
        QCOMPARE(resizeOperation.reframeOp->targetSize, QSize(200, 100));
        QCOMPARE(resizeOperation.reframeOp->sampling, SamplingMode::Smooth);

        controller.undoStack()->undo();
        QCOMPARE(controller.document().size, QSize(100, 50));
        const Stroke &restored =
            controller.document().layers.first().strokes.first();
        QCOMPARE(restored.points.first().position, QPointF(10.0, 10.0));
        QCOMPARE(restored.points.last().position, QPointF(90.0, 40.0));
        QCOMPARE(restored.width, 10.0);
        QCOMPARE(restored.clipMask, stroke.clipMask);

        controller.undoStack()->redo();
        QCOMPARE(controller.document().size, QSize(200, 100));
        QCOMPARE(controller.document().layers.first().strokes.size(), 2);
        QVERIFY(!controller.resizeImage(QSize(0, 100)));
    }

    void keepsEdgePointsSafeWhenResizingImage()
    {
        Document document = Document::createDefault(QSize(100, 100));
        Stroke stroke;
        stroke.width = 6.0;
        stroke.points = {
            {QPointF(50.0, 50.0), 1.0}, {QPointF(100.0, 100.0), 1.0}};
        document.layers.first().strokes.append(stroke);

        DocumentController controller;
        controller.loadDocument(document);
        QVERIFY(controller.resizeImage(QSize(109, 109)));

        const QSize size = controller.document().size;
        for (const StrokePoint &point :
            controller.document().layers.first().strokes.first().points)
        {
            QVERIFY(point.position.x() >= 0.0);
            QVERIFY(point.position.y() >= 0.0);
            QVERIFY(point.position.x() <= size.width());
            QVERIFY(point.position.y() <= size.height());
        }

        QTemporaryDir directory;
        QVERIFY(directory.isValid());
        const QString filePath =
            directory.filePath(QStringLiteral("resized.ugu"));
        QString error;
        QVERIFY2(
            DocumentSerializer::save(filePath, controller.document(), &error),
            qPrintable(error));
    }

    void resizesCanvasByTranslatingContentUndoably()
    {
        Document document = Document::createDefault(QSize(100, 80));
        Stroke first;
        first.width = 10.0;
        first.points = {{QPointF(10.0, 20.0), 0.5}, {QPointF(90.0, 60.0), 1.0}};
        Stroke second;
        second.width = 4.0;
        second.points = {
            {QPointF(25.0, 35.0), 1.0}, {QPointF(50.0, 45.0), 1.0}};
        document.layers.first().strokes = {first, second};

        DocumentController controller;
        controller.loadDocument(document);
        QSignalSpy resizeSpy(&controller, &DocumentController::canvasResized);

        const QSize targetSize(120, 100);
        const QPoint offset(10, 15);
        QVERIFY(controller.resizeCanvas(targetSize, offset));

        QCOMPARE(controller.document().size, targetSize);
        const QVector<Stroke> &resized =
            controller.document().layers.first().strokes;
        QCOMPARE(resized.size(), 3);
        QCOMPARE(resized[0].points, first.points);
        QCOMPARE(resized[1].points, second.points);
        QCOMPARE(resized[0].width, first.width);
        QCOMPARE(resized[1].width, second.width);
        QVERIFY(resized[0].clipMask.isNull());
        QVERIFY(resized[1].clipMask.isNull());
        QCOMPARE(resized.last().mode, StrokeMode::Reframe);
        QVERIFY(resized.last().reframeOp.has_value());
        QCOMPARE(resized.last().reframeOp->mode, ReframeMode::Canvas);
        QCOMPARE(resized.last().reframeOp->sourceSize, document.size);
        QCOMPARE(resized.last().reframeOp->targetSize, targetSize);
        QCOMPARE(resized.last().reframeOp->contentOffset, offset);

        QCOMPARE(resizeSpy.size(), 1);
        QCOMPARE(resizeSpy[0][0].toSize(), document.size);
        QCOMPARE(resizeSpy[0][1].toSize(), targetSize);
        const QTransform forward = qvariant_cast<QTransform>(resizeSpy[0][2]);
        QCOMPARE(forward.map(QPointF(3.0, 4.0)), QPointF(13.0, 19.0));

        controller.undoStack()->undo();
        QCOMPARE(controller.document().size, document.size);
        const QVector<Stroke> &restored =
            controller.document().layers.first().strokes;
        QCOMPARE(restored[0].points, first.points);
        QCOMPARE(restored[1].points, second.points);
        QVERIFY(restored[0].clipMask.isNull());
        QVERIFY(restored[1].clipMask.isNull());
        QCOMPARE(resizeSpy.size(), 2);
        const QTransform inverse = qvariant_cast<QTransform>(resizeSpy[1][2]);
        QCOMPARE(inverse.map(QPointF(13.0, 19.0)), QPointF(3.0, 4.0));

        controller.undoStack()->redo();
        QCOMPARE(controller.document().size, targetSize);
        QCOMPARE(controller.document().layers.first().strokes[0].points,
            first.points);
        QCOMPARE(controller.document().layers.first().strokes.size(), 3);
    }

    void supportsSameSizeCanvasOffsetsAndOffCanvasRoundTrip()
    {
        Document document = Document::createDefault(QSize(100, 80));
        Stroke stroke;
        stroke.width = 7.0;
        stroke.points = {{QPointF(5.0, 20.0), 1.0}, {QPointF(40.0, 30.0), 1.0}};
        document.layers.first().strokes.append(stroke);

        DocumentController controller;
        controller.loadDocument(document);
        const int undoCount = controller.undoStack()->count();
        QVERIFY(!controller.resizeCanvas(document.size, QPoint()));
        QCOMPARE(controller.undoStack()->count(), undoCount);

        QVERIFY(controller.resizeCanvas(document.size, QPoint(-20, 5)));
        const Layer &translatedLayer = controller.document().layers.first();
        QCOMPARE(translatedLayer.strokes.size(), 2);
        QCOMPARE(translatedLayer.strokes.first().points, stroke.points);
        QCOMPARE(translatedLayer.strokes.first().width, stroke.width);
        QVERIFY(translatedLayer.strokes.first().clipMask.isNull());
        const Stroke &translated = translatedLayer.strokes.last();
        QCOMPARE(translated.mode, StrokeMode::Reframe);
        QVERIFY(translated.reframeOp.has_value());
        QCOMPARE(translated.reframeOp->contentOffset, QPoint(-20, 5));

        QString error;
        const QByteArray json =
            DocumentSerializer::toJson(controller.document());
        QVERIFY(!json.isEmpty());
        QCOMPARE(QJsonDocument::fromJson(json)
                     .object()
                     .value(QStringLiteral("schemaVersion"))
                     .toInt(),
            13);
        const std::optional<Document> loaded =
            DocumentSerializer::fromJson(json, &error);
        QVERIFY2(loaded.has_value(), qPrintable(error));
        QCOMPARE(loaded->layers.first().strokes.first().points, stroke.points);
        QCOMPARE(loaded->layers.first().strokes.last().reframeOp,
            translated.reframeOp);

        QTemporaryDir directory;
        QVERIFY(directory.isValid());
        const QString path =
            directory.filePath(QStringLiteral("off-canvas.ugu"));
        QVERIFY2(DocumentSerializer::save(path, controller.document(), &error),
            qPrintable(error));
        const std::optional<Document> loadedFile =
            DocumentSerializer::load(path, &error);
        QVERIFY2(loadedFile.has_value(), qPrintable(error));
        QCOMPARE(
            loadedFile->layers.first().strokes.first().points, stroke.points);
    }

    void imageResizePreservesHiddenOffCanvasGeometry()
    {
        Document document = Document::createDefault(QSize(100, 80));
        Stroke stroke;
        stroke.width = 6.0;
        stroke.points = {{QPointF(5.0, 20.0), 1.0}, {QPointF(40.0, 30.0), 1.0}};
        document.layers.first().strokes.append(stroke);

        DocumentController controller;
        controller.loadDocument(document);
        QVERIFY(controller.resizeCanvas(document.size, QPoint(-20, 0)));
        QCOMPARE(controller.document()
                     .layers.first()
                     .strokes.first()
                     .points.first()
                     .position,
            QPointF(5.0, 20.0));

        QVERIFY(controller.resizeImage(QSize(200, 160)));
        const Layer &scaledLayer = controller.document().layers.first();
        QCOMPARE(scaledLayer.strokes.size(), 3);
        const Stroke &sourceStroke = scaledLayer.strokes.first();
        QCOMPARE(sourceStroke.points, stroke.points);
        QCOMPARE(sourceStroke.width, stroke.width);
        QVERIFY(sourceStroke.clipMask.isNull());
        QCOMPARE(
            scaledLayer.strokes[1].reframeOp->contentOffset, QPoint(-20, 0));
        QCOMPARE(scaledLayer.strokes[2].reframeOp->mode, ReframeMode::Image);
    }

    void imageResizePreservesInsertedImageLayersAndHistory()
    {
        DocumentController controller;
        controller.newDocument(QSize(64, 48));
        QImage image(QSize(8, 6), QImage::Format_RGBA8888);
        image.fill(Qt::transparent);
        image.setPixelColor(1, 1, QColor(230, 40, 20, 255));
        image.setPixelColor(6, 4, QColor(20, 170, 80, 220));
        QCOMPARE(controller.insertImage(image, QStringLiteral("image.png")),
            DocumentController::InsertImageResult::Inserted);
        const QUuid layerId = controller.document().activeLayerId;
        const QString assetId = controller.document()
                                    .layer(layerId)
                                    ->strokes.first()
                                    .imageOp->assetId;
        const QImage before = RenderEngine::render(controller.document(), 0);

        QVERIFY(controller.resizeImage(QSize(128, 96)));
        const Layer *resized = controller.document().layer(layerId);
        QVERIFY(resized);
        QCOMPARE(resized->strokes.size(), 2);
        QCOMPARE(resized->strokes.first().mode, StrokeMode::Image);
        QCOMPARE(resized->strokes.last().mode, StrokeMode::Reframe);
        QVERIFY(resized->strokes.last().reframeOp.has_value());
        QCOMPARE(resized->strokes.last().reframeOp->mode, ReframeMode::Image);
        QVERIFY(controller.document().rasterAssets.contains(assetId));
        const QImage after = RenderEngine::render(controller.document(), 0);
        QVERIFY(!after.isNull());

        controller.undoStack()->undo();
        QCOMPARE(RenderEngine::render(controller.document(), 0), before);
        QVERIFY(controller.document().rasterAssets.contains(assetId));
        controller.undoStack()->redo();
        QCOMPARE(RenderEngine::render(controller.document(), 0), after);
        QVERIFY(controller.document().rasterAssets.contains(assetId));
    }

    void canvasResizePreservesInsertedImageLayersAndHistory()
    {
        DocumentController controller;
        controller.newDocument(QSize(64, 48));
        QImage image(QSize(8, 6), QImage::Format_RGBA8888);
        image.fill(QColor(40, 100, 220, 255));
        QCOMPARE(controller.insertImage(image, QStringLiteral("image.png")),
            DocumentController::InsertImageResult::Inserted);
        const QUuid layerId = controller.document().activeLayerId;
        const QString assetId = controller.document()
                                    .layer(layerId)
                                    ->strokes.first()
                                    .imageOp->assetId;
        const QImage before = RenderEngine::render(controller.document(), 0);

        QVERIFY(controller.resizeCanvas(QSize(80, 60), QPoint(7, 5)));
        const Layer *resized = controller.document().layer(layerId);
        QVERIFY(resized);
        QCOMPARE(resized->strokes.size(), 2);
        QCOMPARE(resized->strokes.first().mode, StrokeMode::Image);
        QCOMPARE(resized->strokes.last().mode, StrokeMode::Reframe);
        QVERIFY(resized->strokes.last().reframeOp.has_value());
        QCOMPARE(resized->strokes.last().reframeOp->mode, ReframeMode::Canvas);
        QCOMPARE(
            resized->strokes.last().reframeOp->contentOffset, QPoint(7, 5));
        QVERIFY(controller.document().rasterAssets.contains(assetId));
        const QImage after = RenderEngine::render(controller.document(), 0);
        QVERIFY(!after.isNull());

        controller.undoStack()->undo();
        QCOMPARE(RenderEngine::render(controller.document(), 0), before);
        QVERIFY(controller.document().rasterAssets.contains(assetId));
        controller.undoStack()->redo();
        QCOMPARE(RenderEngine::render(controller.document(), 0), after);
        QVERIFY(controller.document().rasterAssets.contains(assetId));
    }

    void resizesCanvasWithoutLayers()
    {
        Document document = Document::createDefault(QSize(90, 70));
        document.layers.clear();
        document.activeLayerId = QUuid();

        DocumentController controller;
        controller.loadDocument(document);
        QVERIFY(controller.resizeCanvas(QSize(120, 100), QPoint(12, 8)));
        QCOMPARE(controller.document().size, QSize(120, 100));
        QVERIFY(controller.document().layers.isEmpty());
        QVERIFY(controller.document().activeLayerId.isNull());

        controller.undoStack()->undo();
        QCOMPARE(controller.document().size, document.size);
        QVERIFY(controller.document().layers.isEmpty());
        QVERIFY(controller.document().activeLayerId.isNull());
    }

    void canvasCropMatchesTranslatedPaintRendering()
    {
        Document document = Document::createDefault(QSize(64, 64));
        document.background = Qt::transparent;
        document.wobbleAmount = 0.0;
        Stroke stroke;
        stroke.seed = 42;
        stroke.width = 8.0;
        stroke.brush.antialiasing = false;
        stroke.points = {{QPointF(5.0, 30.0), 1.0}, {QPointF(55.0, 30.0), 1.0}};
        document.layers.first().strokes.append(stroke);

        const QImage before = RenderEngine::render(document, 0);
        QVERIFY(!before.isNull());

        DocumentController controller;
        controller.loadDocument(document);
        QVERIFY(controller.resizeCanvas(QSize(40, 40), QPoint(-12, -8)));
        const Layer &croppedLayer = controller.document().layers.first();
        QCOMPARE(croppedLayer.strokes.size(), 2);
        QCOMPARE(croppedLayer.strokes.first().points, stroke.points);
        QCOMPARE(croppedLayer.strokes.first().width, stroke.width);
        QVERIFY(croppedLayer.strokes.last().reframeOp.has_value());
        QCOMPARE(croppedLayer.strokes.last().reframeOp->contentOffset,
            QPoint(-12, -8));

        const QImage after = RenderEngine::render(controller.document(), 0);
        QCOMPARE(after, before.copy(QRect(12, 8, 40, 40)));
    }

    void validatesStoredOffCanvasCoordinatesBySchema()
    {
        Document document = Document::createDefault(QSize(20, 20));
        Stroke stroke;
        stroke.points = {{QPointF(-1.0, 5.0), 1.0}, {QPointF(10.0, 5.0), 1.0}};
        document.layers.first().strokes.append(stroke);

        const QByteArray currentJson = DocumentSerializer::toJson(document);
        QVERIFY(!currentJson.isEmpty());
        QString error;
        const std::optional<Document> current =
            DocumentSerializer::fromJson(currentJson, &error);
        QVERIFY2(current.has_value(), qPrintable(error));

        QJsonObject legacyRoot = QJsonDocument::fromJson(currentJson).object();
        legacyRoot.insert(QStringLiteral("schemaVersion"), 4);
        const std::optional<Document> legacy = DocumentSerializer::fromJson(
            QJsonDocument(legacyRoot).toJson(QJsonDocument::Compact), &error);
        QVERIFY(!legacy.has_value());

        Document unsafe = document;
        unsafe.layers.first().strokes.first().points.first().position.setX(
            DocumentLimits::maximumStoredCoordinateMagnitude + 1.0);
        const QByteArray unsafeJson = DocumentSerializer::toJson(unsafe);
        QVERIFY(unsafeJson.isEmpty());

        QJsonObject unsafeRoot = QJsonDocument::fromJson(currentJson).object();
        QJsonArray layers =
            unsafeRoot.value(QStringLiteral("layers")).toArray();
        QJsonObject layer = layers.first().toObject();
        QJsonArray strokes = layer.value(QStringLiteral("strokes")).toArray();
        QJsonObject unsafeStroke = strokes.first().toObject();
        QJsonArray points =
            unsafeStroke.value(QStringLiteral("points")).toArray();
        QJsonArray unsafePoint = points.first().toArray();
        unsafePoint[0] = DocumentLimits::maximumStoredCoordinateMagnitude + 1.0;
        points[0] = unsafePoint;
        unsafeStroke.insert(QStringLiteral("points"), points);
        strokes[0] = unsafeStroke;
        layer.insert(QStringLiteral("strokes"), strokes);
        layers[0] = layer;
        unsafeRoot.insert(QStringLiteral("layers"), layers);
        const std::optional<Document> rejected = DocumentSerializer::fromJson(
            QJsonDocument(unsafeRoot).toJson(QJsonDocument::Compact), &error);
        QVERIFY(!rejected.has_value());
    }

    void rejectsCanvasResizeThatWouldExceedCoordinateLimitAtomically()
    {
        Document document = Document::createDefault(QSize(20, 20));
        Stroke stroke;
        stroke.points = {
            {QPointF(
                 DocumentLimits::maximumStoredCoordinateMagnitude - 1.0, 5.0),
                1.0}};
        document.layers.first().strokes.append(stroke);

        DocumentController controller;
        controller.loadDocument(document);
        const int undoCount = controller.undoStack()->count();
        QVERIFY(!controller.resizeCanvas(QSize(30, 30),
            QPoint(static_cast<int>(
                       DocumentLimits::maximumStoredCoordinateMagnitude)
                       + 1,
                0)));
        QCOMPARE(controller.undoStack()->count(), undoCount);
        QCOMPARE(controller.document().size, document.size);
        QCOMPARE(controller.document().layers.first().strokes.first().points,
            stroke.points);
        QVERIFY(controller.document()
                .layers.first()
                .strokes.first()
                .clipMask.isNull());
    }
};

int runDocumentResizeTests(int argc, char **argv)
{
    DocumentResizeTests tests;
    return QTest::qExec(&tests, argc, argv);
}

}

#include "DocumentResizeTests.moc"

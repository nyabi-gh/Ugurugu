// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "io/serializer/RasterAssetTable.hpp"
#include "support/DocumentTestHelpers.hpp"
#include "support/DocumentTestSuites.hpp"

namespace ugurugu
{

class DocumentSchemaTests final : public QObject
{
    Q_OBJECT

private slots:
    void roundTripsJson()
    {
        Document source = Document::createDefault(QSize(321, 123));
        source.background = QColor(12, 34, 56, 78);
        source.animationFrames = 17;
        source.framesPerSecond = 12.5;
        source.wobbleAmount = 4.25;
        source.motion.style = MotionStyle::Smooth;
        source.motion.poseCount = 7;
        source.motion.detail = 19;
        source.motion.linked = 0.4;
        source.motion.randomness = 0.65;
        source.motion.brokenLine = true;
        source.motion.breakAmount = 0.55;
        source.motion.breakRange = 42.0;

        Layer &firstLayer = source.layers.first();
        firstLayer.id =
            QUuid(QStringLiteral("{22222222-2222-2222-2222-222222222222}"));
        firstLayer.name = QStringLiteral("Ink");
        firstLayer.opacity = 0.625;
        firstLayer.blendMode = LayerBlendMode::Multiply;
        firstLayer.reference = true;

        Stroke paint;
        paint.id =
            QUuid(QStringLiteral("{33333333-3333-3333-3333-333333333333}"));
        paint.seed = 9876543210123456789ULL;
        paint.mode = StrokeMode::Paint;
        paint.color = QColor(1, 2, 3, 4);
        paint.width = 9.75;
        paint.brush =
            BrushPresetCatalog::find(QStringLiteral("soft-airbrush"))->settings;
        paint.brush.wobbleScale = 0.35;
        paint.brush.antialiasing = true;
        paint.points = {{QPointF(1.25, 2.5), 0.2}, {QPointF(30.0, 40.0), 0.9}};
        paint.clipMask = QImage(source.size, QImage::Format_Grayscale8);
        paint.clipMask.fill(0);
        for (int y = 10; y < 40; ++y)
        {
            std::fill_n(paint.clipMask.scanLine(y) + 20, 60, 255);
        }
        firstLayer.strokes.append(paint);

        Layer secondLayer;
        secondLayer.id =
            QUuid(QStringLiteral("{44444444-4444-4444-4444-444444444444}"));
        secondLayer.name = QStringLiteral("Erase");
        secondLayer.visible = false;
        secondLayer.opacity = 0.4;
        secondLayer.blendMode = LayerBlendMode::Screen;

        Stroke erase;
        erase.id =
            QUuid(QStringLiteral("{55555555-5555-5555-5555-555555555555}"));
        erase.seed = 123456789;
        erase.mode = StrokeMode::Erase;
        erase.color = QColor(200, 150, 100, 50);
        erase.width = 13.5;
        erase.points = {{QPointF(6.0, 7.0), 1.0}};
        secondLayer.strokes.append(erase);
        source.layers.append(secondLayer);
        source.activeLayerId = secondLayer.id;

        QString error;
        const std::optional<Document> loaded = DocumentSerializer::fromJson(
            DocumentSerializer::toJson(source), &error);
        QVERIFY2(loaded.has_value(), qPrintable(error));
        QCOMPARE(loaded->size, source.size);
        QCOMPARE(loaded->background, source.background);
        QCOMPARE(loaded->animationFrames, source.animationFrames);
        QCOMPARE(loaded->framesPerSecond, source.framesPerSecond);
        QCOMPARE(loaded->wobbleAmount, source.wobbleAmount);
        QVERIFY(loaded->motion == source.motion);
        QCOMPARE(loaded->activeLayerId, source.activeLayerId);
        QCOMPARE(loaded->layers.size(), source.layers.size());

        for (int layerIndex = 0; layerIndex < source.layers.size();
            ++layerIndex)
        {
            const Layer &actualLayer = loaded->layers[layerIndex];
            const Layer &expectedLayer = source.layers[layerIndex];
            QCOMPARE(actualLayer.id, expectedLayer.id);
            QCOMPARE(actualLayer.name, expectedLayer.name);
            QCOMPARE(actualLayer.visible, expectedLayer.visible);
            QCOMPARE(actualLayer.reference, expectedLayer.reference);
            QCOMPARE(actualLayer.opacity, expectedLayer.opacity);
            QCOMPARE(actualLayer.blendMode, expectedLayer.blendMode);
            QCOMPARE(actualLayer.strokes.size(), expectedLayer.strokes.size());

            for (int strokeIndex = 0;
                strokeIndex < expectedLayer.strokes.size();
                ++strokeIndex)
            {
                const Stroke &actualStroke = actualLayer.strokes[strokeIndex];
                const Stroke &expectedStroke =
                    expectedLayer.strokes[strokeIndex];
                QCOMPARE(actualStroke.id, expectedStroke.id);
                QCOMPARE(actualStroke.seed, expectedStroke.seed);
                QCOMPARE(actualStroke.mode, expectedStroke.mode);
                QCOMPARE(actualStroke.color, expectedStroke.color);
                QCOMPARE(actualStroke.width, expectedStroke.width);
                QVERIFY(actualStroke.brush == expectedStroke.brush);
                QCOMPARE(actualStroke.clipMask, expectedStroke.clipMask);
                QCOMPARE(
                    actualStroke.points.size(), expectedStroke.points.size());

                for (int pointIndex = 0;
                    pointIndex < expectedStroke.points.size();
                    ++pointIndex)
                {
                    QCOMPARE(actualStroke.points[pointIndex].position,
                        expectedStroke.points[pointIndex].position);
                    QCOMPARE(actualStroke.points[pointIndex].pressure,
                        expectedStroke.points[pointIndex].pressure);
                }
            }
        }
    }

    void rejectsIncompleteLayerWobbleOverrides()
    {
        Document source = Document::createDefault(QSize(64, 64));
        Layer &layer = source.layers.first();
        layer.wobbleAmount = 2.5;
        layer.motion = source.motion;
        const QByteArray json = DocumentSerializer::toJson(source);
        QVERIFY(!json.isEmpty());

        QJsonObject root = QJsonDocument::fromJson(json).object();
        QJsonArray layers = root.value(QStringLiteral("layers")).toArray();
        QJsonObject serializedLayer = layers.first().toObject();
        serializedLayer.remove(QStringLiteral("motion"));
        layers[0] = serializedLayer;
        root.insert(QStringLiteral("layers"), layers);
        QString error;
        QVERIFY(!DocumentSerializer::fromJson(
            QJsonDocument(root).toJson(QJsonDocument::Compact), &error));
        QVERIFY(error.contains(QStringLiteral("wobble override")));

        source.layers.first().motion.reset();
        QVERIFY(DocumentSerializer::toJson(source).isEmpty());
    }

    void roundTripsSchemaTenFrozenFillCoverage()
    {
        Document source = Document::createDefault(QSize(48, 36));
        source.background = Qt::transparent;
        QImage coverage(source.size, QImage::Format_Grayscale8);
        coverage.fill(0);
        for (int y = 8; y < 28; ++y)
        {
            std::fill(coverage.scanLine(y) + 12,
                coverage.scanLine(y) + 38,
                uchar(255));
        }
        const std::optional<PackedMaskRegion> packed = packBinaryMask(coverage);
        QVERIFY(packed.has_value());
        Stroke fill;
        fill.mode = StrokeMode::Fill;
        fill.color = QColor(25, 110, 230, 180);
        fill.brush.antialiasing = false;
        fill.points = {{QPointF(20.0, 16.0), 1.0}};
        fill.fillCoverage = packed;
        source.layers.first().strokes = {fill};

        const QByteArray json = DocumentSerializer::toJson(source);
        QVERIFY(!json.isEmpty());
        const QJsonObject root = QJsonDocument::fromJson(json).object();
        QCOMPARE(root.value(QStringLiteral("schemaVersion")).toInt(), 13);
        QCOMPARE(root.value(QStringLiteral("binaryMasks")).toArray().size(), 1);
        const QJsonObject serializedStroke =
            root.value(QStringLiteral("layers"))
                .toArray()
                .first()
                .toObject()
                .value(QStringLiteral("strokes"))
                .toArray()
                .first()
                .toObject();
        QVERIFY(serializedStroke.contains(QStringLiteral("fillCoverageId")));

        QString error;
        const std::optional<Document> loaded =
            DocumentSerializer::fromJson(json, &error);
        QVERIFY2(loaded.has_value(), qPrintable(error));
        const Stroke &loadedFill = loaded->layers.first().strokes.first();
        QVERIFY(loadedFill.fillCoverage.has_value());
        QVERIFY(loadedFill.fillCoverage == fill.fillCoverage);
        QCOMPARE(
            RenderEngine::render(*loaded, 0), RenderEngine::render(source, 0));
    }

    void roundTripsSchemaElevenRasterAssetsAndRejectsCorruption()
    {
        Document source = Document::createDefault(QSize(40, 30));
        source.background = Qt::transparent;
        QImage image(QSize(6, 4), QImage::Format_RGBA8888);
        image.fill(QColor(15, 80, 170, 210));
        image.setPixelColor(2, 1, QColor(240, 120, 20, 255));
        const std::optional<RasterAsset> asset =
            serializer_detail::rasterAssetFromImage(image);
        QVERIFY(asset.has_value());
        source.rasterAssets.insert(asset->id, *asset);
        Stroke operation;
        operation.mode = StrokeMode::Image;
        operation.points.clear();
        operation.imageOp = ImageOp{asset->id,
            QTransform(1.5, 0.2, -0.1, 1.25, 8.0, 6.0),
            SamplingMode::Smooth};
        source.layers.first().strokes = {operation};

        const QByteArray json = DocumentSerializer::toJson(source);
        QVERIFY(!json.isEmpty());
        const QJsonObject root = QJsonDocument::fromJson(json).object();
        QCOMPARE(root.value(QStringLiteral("schemaVersion")).toInt(), 13);
        QCOMPARE(
            root.value(QStringLiteral("rasterAssets")).toArray().size(), 1);

        QString error;
        const std::optional<Document> loaded =
            DocumentSerializer::fromJson(json, &error);
        QVERIFY2(loaded.has_value(), qPrintable(error));
        QCOMPARE(loaded->rasterAssets, source.rasterAssets);
        QCOMPARE(
            loaded->layers.first().strokes.first().imageOp, operation.imageOp);
        QCOMPARE(
            RenderEngine::render(*loaded, 0), RenderEngine::render(source, 0));

        QJsonObject corruptedRoot = root;
        QJsonArray assets =
            corruptedRoot.value(QStringLiteral("rasterAssets")).toArray();
        QJsonObject corruptedAsset = assets.first().toObject();
        corruptedAsset.insert(
            QStringLiteral("id"), QString(64, QLatin1Char('0')));
        assets[0] = corruptedAsset;
        corruptedRoot.insert(QStringLiteral("rasterAssets"), assets);
        QVERIFY(!DocumentSerializer::fromJson(
            QJsonDocument(corruptedRoot).toJson(QJsonDocument::Compact),
            &error));

        QJsonObject schemaTenRoot = root;
        schemaTenRoot.insert(QStringLiteral("schemaVersion"), 10);
        QVERIFY(!DocumentSerializer::fromJson(
            QJsonDocument(schemaTenRoot).toJson(QJsonDocument::Compact),
            &error));
    }

    void loadsLegacyBrushAsInk()
    {
        const QByteArray json = QByteArrayLiteral(
            R"({"schemaVersion":1,"algorithmVersion":1,"canvas":{"width":20,"height":20,"background":"#ffffffff"},"animation":{"frames":2,"fps":25,"wobble":1},"activeLayerId":"11111111-1111-1111-1111-111111111111","layers":[{"id":"11111111-1111-1111-1111-111111111111","name":"Layer","visible":true,"opacity":1,"strokes":[{"id":"22222222-2222-2222-2222-222222222222","seed":"1","mode":"paint","color":"#ff000000","width":6,"points":[[5,5,0.5],[15,15,1]]}]}]})");

        QString error;
        const std::optional<Document> document =
            DocumentSerializer::fromJson(json, &error);
        QVERIFY2(document.has_value(), qPrintable(error));
        const Stroke &stroke = document->layers.first().strokes.first();
        QCOMPARE(stroke.brush.engine, BrushEngine::Line);
        QCOMPARE(stroke.brush.tipShape, BrushTipShape::Round);
        QCOMPARE(stroke.brush.sizeDynamics, 0.8);
        QCOMPARE(stroke.brush.wobbleScale, 1.0);
        QVERIFY(isValidBrushSettings(stroke.brush));
    }

    void preservesProceduralFillAcrossSchemaFourMigration()
    {
        Document source = Document::createDefault(QSize(64, 64));
        source.background = Qt::transparent;
        source.animationFrames = 8;
        source.wobbleAmount = 2.4;

        Stroke boundary;
        boundary.seed = 91234;
        boundary.width = 3.0;
        boundary.brush.antialiasing = false;
        boundary.points = {{QPointF(15.0, 15.0), 1.0},
            {QPointF(49.0, 15.0), 1.0},
            {QPointF(49.0, 49.0), 1.0},
            {QPointF(15.0, 49.0), 1.0},
            {QPointF(15.0, 20.0), 1.0}};
        Stroke fill;
        fill.seed = 5678;
        fill.mode = StrokeMode::Fill;
        fill.color = QColor(220, 40, 70);
        fill.points = {{QPointF(32.0, 32.0), 1.0}};
        source.layers.first().strokes = {boundary, fill};

        QJsonObject schemaFour =
            QJsonDocument::fromJson(DocumentSerializer::toJson(source))
                .object();
        schemaFour.insert(QStringLiteral("schemaVersion"), 4);
        schemaFour.remove(QStringLiteral("binaryMasks"));
        QJsonArray layers =
            schemaFour.value(QStringLiteral("layers")).toArray();
        QJsonObject layer = layers.first().toObject();
        layer.remove(QStringLiteral("initialCanvasSize"));
        layers[0] = layer;
        schemaFour.insert(QStringLiteral("layers"), layers);

        QString error;
        const std::optional<Document> migrated = DocumentSerializer::fromJson(
            QJsonDocument(schemaFour).toJson(QJsonDocument::Compact), &error);
        QVERIFY2(migrated.has_value(), qPrintable(error));
        QVERIFY(migrated->layers.first().strokes.last().fillMask.isNull());

        const QByteArray currentJson = DocumentSerializer::toJson(*migrated);
        QVERIFY(!currentJson.isEmpty());
        const QJsonObject currentRoot =
            QJsonDocument::fromJson(currentJson).object();
        QCOMPARE(
            currentRoot.value(QStringLiteral("schemaVersion")).toInt(), 13);
        QCOMPARE(
            currentRoot.value(QStringLiteral("algorithmVersion")).toInt(), 3);
        const std::optional<Document> reloaded =
            DocumentSerializer::fromJson(currentJson, &error);
        QVERIFY2(reloaded.has_value(), qPrintable(error));
        QVERIFY(reloaded->layers.first().strokes.last().fillMask.isNull());
        for (int frame = 0; frame < source.animationFrames; ++frame)
        {
            QCOMPARE(RenderEngine::render(*migrated, frame),
                RenderEngine::render(*reloaded, frame));
            QCOMPARE(RenderEngine::render(source, frame),
                RenderEngine::render(*reloaded, frame));
        }

        Document changed = *reloaded;
        changed.animationFrames = 12;
        changed.wobbleAmount = 5.5;
        const QByteArray changedJson = DocumentSerializer::toJson(changed);
        const std::optional<Document> changedReloaded =
            DocumentSerializer::fromJson(changedJson, &error);
        QVERIFY2(changedReloaded.has_value(), qPrintable(error));
        bool changedAtLeastOneFrame = false;
        for (int frame = 0; frame < changed.animationFrames; ++frame)
        {
            const QImage changedFrame = RenderEngine::render(changed, frame);
            QCOMPARE(
                changedFrame, RenderEngine::render(*changedReloaded, frame));
            if (changedFrame
                != RenderEngine::render(
                    *reloaded, frame % reloaded->animationFrames))
            {
                changedAtLeastOneFrame = true;
            }
        }
        QVERIFY(changedAtLeastOneFrame);
    }

    void preservesFrozenSchemaFiveFillAcrossSchemaSixRoundTrip()
    {
        Document source = Document::createDefault(QSize(32, 24));
        source.background = Qt::transparent;
        source.animationFrames = 6;
        source.wobbleAmount = 3.0;
        Stroke fill;
        fill.mode = StrokeMode::Fill;
        fill.color = QColor(20, 120, 230, 190);
        fill.points = {{QPointF(8.0, 8.0), 1.0}};
        fill.fillMask = QImage(source.size, QImage::Format_Grayscale8);
        fill.fillMask.fill(0);
        for (int y = 4; y < 18; ++y)
        {
            std::fill(fill.fillMask.scanLine(y) + 5,
                fill.fillMask.scanLine(y) + 21,
                255);
        }
        source.layers.first().strokes = {fill};

        QJsonObject schemaFive =
            QJsonDocument::fromJson(DocumentSerializer::toJson(source))
                .object();
        schemaFive.insert(QStringLiteral("schemaVersion"), 5);
        schemaFive.remove(QStringLiteral("binaryMasks"));
        QJsonArray layers =
            schemaFive.value(QStringLiteral("layers")).toArray();
        QJsonObject layer = layers.first().toObject();
        layer.remove(QStringLiteral("initialCanvasSize"));
        layers[0] = layer;
        schemaFive.insert(QStringLiteral("layers"), layers);

        QString error;
        const std::optional<Document> migrated = DocumentSerializer::fromJson(
            QJsonDocument(schemaFive).toJson(QJsonDocument::Compact), &error);
        QVERIFY2(migrated.has_value(), qPrintable(error));
        QCOMPARE(
            migrated->layers.first().strokes.first().fillMask, fill.fillMask);

        const QByteArray schemaSixJson = DocumentSerializer::toJson(*migrated);
        QVERIFY(!schemaSixJson.isEmpty());
        const QJsonObject schemaSixRoot =
            QJsonDocument::fromJson(schemaSixJson).object();
        QCOMPARE(
            schemaSixRoot.value(QStringLiteral("algorithmVersion")).toInt(), 3);
        const QJsonObject schemaSixStroke =
            schemaSixRoot.value(QStringLiteral("layers"))
                .toArray()
                .first()
                .toObject()
                .value(QStringLiteral("strokes"))
                .toArray()
                .first()
                .toObject();
        QVERIFY(schemaSixStroke.contains(QStringLiteral("fillMaskId")));
        const std::optional<Document> reloaded =
            DocumentSerializer::fromJson(schemaSixJson, &error);
        QVERIFY2(reloaded.has_value(), qPrintable(error));
        QCOMPARE(
            reloaded->layers.first().strokes.first().fillMask, fill.fillMask);
        for (int frame = 0; frame < source.animationFrames; ++frame)
        {
            QCOMPARE(RenderEngine::render(*migrated, frame),
                RenderEngine::render(*reloaded, frame));
        }
    }

    void validatesLayerBlendModeSchema()
    {
        const Document source = Document::createDefault(QSize(32, 24));
        QJsonObject root =
            QJsonDocument::fromJson(DocumentSerializer::toJson(source))
                .object();
        QJsonArray layers = root.value(QStringLiteral("layers")).toArray();
        QJsonObject layer = layers.first().toObject();
        layer.insert(QStringLiteral("blendMode"), QStringLiteral("invalid"));
        layers[0] = layer;
        root.insert(QStringLiteral("layers"), layers);

        QString error;
        QVERIFY(!DocumentSerializer::fromJson(
            QJsonDocument(root).toJson(QJsonDocument::Compact), &error)
                .has_value());
        QVERIFY(!error.isEmpty());

        root.insert(QStringLiteral("schemaVersion"), 6);
        layer.remove(QStringLiteral("blendMode"));
        layers[0] = layer;
        root.insert(QStringLiteral("layers"), layers);
        const std::optional<Document> legacy = DocumentSerializer::fromJson(
            QJsonDocument(root).toJson(QJsonDocument::Compact), &error);
        QVERIFY2(legacy.has_value(), qPrintable(error));
        QCOMPARE(legacy->layers.first().blendMode, LayerBlendMode::Normal);
    }

    void roundTripsOrderedFramebufferOperations()
    {
        Document document = Document::createDefault(QSize(48, 40));
        document.background = Qt::transparent;
        document.wobbleAmount = 0.0;
        Stroke stroke;
        stroke.width = 5.0;
        stroke.points = {{QPointF(8.0, 18.0), 1.0}, {QPointF(28.0, 18.0), 1.0}};
        const QUuid strokeId = stroke.id;
        const QUuid layerId = document.activeLayerId;
        document.layers.first().strokes = {stroke};

        DocumentController controller;
        controller.loadDocument(document);
        QVERIFY(controller.resizeCanvas(QSize(56, 44), QPoint(4, 2)));
        QImage selection(controller.document().size, QImage::Format_Grayscale8);
        selection.fill(0);
        for (int y = 14; y < 27; ++y)
        {
            std::fill(
                selection.scanLine(y) + 10, selection.scanLine(y) + 37, 255);
        }
        QVERIFY(controller.moveStrokes(
            layerId, {strokeId}, QPointF(6.0, 5.0), selection));

        const QVector<Stroke> &operations =
            controller.document().layers.first().strokes;
        QCOMPARE(operations.size(), 3);
        QCOMPARE(operations[1].mode, StrokeMode::Reframe);
        QCOMPARE(operations[2].mode, StrokeMode::PixelSelection);
        const QByteArray json =
            DocumentSerializer::toJson(controller.document());
        QVERIFY(!json.isEmpty());
        QString error;
        const std::optional<Document> loaded =
            DocumentSerializer::fromJson(json, &error);
        QVERIFY2(loaded.has_value(), qPrintable(error));
        const QVector<Stroke> &loadedOperations =
            loaded->layers.first().strokes;
        QCOMPARE(loadedOperations.size(), operations.size());
        QVERIFY(loadedOperations[1].reframeOp == operations[1].reframeOp);
        QVERIFY(loadedOperations[2].pixelSelectionOp
                == operations[2].pixelSelectionOp);
        for (int frame = 0; frame < controller.document().animationFrames;
            ++frame)
        {
            QCOMPARE(RenderEngine::render(controller.document(), frame),
                RenderEngine::render(*loaded, frame));
        }
    }

    void roundTripsSchemaThirteenCompositeBoundaries()
    {
        Document source = Document::createDefault(QSize(64, 48));
        source.background = Qt::transparent;
        Stroke paint;
        paint.color = QColor(220, 40, 30);
        paint.width = 12.0;
        paint.points = {{QPointF(8.0, 24.0), 1.0}, {QPointF(56.0, 24.0), 1.0}};
        Stroke boundary;
        boundary.mode = StrokeMode::CompositeBoundary;
        boundary.points.clear();
        Stroke erase;
        erase.mode = StrokeMode::Erase;
        erase.width = 12.0;
        erase.points = {{QPointF(24.0, 24.0), 1.0}, {QPointF(40.0, 24.0), 1.0}};
        source.layers.first().strokes = {paint, boundary, erase};

        const QByteArray json = DocumentSerializer::toJson(source);
        QVERIFY(!json.isEmpty());
        const QJsonObject root = QJsonDocument::fromJson(json).object();
        QCOMPARE(root.value(QStringLiteral("schemaVersion")).toInt(), 13);
        const QJsonObject serializedBoundary =
            root.value(QStringLiteral("layers"))
                .toArray()
                .first()
                .toObject()
                .value(QStringLiteral("strokes"))
                .toArray()[1]
                .toObject();
        QCOMPARE(serializedBoundary.value(QStringLiteral("mode")).toString(),
            QStringLiteral("compositeBoundary"));
        QVERIFY(serializedBoundary.value(QStringLiteral("points"))
                .toArray()
                .isEmpty());
        QVERIFY(!serializedBoundary.contains(QStringLiteral("pixelSelection")));
        QVERIFY(!serializedBoundary.contains(QStringLiteral("reframe")));
        QVERIFY(!serializedBoundary.contains(QStringLiteral("image")));

        QString error;
        const std::optional<Document> loaded =
            DocumentSerializer::fromJson(json, &error);
        QVERIFY2(loaded.has_value(), qPrintable(error));
        const QVector<Stroke> &loadedStrokes = loaded->layers.first().strokes;
        QCOMPARE(loadedStrokes.size(), 3);
        QCOMPARE(loadedStrokes[1].mode, StrokeMode::CompositeBoundary);
        QCOMPARE(loadedStrokes[1].id, boundary.id);
        QVERIFY(loadedStrokes[1].points.isEmpty());
        QVERIFY(!loadedStrokes[1].pixelSelectionOp);
        QVERIFY(!loadedStrokes[1].reframeOp);
        QVERIFY(!loadedStrokes[1].imageOp);
        QVERIFY(!loadedStrokes[1].visibilityClip);
        QVERIFY(loadedStrokes[1].clipMask.isNull());
        for (int frame = 0; frame < source.animationFrames; ++frame)
        {
            QCOMPARE(RenderEngine::render(*loaded, frame),
                RenderEngine::render(source, frame));
        }
    }

    void rejectsCompositeBoundariesThatCarryContent()
    {
        Document source = Document::createDefault(QSize(32, 32));
        Stroke paint;
        paint.points = {{QPointF(4.0, 16.0), 1.0}, {QPointF(28.0, 16.0), 1.0}};
        source.layers.first().strokes = {paint};
        const auto documentWithBoundary = [&source](const Stroke &boundary)
        {
            Document candidate = source;
            candidate.layers.first().strokes.append(boundary);
            return candidate;
        };

        Stroke boundary;
        boundary.mode = StrokeMode::CompositeBoundary;
        boundary.points.clear();
        const QByteArray validJson =
            DocumentSerializer::toJson(documentWithBoundary(boundary));
        QVERIFY(!validJson.isEmpty());

        Stroke withPoints = boundary;
        withPoints.points = {{QPointF(2.0, 2.0), 1.0}};
        QVERIFY(DocumentSerializer::toJson(documentWithBoundary(withPoints))
                .isEmpty());

        Stroke withPixelSelection = boundary;
        withPixelSelection.pixelSelectionOp = PixelSelectionOp();
        QVERIFY(
            DocumentSerializer::toJson(documentWithBoundary(withPixelSelection))
                .isEmpty());

        Stroke withReframe = boundary;
        withReframe.reframeOp = ReframeOp();
        QVERIFY(DocumentSerializer::toJson(documentWithBoundary(withReframe))
                .isEmpty());

        Stroke withImage = boundary;
        withImage.imageOp = ImageOp();
        QVERIFY(DocumentSerializer::toJson(documentWithBoundary(withImage))
                .isEmpty());

        Stroke withVisibilityClip = boundary;
        withVisibilityClip.visibilityClip = QRect(2, 2, 8, 8);
        QVERIFY(
            DocumentSerializer::toJson(documentWithBoundary(withVisibilityClip))
                .isEmpty());

        Stroke withClipMask = boundary;
        withClipMask.clipMask = QImage(source.size, QImage::Format_Grayscale8);
        withClipMask.clipMask.fill(255);
        QVERIFY(DocumentSerializer::toJson(documentWithBoundary(withClipMask))
                .isEmpty());

        Stroke withFillCoverage = boundary;
        withFillCoverage.fillCoverage = PackedMaskRegion();
        QVERIFY(
            DocumentSerializer::toJson(documentWithBoundary(withFillCoverage))
                .isEmpty());

        const QJsonObject validRoot =
            QJsonDocument::fromJson(validJson).object();
        const auto boundaryWithField =
            [validRoot](const QString &key, const QJsonValue &value)
        {
            QJsonObject root = validRoot;
            QJsonArray layers = root.value(QStringLiteral("layers")).toArray();
            QJsonObject layer = layers.first().toObject();
            QJsonArray strokes =
                layer.value(QStringLiteral("strokes")).toArray();
            QJsonObject serializedBoundary = strokes.last().toObject();
            serializedBoundary.insert(key, value);
            strokes[strokes.size() - 1] = serializedBoundary;
            layer.insert(QStringLiteral("strokes"), strokes);
            layers[0] = layer;
            root.insert(QStringLiteral("layers"), layers);
            return QJsonDocument(root).toJson(QJsonDocument::Compact);
        };

        QString error;
        // Built with append: QJsonArray{QJsonArray{...}} picks the copy
        // constructor on compilers without CWG2137 and would flatten the
        // payload into plain numbers.
        QJsonArray boundaryPoints;
        boundaryPoints.append(QJsonArray{2.0, 2.0, 1.0});
        QVERIFY(!DocumentSerializer::fromJson(
            boundaryWithField(QStringLiteral("points"), boundaryPoints),
            &error));
        QVERIFY(error.contains(QStringLiteral("point count")));
        QVERIFY(!DocumentSerializer::fromJson(
            boundaryWithField(QStringLiteral("reframe"), QJsonObject()),
            &error));
        QVERIFY(error.contains(QStringLiteral("wrong mode")));
        QVERIFY(!DocumentSerializer::fromJson(
            boundaryWithField(QStringLiteral("image"), QJsonObject()), &error));
        QVERIFY(error.contains(QStringLiteral("wrong mode")));
        // A visibility clip decodes for any mode, so only the marker invariant
        // can reject it. Asserting the message keeps this case from passing on
        // some unrelated rejection.
        QVERIFY(!DocumentSerializer::fromJson(
            boundaryWithField(
                QStringLiteral("visibilityClip"), QJsonArray{2, 2, 8, 8}),
            &error));
        QVERIFY(error.contains(QStringLiteral("composite boundary")));

        QJsonObject downgraded = validRoot;
        downgraded.insert(QStringLiteral("schemaVersion"), 12);
        QVERIFY(!DocumentSerializer::fromJson(
            QJsonDocument(downgraded).toJson(QJsonDocument::Compact), &error));
        QVERIFY(error.contains(QStringLiteral("invalid mode")));
    }

    void opensProjectsWrittenBeforeCompositeBoundaries()
    {
        Document source = Document::createDefault(QSize(64, 48));
        source.background = Qt::transparent;
        Stroke paint;
        paint.color = QColor(30, 60, 200);
        paint.width = 12.0;
        paint.points = {{QPointF(8.0, 24.0), 1.0}, {QPointF(56.0, 24.0), 1.0}};
        Stroke erase;
        erase.mode = StrokeMode::Erase;
        erase.width = 12.0;
        erase.points = {{QPointF(24.0, 24.0), 1.0}, {QPointF(40.0, 24.0), 1.0}};
        source.layers.first().strokes = {paint, erase};

        const QByteArray json = DocumentSerializer::toJson(source);
        QVERIFY(!json.isEmpty());
        QJsonObject legacyRoot = QJsonDocument::fromJson(json).object();
        legacyRoot.insert(QStringLiteral("schemaVersion"), 12);
        QString error;
        const std::optional<Document> legacy = DocumentSerializer::fromJson(
            QJsonDocument(legacyRoot).toJson(QJsonDocument::Compact), &error);
        QVERIFY2(legacy.has_value(), qPrintable(error));
        const QVector<Stroke> &legacyStrokes = legacy->layers.first().strokes;
        QCOMPARE(legacyStrokes.size(), 2);
        QVERIFY(std::none_of(legacyStrokes.cbegin(),
            legacyStrokes.cend(),
            [](const Stroke &stroke)
            {
                return stroke.mode == StrokeMode::CompositeBoundary;
            }));
        for (int frame = 0; frame < source.animationFrames; ++frame)
        {
            QCOMPARE(RenderEngine::render(*legacy, frame),
                RenderEngine::render(source, frame));
        }
    }

    void rejectsUnsafeSelectionTransformsAtomically()
    {
        QImage selection(QSize(32, 32), QImage::Format_Grayscale8);
        selection.fill(0);
        for (int y = 8; y < 24; ++y)
        {
            std::fill(
                selection.scanLine(y) + 8, selection.scanLine(y) + 24, 255);
        }

        QTransform huge;
        huge.scale(1.0e300, 1.0e300);
        QVERIFY(!makePixelSelectionOp(selection, huge, true, true));
        const QTransform projective(
            1.0, 0.0, 0.01, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0);
        QVERIFY(!makePixelSelectionOp(selection, projective, true, true));
        QTransform singular;
        singular.scale(0.0, 1.0);
        QVERIFY(!makePixelSelectionOp(selection, singular, true, true));

        Document document = Document::createDefault(selection.size());
        Stroke stroke;
        stroke.width = 4.0;
        stroke.points = {{QPointF(6.0, 16.0), 1.0}, {QPointF(26.0, 16.0), 1.0}};
        const QUuid strokeId = stroke.id;
        const QUuid layerId = document.activeLayerId;
        document.layers.first().strokes = {stroke};
        DocumentController controller;
        controller.loadDocument(document);
        const QByteArray before =
            DocumentSerializer::toJson(controller.document());
        const int undoCount = controller.undoStack()->count();
        QVERIFY(!controller.scaleStrokes(
            layerId, {strokeId}, QPointF(16.0, 16.0), 1.0e300, selection));
        QCOMPARE(controller.undoStack()->count(), undoCount);
        QCOMPARE(DocumentSerializer::toJson(controller.document()), before);

        QTransform safeTransform;
        safeTransform.translate(2.0, 1.0);
        const std::optional<PixelSelectionOp> safeOperation =
            makePixelSelectionOp(selection, safeTransform, true, true);
        QVERIFY(safeOperation.has_value());
        Stroke operationStroke;
        operationStroke.mode = StrokeMode::PixelSelection;
        operationStroke.points.clear();
        operationStroke.pixelSelectionOp = *safeOperation;
        Document operationDocument = document;
        operationDocument.layers.first().strokes.append(operationStroke);
        QJsonObject unsafeRoot = QJsonDocument::fromJson(
            DocumentSerializer::toJson(operationDocument))
                                     .object();
        QJsonArray layers =
            unsafeRoot.value(QStringLiteral("layers")).toArray();
        QJsonObject layer = layers.first().toObject();
        QJsonArray strokes = layer.value(QStringLiteral("strokes")).toArray();
        QJsonObject serializedOperation = strokes.last().toObject();
        QJsonObject payload =
            serializedOperation.value(QStringLiteral("pixelSelection"))
                .toObject();
        payload.insert(QStringLiteral("transform"),
            QJsonArray{1.0e300, 0.0, 0.0, 0.0, 1.0e300, 0.0, 0.0, 0.0, 1.0});
        serializedOperation.insert(QStringLiteral("pixelSelection"), payload);
        strokes[strokes.size() - 1] = serializedOperation;
        layer.insert(QStringLiteral("strokes"), strokes);
        layers[0] = layer;
        unsafeRoot.insert(QStringLiteral("layers"), layers);
        QString error;
        QVERIFY(!DocumentSerializer::fromJson(
            QJsonDocument(unsafeRoot).toJson(QJsonDocument::Compact), &error));
    }

    void clearsResizedLayerWithCurrentFramebufferEpoch()
    {
        Document document = Document::createDefault(QSize(30, 24));
        document.background = Qt::transparent;
        document.wobbleAmount = 0.0;
        Stroke stroke;
        stroke.width = 4.0;
        stroke.points = {{QPointF(4.0, 12.0), 1.0}, {QPointF(24.0, 12.0), 1.0}};
        const QUuid layerId = document.activeLayerId;
        document.layers.first().strokes = {stroke};
        DocumentController controller;
        controller.loadDocument(document);
        QVERIFY(controller.resizeCanvas(QSize(42, 32), QPoint(6, 4)));
        const QImage beforeClear =
            RenderEngine::render(controller.document(), 0);

        controller.clearLayer(layerId);
        const Layer &cleared = controller.document().layers.first();
        QVERIFY(cleared.strokes.isEmpty());
        QCOMPARE(cleared.initialCanvasSize, controller.document().size);
        const QByteArray clearedJson =
            DocumentSerializer::toJson(controller.document());
        QVERIFY(!clearedJson.isEmpty());
        QString error;
        const std::optional<Document> loaded =
            DocumentSerializer::fromJson(clearedJson, &error);
        QVERIFY2(loaded.has_value(), qPrintable(error));

        DocumentController loadedController;
        loadedController.loadDocument(*loaded);
        Stroke newStroke;
        newStroke.color = Qt::red;
        newStroke.points = {
            {QPointF(8.0, 8.0), 1.0}, {QPointF(30.0, 20.0), 1.0}};
        loadedController.addStroke(layerId, newStroke);
        QVERIFY(!RenderEngine::render(loadedController.document(), 0).isNull());
        QVERIFY(
            !DocumentSerializer::toJson(loadedController.document()).isEmpty());

        controller.undoStack()->undo();
        QCOMPARE(RenderEngine::render(controller.document(), 0), beforeClear);
        controller.undoStack()->redo();
        QVERIFY(controller.document().layers.first().strokes.isEmpty());
        QCOMPARE(controller.document().layers.first().initialCanvasSize,
            QSize(42, 32));
    }

    void resizesTransparentLayerWithoutFramebufferHistory()
    {
        Document document = Document::createDefault(QSize(64, 48));
        Stroke erase;
        erase.mode = StrokeMode::Erase;
        erase.points = {{QPointF(8.0, 20.0), 1.0}, {QPointF(50.0, 20.0), 1.0}};
        document.layers.first().strokes = {erase};

        DocumentController controller;
        controller.loadDocument(document);
        QVERIFY(controller.resizeCanvas(QSize(80, 60), QPoint(5, 4)));
        const Layer &resized = controller.document().layers.first();
        QVERIFY(resized.strokes.isEmpty());
        QCOMPARE(resized.initialCanvasSize, QSize(80, 60));
        QVERIFY(!DocumentSerializer::toJson(controller.document()).isEmpty());

        controller.undoStack()->undo();
        const Layer &restored = controller.document().layers.first();
        QCOMPARE(restored.strokes.size(), 1);
        QCOMPARE(restored.strokes.first().mode, StrokeMode::Erase);
        QCOMPARE(restored.initialCanvasSize, QSize(64, 48));
        controller.undoStack()->redo();
        QVERIFY(controller.document().layers.first().strokes.isEmpty());
    }

    void rejectsLegacyVectorEditsAfterFramebufferOperations()
    {
        Document document = Document::createDefault(QSize(30, 24));
        Stroke stroke;
        stroke.points = {{QPointF(4.0, 12.0), 1.0}, {QPointF(24.0, 12.0), 1.0}};
        const QUuid strokeId = stroke.id;
        const QUuid layerId = document.activeLayerId;
        document.layers.first().strokes = {stroke};
        DocumentController controller;
        controller.loadDocument(document);
        QVERIFY(controller.resizeImage(QSize(45, 36)));
        const QByteArray before =
            DocumentSerializer::toJson(controller.document());
        const int undoCount = controller.undoStack()->count();

        QVERIFY(!controller.scaleStrokes(
            layerId, {strokeId}, QPointF(15.0, 12.0), 1.5));
        QVERIFY(!controller.duplicateStrokes(
            layerId, {strokeId}, QPointF(4.0, 3.0)));
        QCOMPARE(controller.undoStack()->count(), undoCount);
        QCOMPARE(DocumentSerializer::toJson(controller.document()), before);
    }

    void rejectsOverflowingSchemaSixBounds()
    {
        Document document = Document::createDefault(QSize(32, 32));
        Stroke stroke;
        stroke.points = {{QPointF(4.0, 16.0), 1.0}, {QPointF(28.0, 16.0), 1.0}};
        stroke.visibilityClip = QRect(2, 2, 28, 28);
        const QUuid strokeId = stroke.id;
        document.layers.first().strokes = {stroke};

        DocumentController controller;
        controller.loadDocument(document);
        QImage selection(document.size, QImage::Format_Grayscale8);
        selection.fill(0);
        for (int y = 8; y < 24; ++y)
        {
            std::fill(
                selection.scanLine(y) + 8, selection.scanLine(y) + 24, 255);
        }
        QVERIFY(controller.moveStrokes(
            document.activeLayerId, {strokeId}, QPointF(2.0, 0.0), selection));
        const QJsonObject validRoot = QJsonDocument::fromJson(
            DocumentSerializer::toJson(controller.document()))
                                          .object();

        QJsonObject badBinaryRoot = validRoot;
        QJsonArray binaryMasks =
            badBinaryRoot.value(QStringLiteral("binaryMasks")).toArray();
        QJsonObject binaryMask = binaryMasks.first().toObject();
        binaryMask.insert(QStringLiteral("bounds"),
            QJsonArray{std::numeric_limits<int>::max(), 0, 16, 16});
        binaryMasks[0] = binaryMask;
        badBinaryRoot.insert(QStringLiteral("binaryMasks"), binaryMasks);
        QString error;
        QVERIFY(!DocumentSerializer::fromJson(
            QJsonDocument(badBinaryRoot).toJson(QJsonDocument::Compact),
            &error));

        QJsonObject badClipRoot = validRoot;
        QJsonArray layers =
            badClipRoot.value(QStringLiteral("layers")).toArray();
        QJsonObject layer = layers.first().toObject();
        QJsonArray strokes = layer.value(QStringLiteral("strokes")).toArray();
        QJsonObject firstStroke = strokes.first().toObject();
        firstStroke.insert(QStringLiteral("visibilityClip"),
            QJsonArray{std::numeric_limits<int>::max(), 0, 16, 16});
        strokes[0] = firstStroke;
        layer.insert(QStringLiteral("strokes"), strokes);
        layers[0] = layer;
        badClipRoot.insert(QStringLiteral("layers"), layers);
        QVERIFY(!DocumentSerializer::fromJson(
            QJsonDocument(badClipRoot).toJson(QJsonDocument::Compact), &error));
    }
};

int runDocumentSchemaTests(int argc, char **argv)
{
    DocumentSchemaTests tests;
    return QTest::qExec(&tests, argc, argv);
}

}

#include "DocumentSchemaTests.moc"

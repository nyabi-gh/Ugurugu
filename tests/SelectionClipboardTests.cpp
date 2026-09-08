// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "io/SelectionClipboardCodec.hpp"
#include "io/serializer/RasterAssetTable.hpp"
#include "support/DocumentTestHelpers.hpp"
#include "support/DocumentTestSuites.hpp"

#include <QPainter>

namespace ugurugu
{

namespace
{

Document clipboardSourceDocument()
{
    Document document = Document::createDefault(QSize(96, 96));
    Layer &layer = document.layers.first();
    Stroke first;
    first.seed = 77;
    first.color = QColor(200, 40, 40);
    first.width = 8.0;
    first.points = {{QPointF(20, 30), 1.0},
        {QPointF(70, 30), 0.5},
        {QPointF(70, 60), 0.75}};
    layer.strokes.append(first);
    Stroke second;
    second.seed = 12345;
    second.color = QColor(40, 40, 220);
    second.width = 4.0;
    second.brush.wobbleScale = 1.5;
    second.points = {{QPointF(10, 80), 1.0}, {QPointF(85, 80), 1.0}};
    layer.strokes.append(second);
    return document;
}

QImage rectangularSelectionMask(const QSize &size, const QRect &rect)
{
    QImage mask(size, QImage::Format_Grayscale8);
    mask.fill(0);
    QPainter painter(&mask);
    painter.fillRect(rect, Qt::white);
    return mask;
}

}

class SelectionClipboardTests final : public QObject
{
    Q_OBJECT

private slots:
    void copyRoundTripPreservesStrokesAndSeeds()
    {
        const Document document = clipboardSourceDocument();
        const Layer &sourceLayer = document.layers.first();
        const QImage mask =
            rectangularSelectionMask(document.size, QRect(10, 20, 70, 50));
        QString error;
        const std::optional<SelectionClipboardCodec::Copy> copy =
            SelectionClipboardCodec::makeCopy(
                document, sourceLayer.id, mask, 0, &error);
        QVERIFY2(copy.has_value(), qPrintable(error));
        QVERIFY(!copy->payload.isEmpty());
        QCOMPARE(copy->raster.size(), QSize(70, 50));
        QCOMPARE(copy->raster.format(), QImage::Format_ARGB32);

        const std::optional<SelectionClipboardCodec::Pasted> pasted =
            SelectionClipboardCodec::decode(copy->payload, &error);
        QVERIFY2(pasted.has_value(), qPrintable(error));
        QCOMPARE(pasted->canvasSize, document.size);
        QCOMPARE(pasted->layer.strokes.size(), sourceLayer.strokes.size() + 1);
        for (int index = 0; index < sourceLayer.strokes.size(); ++index)
        {
            const Stroke &source = sourceLayer.strokes[index];
            const Stroke &copied = pasted->layer.strokes[index];
            QCOMPARE(copied.seed, source.seed);
            QCOMPARE(copied.points, source.points);
            QCOMPARE(copied.color, source.color);
            QCOMPARE(copied.width, source.width);
            QVERIFY(copied.brush == source.brush);
        }
        const Stroke &clip = pasted->layer.strokes.last();
        QCOMPARE(clip.mode, StrokeMode::PixelSelection);
        QVERIFY(clip.pixelSelectionOp.has_value());
        QVERIFY(clip.pixelSelectionOp->clearSource);
        QVERIFY(!clip.pixelSelectionOp->drawDestination);
    }

    void strokesFarOutsideTheSelectionAreNotPutOnTheClipboard()
    {
        Document document = Document::createDefault(QSize(96, 96));
        document.background = Qt::transparent;
        Layer &layer = document.layers.first();
        Stroke inside;
        inside.seed = 5;
        inside.width = 3.0;
        inside.points = {{QPointF(16, 16), 1.0}, {QPointF(24, 24), 1.0}};
        layer.strokes.append(inside);
        Stroke faraway;
        faraway.seed = 6;
        faraway.width = 3.0;
        faraway.color = QColor(10, 200, 60);
        faraway.points = {{QPointF(80, 80), 1.0}, {QPointF(90, 90), 1.0}};
        layer.strokes.append(faraway);

        const QImage mask =
            rectangularSelectionMask(document.size, QRect(8, 8, 24, 24));
        QString error;
        const std::optional<SelectionClipboardCodec::Copy> copy =
            SelectionClipboardCodec::makeCopy(
                document, layer.id, mask, 0, &error);
        QVERIFY2(copy.has_value(), qPrintable(error));

        const std::optional<SelectionClipboardCodec::Pasted> pasted =
            SelectionClipboardCodec::decode(copy->payload, &error);
        QVERIFY2(pasted.has_value(), qPrintable(error));
        // The stroke inside the selection plus the appended clip, and nothing
        // of the stroke the selection never touched: it used to travel along,
        // masked but fully recoverable by whoever dropped the clip.
        QCOMPARE(pasted->layer.strokes.size(), 2);
        QCOMPARE(pasted->layer.strokes.first().seed, inside.seed);
        QCOMPARE(pasted->layer.strokes.last().mode, StrokeMode::PixelSelection);

        // What the copy shows is unchanged: the dropped stroke never reached
        // the selection on any frame, so clipping the whole layer and clipping
        // the pruned one render the same.
        Document clipped = document;
        clipped.layers = {pasted->layer};
        clipped.activeLayerId = pasted->layer.id;
        Document unpruned = document;
        unpruned.layers.first().strokes.append(pasted->layer.strokes.last());
        for (const int frame : {0, 4, 17})
        {
            QCOMPARE(RenderEngine::render(clipped, frame),
                RenderEngine::render(unpruned, frame));
        }
    }

    void fullCanvasSelectionSkipsClipOperation()
    {
        const Document document = clipboardSourceDocument();
        const QImage mask = rectangularSelectionMask(
            document.size, QRect(QPoint(), document.size));
        const std::optional<SelectionClipboardCodec::Copy> copy =
            SelectionClipboardCodec::makeCopy(
                document, document.layers.first().id, mask, 0);
        QVERIFY(copy.has_value());
        QCOMPARE(copy->raster.size(), document.size);
        const std::optional<SelectionClipboardCodec::Pasted> pasted =
            SelectionClipboardCodec::decode(copy->payload);
        QVERIFY(pasted.has_value());
        QCOMPARE(pasted->layer.strokes.size(),
            document.layers.first().strokes.size());
    }

    void copiedRasterUsesTheSourceMotionSettings()
    {
        Document document = clipboardSourceDocument();
        document.background = Qt::transparent;
        document.wobbleAmount = 6.0;
        document.motion.style = MotionStyle::Smooth;
        document.motion.poseCount = 5;
        document.motion.detail = 18;
        document.motion.linked = 0.25;
        document.motion.randomness = 0.7;
        const int frame = 7;
        const QImage mask = rectangularSelectionMask(
            document.size, QRect(QPoint(), document.size));

        Document defaultMotion = document;
        defaultMotion.motion = MotionSettings{};
        const QImage sourceFrame = RenderEngine::render(document, frame);
        QVERIFY(sourceFrame != RenderEngine::render(defaultMotion, frame));
        const QImage expected =
            sourceFrame.convertToFormat(QImage::Format_ARGB32);

        QString error;
        const std::optional<SelectionClipboardCodec::Copy> copy =
            SelectionClipboardCodec::makeCopy(
                document, document.layers.first().id, mask, frame, &error);
        QVERIFY2(copy.has_value(), qPrintable(error));
        QCOMPARE(copy->raster, expected);

        const std::optional<Document> payload =
            DocumentSerializer::fromJson(copy->payload, &error);
        QVERIFY2(payload.has_value(), qPrintable(error));
        QCOMPARE(payload->motion, document.motion);
        QCOMPARE(RenderEngine::render(*payload, frame)
                     .convertToFormat(QImage::Format_ARGB32),
            expected);
    }

    void imageAssetsSurviveCrossDocumentPasteAndHistory()
    {
        Document source = Document::createDefault(QSize(64, 64));
        QImage image(QSize(18, 12), QImage::Format_RGBA8888);
        image.fill(QColor(210, 40, 80, 190));
        QPainter painter(&image);
        painter.fillRect(QRect(3, 2, 8, 7), QColor(20, 180, 90, 255));
        painter.end();
        const std::optional<RasterAsset> asset =
            serializer_detail::rasterAssetFromImage(image);
        QVERIFY(asset.has_value());
        source.rasterAssets.insert(asset->id, *asset);
        Stroke imageStroke;
        imageStroke.mode = StrokeMode::Image;
        imageStroke.points.clear();
        imageStroke.imageOp = ImageOp{asset->id,
            QTransform::fromTranslate(9.0, 11.0),
            SamplingMode::Nearest};
        source.layers.first().strokes = {imageStroke};

        QImage unusedImage(QSize(4, 4), QImage::Format_RGBA8888);
        unusedImage.fill(Qt::blue);
        const std::optional<RasterAsset> unused =
            serializer_detail::rasterAssetFromImage(unusedImage);
        QVERIFY(unused.has_value());
        source.rasterAssets.insert(unused->id, *unused);

        QString error;
        const std::optional<SelectionClipboardCodec::Copy> copy =
            SelectionClipboardCodec::makeCopy(source,
                source.layers.first().id,
                rectangularSelectionMask(
                    source.size, QRect(QPoint(), source.size)),
                0,
                &error);
        QVERIFY2(copy.has_value(), qPrintable(error));
        QCOMPARE(copy->rasterAssets.size(), 1);
        QCOMPARE(copy->rasterAssets.value(asset->id), *asset);

        const std::optional<SelectionClipboardCodec::Pasted> pasted =
            SelectionClipboardCodec::decode(copy->payload, &error);
        QVERIFY2(pasted.has_value(), qPrintable(error));
        QCOMPARE(pasted->rasterAssets, copy->rasterAssets);

        DocumentController directController;
        QVERIFY(directController.loadDocument(
            Document::createDefault(source.size)));
        QCOMPARE(directController.pasteLayer(
                     copy->layer, copy->canvasSize, {}, {}, copy->rasterAssets),
            DocumentController::PasteLayerResult::Pasted);
        QCOMPARE(
            directController.document().rasterAssets.value(asset->id), *asset);

        DocumentController controller;
        QVERIFY(controller.loadDocument(Document::createDefault(source.size)));
        const QByteArray before =
            DocumentSerializer::toJson(controller.document());
        QCOMPARE(controller.pasteLayer(pasted->layer,
                     pasted->canvasSize,
                     {},
                     {},
                     pasted->rasterAssets),
            DocumentController::PasteLayerResult::Pasted);
        QCOMPARE(controller.document().rasterAssets.size(), 1);
        QCOMPARE(controller.document().rasterAssets.value(asset->id), *asset);

        controller.undoStack()->undo();
        QCOMPARE(DocumentSerializer::toJson(controller.document()), before);
        QVERIFY(controller.document().rasterAssets.isEmpty());
        controller.undoStack()->redo();
        QCOMPARE(controller.document().rasterAssets.size(), 1);
        QCOMPARE(controller.document().rasterAssets.value(asset->id), *asset);

        QCOMPARE(controller.pasteLayer(pasted->layer,
                     pasted->canvasSize,
                     {},
                     {},
                     pasted->rasterAssets),
            DocumentController::PasteLayerResult::Pasted);
        QCOMPARE(controller.document().rasterAssets.size(), 1);

        QImage conflictingImage(QSize(18, 12), QImage::Format_RGBA8888);
        conflictingImage.fill(Qt::yellow);
        std::optional<RasterAsset> conflicting =
            serializer_detail::rasterAssetFromImage(conflictingImage);
        QVERIFY(conflicting.has_value());
        conflicting->id = asset->id;
        const QMap<QString, RasterAsset> conflictingAssets{
            {asset->id, *conflicting}};
        const QByteArray beforeConflict =
            DocumentSerializer::toJson(controller.document());
        const int historyCount = controller.undoStack()->count();
        QCOMPARE(
            controller.pasteLayer(
                pasted->layer, pasted->canvasSize, {}, {}, conflictingAssets),
            DocumentController::PasteLayerResult::RejectedInvalidLayer);
        QCOMPARE(
            DocumentSerializer::toJson(controller.document()), beforeConflict);
        QCOMPARE(controller.undoStack()->count(), historyCount);
    }

    void emptySelectionMaskIsRejected()
    {
        const Document document = clipboardSourceDocument();
        QImage mask(document.size, QImage::Format_Grayscale8);
        mask.fill(0);
        QString error;
        QVERIFY(!SelectionClipboardCodec::makeCopy(
            document, document.layers.first().id, mask, 0, &error));
        QVERIFY(!error.isEmpty());
    }

    void pastedLayerRendersIdenticallyToCopiedSelection()
    {
        const Document document = clipboardSourceDocument();
        const QImage mask =
            rectangularSelectionMask(document.size, QRect(10, 20, 70, 50));
        const std::optional<SelectionClipboardCodec::Copy> copy =
            SelectionClipboardCodec::makeCopy(
                document, document.layers.first().id, mask, 0);
        QVERIFY(copy.has_value());
        const std::optional<SelectionClipboardCodec::Pasted> pasted =
            SelectionClipboardCodec::decode(copy->payload);
        QVERIFY(pasted.has_value());

        DocumentController controller;
        QVERIFY(controller.loadDocument(document));
        const QUuid originalActive = controller.document().activeLayerId;
        const QByteArray originalLayerJson = DocumentSerializer::toJson(
            [&]
            {
                Document single = controller.document();
                single.layers = {*single.layer(originalActive)};
                return single;
            }());
        QCOMPARE(controller.pasteLayer(pasted->layer, pasted->canvasSize),
            DocumentController::PasteLayerResult::Pasted);

        const Document &after = controller.document();
        QCOMPARE(after.layers.size(), 2);
        const Layer &pastedLayer = after.layers[1];
        QCOMPARE(after.activeLayerId, pastedLayer.id);
        QVERIFY(pastedLayer.id != pasted->layer.id);
        QCOMPARE(after.layers[0].id, originalActive);
        QCOMPARE(DocumentSerializer::toJson(
                     [&]
                     {
                         Document single = after;
                         single.activeLayerId = originalActive;
                         single.layers = {after.layers[0]};
                         return single;
                     }()),
            originalLayerJson);

        const std::optional<Document> payloadDocument =
            DocumentSerializer::fromJson(copy->payload);
        QVERIFY(payloadDocument.has_value());
        Document pastedOnly = after;
        pastedOnly.background = QColor(0, 0, 0, 0);
        pastedOnly.layers = {pastedLayer};
        for (const int frame : {0, 3, 11})
        {
            QCOMPARE(RenderEngine::render(pastedOnly, frame),
                RenderEngine::render(*payloadDocument, frame));
        }
    }

    void pasteInsertsAboveActiveLayerInsideItsGroup()
    {
        Document document = Document::createDefault(QSize(96, 96));
        Layer group;
        group.kind = LayerKind::Group;
        group.name = QStringLiteral("Group");
        group.initialCanvasSize = document.size;
        Layer child;
        child.name = QStringLiteral("Child");
        child.parentGroupId = group.id;
        child.initialCanvasSize = document.size;
        const QUuid groupId = group.id;
        const QUuid childId = child.id;
        document.layers.append(std::move(group));
        document.layers.append(std::move(child));
        document.activeLayerId = childId;

        DocumentController controller;
        QVERIFY(controller.loadDocument(document));

        const Document source = clipboardSourceDocument();
        const std::optional<SelectionClipboardCodec::Copy> copy =
            SelectionClipboardCodec::makeCopy(source,
                source.layers.first().id,
                rectangularSelectionMask(source.size, QRect(10, 20, 70, 50)),
                0);
        QVERIFY(copy.has_value());
        const std::optional<SelectionClipboardCodec::Pasted> pasted =
            SelectionClipboardCodec::decode(copy->payload);
        QVERIFY(pasted.has_value());
        QCOMPARE(controller.pasteLayer(pasted->layer, pasted->canvasSize),
            DocumentController::PasteLayerResult::Pasted);

        const Document &after = controller.document();
        QCOMPARE(after.layers.size(), 4);
        QCOMPARE(after.layers[2].id, childId);
        const Layer &pastedLayer = after.layers[3];
        QCOMPARE(pastedLayer.parentGroupId, groupId);
        QCOMPARE(after.activeLayerId, pastedLayer.id);
        QVERIFY(pastedLayer.visible);
        QVERIFY(!pastedLayer.reference);
        QVERIFY(!pastedLayer.clipToLayerBelow);
    }

    void pasteAtLayerLimitRejectsWithoutChangingTheDocument()
    {
        Document document = Document::createDefault(QSize(64, 64));
        while (document.layers.size() < DocumentLimits::maximumLayers)
        {
            Layer layer;
            layer.name =
                QStringLiteral("Layer %1").arg(document.layers.size() + 1);
            layer.initialCanvasSize = document.size;
            document.layers.append(std::move(layer));
        }
        DocumentController controller;
        QVERIFY(controller.loadDocument(document));

        Layer payloadLayer;
        payloadLayer.name = QStringLiteral("Pasted");
        payloadLayer.initialCanvasSize = QSize(64, 64);
        Stroke stroke;
        stroke.points = {{QPointF(10, 10), 1.0}, {QPointF(30, 30), 1.0}};
        payloadLayer.strokes.append(std::move(stroke));

        verifyRejectedHierarchyMutation(controller,
            [&]
            {
                QCOMPARE(controller.pasteLayer(payloadLayer, QSize(64, 64)),
                    DocumentController::PasteLayerResult::RejectedLayerLimit);
            });
    }

    void pasteIntoDifferentCanvasSizeAppendsReframe()
    {
        const Document source = clipboardSourceDocument();
        const std::optional<SelectionClipboardCodec::Copy> copy =
            SelectionClipboardCodec::makeCopy(source,
                source.layers.first().id,
                rectangularSelectionMask(source.size, QRect(10, 20, 70, 50)),
                0);
        QVERIFY(copy.has_value());
        const std::optional<SelectionClipboardCodec::Pasted> pasted =
            SelectionClipboardCodec::decode(copy->payload);
        QVERIFY(pasted.has_value());

        DocumentController controller;
        QVERIFY(
            controller.loadDocument(Document::createDefault(QSize(64, 64))));
        QCOMPARE(controller.pasteLayer(pasted->layer, pasted->canvasSize),
            DocumentController::PasteLayerResult::Pasted);

        const Document &after = controller.document();
        QCOMPARE(after.layers.size(), 2);
        const Layer &pastedLayer = after.layers[1];
        QCOMPARE(pastedLayer.initialCanvasSize, QSize(96, 96));
        const Stroke &reframe = pastedLayer.strokes.last();
        QCOMPARE(reframe.mode, StrokeMode::Reframe);
        QVERIFY(reframe.reframeOp.has_value());
        QCOMPARE(reframe.reframeOp->sourceSize, QSize(96, 96));
        QCOMPARE(reframe.reframeOp->targetSize, QSize(64, 64));

        const QImage rendered = RenderEngine::render(after, 0);
        QCOMPARE(rendered.size(), QSize(64, 64));
    }

    void pasteWithDeltaShiftsCopyAndMovesSelectionToTheNewLayer()
    {
        const Document source = clipboardSourceDocument();
        const QImage mask =
            rectangularSelectionMask(source.size, QRect(10, 20, 70, 50));
        const std::optional<SelectionClipboardCodec::Copy> copy =
            SelectionClipboardCodec::makeCopy(
                source, source.layers.first().id, mask, 0);
        QVERIFY(copy.has_value());
        QCOMPARE(copy->canvasSize, source.size);
        QVERIFY(!copy->layer.strokes.isEmpty());

        DocumentController controller;
        QVERIFY(controller.loadDocument(source));
        QSignalSpy selectionSpy(
            &controller, &DocumentController::selectionHistoryStateRequested);
        const QByteArray before =
            DocumentSerializer::toJson(controller.document());
        QCOMPARE(controller.pasteLayer(
                     copy->layer, copy->canvasSize, QPointF(12, 12), mask),
            DocumentController::PasteLayerResult::Pasted);

        const Document &after = controller.document();
        QCOMPARE(after.layers.size(), 2);
        const Layer &pastedLayer = after.layers[1];
        QCOMPARE(after.activeLayerId, pastedLayer.id);
        const Stroke &move = pastedLayer.strokes.last();
        QCOMPARE(move.mode, StrokeMode::PixelSelection);
        QVERIFY(move.pixelSelectionOp.has_value());
        QVERIFY(move.pixelSelectionOp->clearSource);
        QVERIFY(move.pixelSelectionOp->drawDestination);
        QCOMPARE(move.pixelSelectionOp->transform,
            QTransform::fromTranslate(12, 12));

        QCOMPARE(selectionSpy.count(), 1);
        QCOMPARE(selectionSpy.last().at(0).value<QUuid>(), pastedLayer.id);
        const QImage movedMask = selectionSpy.last().at(1).value<QImage>();
        QVERIFY(!movedMask.isNull());
        QVERIFY(movedMask.constScanLine(25 + 12)[15 + 12] >= 128);
        QVERIFY(movedMask.constScanLine(25)[11] < 128);

        controller.undoStack()->undo();
        QCOMPARE(DocumentSerializer::toJson(controller.document()), before);
        QCOMPARE(selectionSpy.count(), 2);
        QCOMPARE(
            selectionSpy.last().at(0).value<QUuid>(), source.layers.first().id);
    }

    void undoAfterPasteRestoresTheDocument()
    {
        const Document source = clipboardSourceDocument();
        const std::optional<SelectionClipboardCodec::Copy> copy =
            SelectionClipboardCodec::makeCopy(source,
                source.layers.first().id,
                rectangularSelectionMask(source.size, QRect(10, 20, 70, 50)),
                0);
        QVERIFY(copy.has_value());
        const std::optional<SelectionClipboardCodec::Pasted> pasted =
            SelectionClipboardCodec::decode(copy->payload);
        QVERIFY(pasted.has_value());

        DocumentController controller;
        QVERIFY(controller.loadDocument(source));
        const QByteArray before =
            DocumentSerializer::toJson(controller.document());
        QCOMPARE(controller.pasteLayer(pasted->layer, pasted->canvasSize),
            DocumentController::PasteLayerResult::Pasted);
        QCOMPARE(controller.document().layers.size(), 2);
        controller.undoStack()->undo();
        QCOMPARE(DocumentSerializer::toJson(controller.document()), before);
        controller.undoStack()->redo();
        QCOMPARE(controller.document().layers.size(), 2);
    }

    void decodeRejectsUnsupportedPayloads()
    {
        QString error;
        QVERIFY(!SelectionClipboardCodec::decode(QByteArray(), &error));
        QVERIFY(!error.isEmpty());
        QVERIFY(!SelectionClipboardCodec::decode(
            QByteArrayLiteral("not json"), &error));

        Document twoLayers = clipboardSourceDocument();
        Layer extra;
        extra.name = QStringLiteral("Extra");
        extra.initialCanvasSize = twoLayers.size;
        twoLayers.layers.append(std::move(extra));
        error.clear();
        QVERIFY(!SelectionClipboardCodec::decode(
            DocumentSerializer::toJson(twoLayers), &error));
        QVERIFY(!error.isEmpty());
    }
};

int runSelectionClipboardTests(int argc, char **argv)
{
    SelectionClipboardTests tests;
    return QTest::qExec(&tests, argc, argv);
}

}

#include "SelectionClipboardTests.moc"

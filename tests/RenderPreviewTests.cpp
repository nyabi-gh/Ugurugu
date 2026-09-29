// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "render/LayerCompositionPlan.hpp"
#include "support/RenderTestHelpers.hpp"
#include "support/RenderTestSuites.hpp"

namespace ugurugu
{

class RenderPreviewTests final : public QObject
{
    Q_OBJECT

private slots:
    void rendersDeterministically()
    {
        const Document document = animatedDocument();
        const QImage first = RenderEngine::render(document, 5);
        const QImage second = RenderEngine::render(document, 5);

        QVERIFY(!first.isNull());
        QCOMPARE(first.size(), document.size);
        QVERIFY(first == second);
    }

    void rendersTheBackgroundWithoutLayers()
    {
        Document document = Document::createDefault(QSize(32, 24));
        document.background = QColor(12, 34, 56);
        document.layers.clear();
        document.activeLayerId = QUuid();

        const QImage rendered = RenderEngine::render(document, 0);
        QVERIFY(!rendered.isNull());
        QCOMPARE(rendered.size(), document.size);
        QCOMPARE(rendered.pixelColor(0, 0), document.background);
        QCOMPARE(
            rendered.pixelColor(rendered.width() - 1, rendered.height() - 1),
            document.background);
    }

    void rendersScaledPreview()
    {
        const Document document = animatedDocument();
        const QSize previewSize(48, 36);
        RenderEngine::ScaledRenderStats stats;
        const QImage preview = RenderEngine::renderScaled(document,
            5,
            previewSize,
            RenderEngine::ScaledRenderMode::DisplayPreview,
            &stats);

        QVERIFY(!preview.isNull());
        QCOMPARE(preview.size(), previewSize);
        QVERIFY(stats.usedDisplayScaleReplay);
        QVERIFY(!stats.usedNativeExactFallback);
        bool containsPaint = false;
        for (int y = 0; y < preview.height() && !containsPaint; ++y)
        {
            for (int x = 0; x < preview.width(); ++x)
            {
                if (preview.pixelColor(x, y) != document.background)
                {
                    containsPaint = true;
                    break;
                }
            }
        }
        QVERIFY(containsPaint);
        QVERIFY(RenderEngine::renderScaled(document, 0, {}).isNull());

        const QImage exact = RenderEngine::renderScaled(document,
            5,
            previewSize,
            RenderEngine::ScaledRenderMode::NativeExact,
            &stats);
        QVERIFY(!exact.isNull());
        QVERIFY(!stats.usedDisplayScaleReplay);
        QVERIFY(stats.usedNativeExactFallback);

        const QImage nativeSized = RenderEngine::renderScaled(document,
            5,
            document.size,
            RenderEngine::ScaledRenderMode::DisplayPreview,
            &stats);
        QCOMPARE(nativeSized, RenderEngine::render(document, 5));
        QVERIFY(!stats.usedDisplayScaleReplay);
        QVERIFY(stats.usedNativeExactFallback);
    }

    void replaysImageOperationsAtNativeAndDisplayScale()
    {
        DocumentController controller;
        controller.newDocument(QSize(80, 60));
        controller.setBackground(Qt::transparent);
        QImage image(QSize(24, 16), QImage::Format_RGBA8888);
        image.fill(QColor(40, 120, 220, 200));
        image.setPixelColor(4, 3, QColor(250, 40, 20, 255));
        QCOMPARE(controller.insertImage(image, QStringLiteral("photo.png")),
            DocumentController::InsertImageResult::Inserted);
        const Document &document = controller.document();

        const QImage frameZero = RenderEngine::render(document, 0);
        QVERIFY(!frameZero.isNull());
        QCOMPARE(RenderEngine::render(document, 11), frameZero);
        QVERIFY(frameZero.pixelColor(30, 25).alpha() > 0);

        RenderEngine::ScaledRenderStats stats;
        const QImage preview = RenderEngine::renderScaled(document,
            5,
            QSize(40, 30),
            RenderEngine::ScaledRenderMode::DisplayPreview,
            &stats);
        QVERIFY(!preview.isNull());
        QCOMPARE(preview.size(), QSize(40, 30));
        QVERIFY(stats.usedDisplayScaleReplay);
        QVERIFY(!stats.usedNativeExactFallback);
        QVERIFY(preview.pixelColor(15, 12).alpha() > 0);
    }

    void roundsPreviewCacheCostUpToWholeKibibytes()
    {
        QCOMPARE(PreviewRenderPolicy::cacheCostKiB(-1), 1);
        QCOMPARE(PreviewRenderPolicy::cacheCostKiB(0), 1);
        QCOMPARE(PreviewRenderPolicy::cacheCostKiB(1), 1);
        QCOMPARE(PreviewRenderPolicy::cacheCostKiB(1024), 1);
        QCOMPARE(PreviewRenderPolicy::cacheCostKiB(1025), 2);
        QCOMPARE(PreviewRenderPolicy::cacheCostKiB(2048), 2);
        QCOMPARE(PreviewRenderPolicy::cacheCostKiB(2049), 3);
        QCOMPARE(PreviewRenderPolicy::cacheCostKiB(
                     std::numeric_limits<qsizetype>::max()),
            std::numeric_limits<int>::max());
    }

    void boundsAnimatedPreviewByFrameCacheBudget()
    {
        const QSize documentSize(4096, 4096);
        const QSize staticPreview =
            PreviewRenderPolicy::renderSize(documentSize, 16.0);
        const QSize animatedPreview =
            PreviewRenderPolicy::renderSize(documentSize, 16.0, 30);

        QCOMPARE(staticPreview, documentSize);
        QVERIFY(animatedPreview.width() < documentSize.width());
        QCOMPARE(animatedPreview.width(), animatedPreview.height());
        const quint64 retainedBytes =
            static_cast<quint64>(animatedPreview.width())
            * animatedPreview.height() * sizeof(quint32) * 30;
        const quint64 cacheBytes =
            static_cast<quint64>(PreviewRenderPolicy::maximumCacheKiB()) * 1024;
        QVERIFY(retainedBytes <= cacheBytes);
        const int retainedCost =
            PreviewRenderPolicy::cacheCostKiB(
                static_cast<qsizetype>(animatedPreview.width())
                * static_cast<qsizetype>(animatedPreview.height())
                * static_cast<qsizetype>(sizeof(quint32)))
            * 30;
        QVERIFY(retainedCost <= PreviewRenderPolicy::maximumCacheKiB());
        QCOMPARE(PreviewRenderPolicy::renderSize(documentSize, 16.0, 0),
            staticPreview);
    }

    void budgetsLargerReframeEpochsAtDisplayScale()
    {
        const auto makeDocument = [](int boundaryCount)
        {
            Document document = Document::createDefault(QSize(1024, 1024));
            Layer &layer = document.layers.first();
            layer.initialCanvasSize = QSize(4096, 4096);
            layer.strokes.clear();
            for (int index = 0; index < boundaryCount; ++index)
            {
                Stroke boundary;
                boundary.mode = StrokeMode::CompositeBoundary;
                layer.strokes.append(boundary);
            }
            Stroke reframe;
            reframe.mode = StrokeMode::Reframe;
            reframe.reframeOp = ReframeOp{ReframeMode::Image,
                SamplingMode::Nearest,
                QSize(4096, 4096),
                document.size,
                {}};
            layer.strokes.append(reframe);
            return document;
        };

        const Document replayProbe = makeDocument(3);
        QVERIFY(!DocumentSerializer::toJson(replayProbe).isEmpty());
        const QSize probeSize(128, 128);
        RenderEngine::ScaledRenderStats stats;
        const QImage preview = RenderEngine::renderScaled(replayProbe,
            0,
            probeSize,
            RenderEngine::ScaledRenderMode::DisplayPreview,
            &stats);
        QVERIFY(!preview.isNull());
        QVERIFY(stats.usedDisplayScaleReplay);
        QCOMPARE(stats.largestIntermediateImageSize, QSize(512, 512));
        const LayerCompositionMemoryEstimate outputHierarchy =
            RenderEngine::estimateHierarchyMemory(replayProbe, probeSize);
        QVERIFY(outputHierarchy.valid);
        QVERIFY(
            stats.maximumEstimatedWorkingSetBytes > outputHierarchy.peakBytes);

        const Document budgetProbe = makeDocument(40);
        QVERIFY(!DocumentSerializer::toJson(budgetProbe).isEmpty());
        constexpr int retainedSurfaces = 7;
        const LayerCompositionPlan plan =
            LayerCompositionPlan::build(budgetProbe);
        QVERIFY(plan.isValid());
        const LayerCompositionMemoryEstimate nativeHierarchy =
            plan.memoryEstimate(budgetProbe.size);
        QVERIFY(nativeHierarchy.valid);
        const QSize legacy = PreviewRenderPolicy::renderSize(budgetProbe.size,
            16.0,
            retainedSurfaces,
            nativeHierarchy.peakSurfaceCount);
        const QSize aware = PreviewRenderPolicy::renderSize(
            budgetProbe, 16.0, retainedSurfaces);
        QVERIFY(aware.isValid());
        QVERIFY(aware.width() < legacy.width());

        const auto estimatedBytes = [&](const QSize &outputSize)
        {
            const LayerCompositionMemoryEstimate hierarchy =
                plan.memoryEstimate(outputSize);
            const long double outputBytes =
                static_cast<long double>(hierarchy.bytesPerSurface);
            const long double paintBytes = static_cast<long double>(
                plan.maximumPaintLayerBytesPerSurfaceAtSize(
                    budgetProbe.size, outputSize));
            return outputBytes
                       * (retainedSurfaces + hierarchy.peakSurfaceCount - 1)
                   + (paintBytes - outputBytes)
                         * plan.peakPaintLayerSurfaceCount()
                   + paintBytes
                         * LayerCompositionPlan::
                             paintOperationScratchSurfaceCount;
        };
        const long double budgetBytes =
            static_cast<long double>(PreviewRenderPolicy::maximumCacheKiB())
            * 1024.0L * 0.9L;
        QVERIFY(estimatedBytes(legacy) > budgetBytes);
        QVERIFY(estimatedBytes(aware) <= budgetBytes);

        Document impossible = Document::createDefault(QSize(16, 16));
        Layer &impossibleLayer = impossible.layers.first();
        impossibleLayer.initialCanvasSize = QSize(4096, 4096);
        impossibleLayer.strokes.clear();
        for (int index = 0; index < 9000; ++index)
        {
            Stroke boundary;
            boundary.mode = StrokeMode::CompositeBoundary;
            impossibleLayer.strokes.append(boundary);
        }
        Stroke finalReframe;
        finalReframe.mode = StrokeMode::Reframe;
        finalReframe.reframeOp = ReframeOp{ReframeMode::Image,
            SamplingMode::Nearest,
            QSize(4096, 4096),
            impossible.size,
            {}};
        impossibleLayer.strokes.append(finalReframe);
        QVERIFY(!DocumentSerializer::toJson(impossible).isEmpty());
        QVERIFY(PreviewRenderPolicy::renderSize(impossible, 16.0).isEmpty());
    }

    void patchesRegionalStrokeRefreshExactly()
    {
        Document document = Document::createDefault(QSize(128, 96));
        document.animationFrames = 6;
        document.wobbleAmount = 4.0;
        Layer &base = document.layers.first();
        base.initialCanvasSize = document.size;
        base.strokes.append(makeStroke(StrokeMode::Paint,
            QColor(200, 60, 40),
            7.0,
            11,
            {QPointF(10.0, 10.0), QPointF(30.0, 20.0), QPointF(20.0, 40.0)}));
        base.strokes.append(makeStroke(StrokeMode::Paint,
            QColor(30, 80, 200),
            9.0,
            12,
            {QPointF(100.0, 70.0), QPointF(118.0, 88.0)}));
        base.strokes.append(makeStroke(StrokeMode::Erase,
            Qt::black,
            12.0,
            13,
            {QPointF(105.0, 75.0), QPointF(115.0, 85.0)}));
        Layer top;
        top.name = QStringLiteral("Top");
        top.initialCanvasSize = document.size;
        top.opacity = 0.8;
        top.blendMode = LayerBlendMode::Multiply;
        top.strokes.append(makeStroke(StrokeMode::Paint,
            QColor(20, 160, 90),
            6.0,
            14,
            {QPointF(60.0, 10.0), QPointF(80.0, 30.0)}));
        document.layers.append(top);

        const QSize previewSize(64, 48);
        QVector<QImage> framesBefore;
        for (int frame = 0; frame < document.animationFrames; ++frame)
        {
            framesBefore.append(
                RenderEngine::renderScaled(document, frame, previewSize));
            QVERIFY(!framesBefore.last().isNull());
        }

        Stroke added = makeStroke(StrokeMode::Paint,
            QColor(240, 200, 40),
            5.0,
            15,
            {QPointF(14.0, 14.0), QPointF(26.0, 30.0)});
        const QUuid addedId = added.id;
        const QUuid layerId = document.layers.first().id;
        document.layers.first().strokes.append(added);

        const RenderEngine::RegionalStrokeRefresh refresh =
            RenderEngine::prepareRegionalStrokeRefresh(
                document, layerId, addedId, previewSize);
        QVERIFY(refresh.valid);
        QVERIFY(!refresh.outputBounds.isEmpty());
        QVERIFY(!refresh.nativeBounds.isEmpty());

        qsizetype filteredStrokes = 0;
        qsizetype fullStrokes = 0;
        for (const Layer &layer : refresh.filteredDocument.layers)
        {
            filteredStrokes += layer.strokes.size();
        }
        for (const Layer &layer : document.layers)
        {
            fullStrokes += layer.strokes.size();
        }
        QVERIFY(filteredStrokes < fullStrokes);

        for (int frame = 0; frame < document.animationFrames; ++frame)
        {
            const QImage expected =
                RenderEngine::renderScaled(document, frame, previewSize);
            const QImage regional = RenderEngine::renderScaled(
                refresh.filteredDocument, frame, previewSize);
            QVERIFY(!expected.isNull());
            QVERIFY(!regional.isNull());
            QImage patched = framesBefore[frame];
            QPainter painter(&patched);
            painter.setCompositionMode(QPainter::CompositionMode_Source);
            painter.drawImage(
                refresh.outputBounds.topLeft(), regional, refresh.outputBounds);
            painter.end();
            QCOMPARE(patched, expected);
        }
    }

    // A pixel-selection operation carries pixels away from where they were
    // painted, so the layer holding one cannot have strokes dropped from it.
    // That is a reason to keep the layer whole, not to refuse the whole
    // document: a selection in one layer used to cost every other layer its
    // regional refresh, and every cached frame with it.
    void keepsLayersThatMovePixelsWholeInARegionalStrokeRefresh()
    {
        Document document = Document::createDefault(QSize(128, 96));
        document.animationFrames = 4;
        Layer &moving = document.layers.first();
        moving.name = QStringLiteral("Moving");
        moving.initialCanvasSize = document.size;
        moving.strokes.append(makeStroke(StrokeMode::Paint,
            QColor(210, 40, 70),
            8.0,
            21,
            {QPointF(20.0, 20.0), QPointF(40.0, 32.0)}));
        const QImage selection =
            rectangularMask(document.size, QRect(8, 8, 32, 32));
        QTransform shift;
        shift.translate(64.0, 32.0);
        const std::optional<PixelSelectionOp> selectionOperation =
            makePixelSelectionOp(selection, shift, true, true);
        QVERIFY(selectionOperation.has_value());
        Stroke operation;
        operation.mode = StrokeMode::PixelSelection;
        operation.pixelSelectionOp = *selectionOperation;
        moving.strokes.append(operation);

        Layer plain;
        plain.name = QStringLiteral("Plain");
        plain.initialCanvasSize = document.size;
        plain.strokes.append(makeStroke(StrokeMode::Paint,
            QColor(30, 80, 200),
            9.0,
            22,
            {QPointF(100.0, 70.0), QPointF(118.0, 88.0)}));
        plain.strokes.append(makeStroke(StrokeMode::Paint,
            QColor(20, 160, 90),
            6.0,
            23,
            {QPointF(14.0, 14.0), QPointF(26.0, 30.0)}));
        document.layers.append(plain);

        const QSize previewSize(64, 48);
        QVector<QImage> framesBefore;
        for (int frame = 0; frame < document.animationFrames; ++frame)
        {
            framesBefore.append(
                RenderEngine::renderScaled(document, frame, previewSize));
            QVERIFY(!framesBefore.last().isNull());
        }

        Stroke added = makeStroke(StrokeMode::Paint,
            QColor(240, 200, 40),
            5.0,
            24,
            {QPointF(16.0, 16.0), QPointF(28.0, 32.0)});
        const QUuid addedId = added.id;
        const QUuid layerId = document.layers.last().id;
        document.layers.last().strokes.append(added);

        const RenderEngine::RegionalStrokeRefresh refresh =
            RenderEngine::prepareRegionalStrokeRefresh(
                document, layerId, addedId, previewSize);
        QVERIFY(refresh.valid);
        QVERIFY(!refresh.outputBounds.isEmpty());
        QCOMPARE(refresh.filteredDocument.layers.first().strokes.size(),
            document.layers.first().strokes.size());
        QVERIFY(refresh.filteredDocument.layers.last().strokes.size()
                < document.layers.last().strokes.size());

        for (int frame = 0; frame < document.animationFrames; ++frame)
        {
            const QImage expected =
                RenderEngine::renderScaled(document, frame, previewSize);
            QVERIFY(!expected.isNull());
            const QImage patch =
                RenderEngine::renderScaledRegion(refresh.filteredDocument,
                    frame,
                    previewSize,
                    refresh.outputBounds);
            QVERIFY(!patch.isNull());
            QImage patched = framesBefore[frame];
            QPainter painter(&patched);
            painter.setCompositionMode(QPainter::CompositionMode_Source);
            painter.drawImage(refresh.outputBounds.topLeft(), patch);
            painter.end();
            QCOMPARE(patched, expected);
        }
    }

    void rendersARegionExactly()
    {
        Document document = Document::createDefault(QSize(128, 96));
        document.animationFrames = 4;
        Layer &base = document.layers.first();
        base.initialCanvasSize = document.size;
        base.strokes.append(makeStroke(StrokeMode::Paint,
            QColor(210, 40, 70),
            8.0,
            41,
            {QPointF(10.0, 10.0), QPointF(60.0, 40.0), QPointF(30.0, 70.0)}));
        base.strokes.append(makeStroke(StrokeMode::Erase,
            Qt::black,
            10.0,
            42,
            {QPointF(40.0, 30.0), QPointF(52.0, 44.0)}));
        const QImage selection =
            rectangularMask(document.size, QRect(8, 8, 32, 32));
        QTransform shift;
        shift.translate(48.0, 24.0);
        const std::optional<PixelSelectionOp> selectionOperation =
            makePixelSelectionOp(selection, shift, true, true);
        QVERIFY(selectionOperation.has_value());
        Stroke operation;
        operation.mode = StrokeMode::PixelSelection;
        operation.pixelSelectionOp = *selectionOperation;
        base.strokes.append(operation);

        Layer top;
        top.name = QStringLiteral("Top");
        top.initialCanvasSize = document.size;
        top.opacity = 0.8;
        top.blendMode = LayerBlendMode::Multiply;
        top.strokes.append(makeStroke(StrokeMode::Paint,
            QColor(20, 160, 90),
            6.0,
            43,
            {QPointF(60.0, 10.0), QPointF(96.0, 60.0)}));
        document.layers.append(top);

        const QSize sizes[] = {document.size, QSize(64, 48), QSize(93, 67)};
        const QRect regions[] = {
            QRect(0, 0, 16, 12), QRect(11, 7, 29, 23), QRect(0, 0, 64, 48)};
        for (const QSize &outputSize : sizes)
        {
            for (const QRect &region : regions)
            {
                const QRect clamped =
                    region.intersected(QRect(QPoint(), outputSize));
                if (clamped.isEmpty())
                {
                    continue;
                }
                const QImage whole =
                    RenderEngine::renderScaled(document, 2, outputSize);
                QVERIFY(!whole.isNull());
                const QImage part = RenderEngine::renderScaledRegion(
                    document, 2, outputSize, clamped);
                QVERIFY(!part.isNull());
                QCOMPARE(part.size(), clamped.size());
                QCOMPARE(part, whole.copy(clamped));
            }
        }

        // Upscaled output resamples across the region edge, so it has no
        // regional form and the caller has to render the frame whole.
        QVERIFY(RenderEngine::renderScaledRegion(
            document, 2, QSize(256, 192), QRect(0, 0, 16, 12))
                .isNull());
    }

    // A reframe rewrites the framebuffer and the coordinate system the plan's
    // bounds are expressed in, which nothing downstream can follow.
    void refusesRegionalStrokeRefreshAcrossAReframe()
    {
        Document document = Document::createDefault(QSize(128, 96));
        document.animationFrames = 4;
        Layer &layer = document.layers.first();
        layer.initialCanvasSize = document.size;
        layer.strokes.append(makeStroke(StrokeMode::Paint,
            QColor(210, 40, 70),
            8.0,
            31,
            {QPointF(20.0, 20.0), QPointF(40.0, 32.0)}));

        Stroke added = makeStroke(StrokeMode::Paint,
            QColor(240, 200, 40),
            5.0,
            32,
            {QPointF(90.0, 70.0), QPointF(110.0, 82.0)});
        const QUuid addedId = added.id;
        layer.strokes.append(added);

        QVERIFY(RenderEngine::prepareRegionalStrokeRefresh(
            document, layer.id, addedId, QSize(64, 48))
                .valid);

        ReframeOp reframe;
        reframe.mode = ReframeMode::Canvas;
        reframe.sourceSize = document.size;
        reframe.targetSize = document.size;
        Stroke operation;
        operation.mode = StrokeMode::Reframe;
        operation.reframeOp = reframe;
        layer.strokes.insert(0, operation);

        QVERIFY(!RenderEngine::prepareRegionalStrokeRefresh(
            document, layer.id, addedId, QSize(64, 48))
                .valid);
    }

    void replaysIntegralNearestSelectionAtDisplayScale()
    {
        Document document = Document::createDefault(QSize(128, 96));
        document.background = Qt::transparent;
        document.wobbleAmount = 0.0;
        Layer &layer = document.layers.first();
        layer.initialCanvasSize = document.size;

        Stroke block = makeStroke(StrokeMode::Paint,
            QColor(210, 40, 70),
            32.0,
            901,
            {QPointF(24.0, 24.0)});
        block.brush.tipShape = BrushTipShape::Square;
        block.brush.antialiasing = false;
        block.brush.sizeDynamics = 0.0;
        block.brush.wobbleScale = 0.0;
        layer.strokes.append(block);

        const QImage selection =
            rectangularMask(document.size, QRect(8, 8, 32, 32));
        QTransform shift;
        shift.translate(64.0, 32.0);
        const std::optional<PixelSelectionOp> selectionOperation =
            makePixelSelectionOp(selection, shift, true, true);
        QVERIFY(selectionOperation.has_value());
        QCOMPARE(selectionOperation->sampling, SamplingMode::Nearest);
        Stroke operation;
        operation.mode = StrokeMode::PixelSelection;
        operation.pixelSelectionOp = *selectionOperation;
        layer.strokes.append(operation);

        Stroke overlay = makeStroke(StrokeMode::Paint,
            QColor(35, 180, 95),
            16.0,
            902,
            {QPointF(88.0, 56.0)});
        overlay.brush.tipShape = BrushTipShape::Square;
        overlay.brush.antialiasing = false;
        overlay.brush.sizeDynamics = 0.0;
        overlay.brush.wobbleScale = 0.0;
        layer.strokes.append(overlay);

        const QSize outputSize(32, 24);
        RenderEngine::ScaledRenderStats stats;
        const QImage preview = RenderEngine::renderScaled(document,
            0,
            outputSize,
            RenderEngine::ScaledRenderMode::DisplayPreview,
            &stats);
        const QImage exact = RenderEngine::renderScaled(document,
            0,
            outputSize,
            RenderEngine::ScaledRenderMode::NativeExact);
        QVERIFY(!preview.isNull());
        QVERIFY(!exact.isNull());
        QCOMPARE(preview, exact);
        QVERIFY(stats.usedDisplayScaleReplay);
        QVERIFY(stats.packedSelectionSamples > 0);
    }

    void conjugatesSelectionTransformForNonUniformPreviewScale()
    {
        Document document = Document::createDefault(QSize(128, 96));
        document.background = Qt::transparent;
        document.wobbleAmount = 0.0;
        Layer &layer = document.layers.first();
        layer.initialCanvasSize = document.size;

        Stroke block = makeStroke(StrokeMode::Paint,
            QColor(195, 65, 215),
            32.0,
            903,
            {QPointF(32.0, 32.0)});
        block.brush.tipShape = BrushTipShape::Square;
        block.brush.antialiasing = false;
        block.brush.sizeDynamics = 0.0;
        block.brush.wobbleScale = 0.0;
        layer.strokes.append(block);

        const QImage selection =
            rectangularMask(document.size, QRect(16, 16, 32, 32));
        // x' = 96 - y, y' = x: an integral quarter turn whose display
        // transform is anisotropic at the requested 1/4 x 1/8 scale.
        const QTransform quarterTurn(
            0.0, 1.0, 0.0, -1.0, 0.0, 0.0, 96.0, 0.0, 1.0);
        const std::optional<PixelSelectionOp> pixelOperation =
            makePixelSelectionOp(selection, quarterTurn, true, true);
        QVERIFY(pixelOperation.has_value());
        QCOMPARE(pixelOperation->sampling, SamplingMode::Nearest);
        Stroke operation;
        operation.mode = StrokeMode::PixelSelection;
        operation.pixelSelectionOp = *pixelOperation;
        layer.strokes.append(operation);

        const QSize outputSize(32, 12);
        const QImage preview =
            RenderEngine::renderScaled(document, 0, outputSize);
        const QImage exact = RenderEngine::renderScaled(document,
            0,
            outputSize,
            RenderEngine::ScaledRenderMode::NativeExact);
        QVERIFY(!preview.isNull());
        QCOMPARE(preview, exact);
    }

    void matchesNativeExactForRandomIntegralSelectionReplays()
    {
        quint32 random = 0x8f3a91d7U;
        const auto next = [&random]()
        {
            random ^= random << 13U;
            random ^= random >> 17U;
            random ^= random << 5U;
            return random;
        };

        for (int caseIndex = 0; caseIndex < 24; ++caseIndex)
        {
            Document document = Document::createDefault(QSize(128, 96));
            document.background = Qt::transparent;
            document.wobbleAmount = 0.0;
            Layer &layer = document.layers.first();
            layer.initialCanvasSize = document.size;

            const int blockEdge = 16 + static_cast<int>(next() % 3) * 8;
            const int sourceX = 8 + static_cast<int>(next() % 9) * 4;
            const int sourceY = 8 + static_cast<int>(next() % 7) * 4;
            Stroke block = makeStroke(StrokeMode::Paint,
                QColor(40 + static_cast<int>(next() % 180),
                    40 + static_cast<int>(next() % 180),
                    40 + static_cast<int>(next() % 180)),
                blockEdge,
                next(),
                {QPointF(
                    sourceX + blockEdge * 0.5, sourceY + blockEdge * 0.5)});
            block.brush.tipShape = BrushTipShape::Square;
            block.brush.antialiasing = false;
            block.brush.sizeDynamics = 0.0;
            block.brush.wobbleScale = 0.0;
            layer.strokes.append(block);

            const QImage selection = rectangularMask(
                document.size, QRect(sourceX, sourceY, blockEdge, blockEdge));
            const int deltaX = 48 + static_cast<int>(next() % 5) * 4;
            const int deltaY = (static_cast<int>(next() % 7) - 3) * 4;
            QTransform transform;
            transform.translate(deltaX, deltaY);
            const bool clearSource = caseIndex % 3 != 1;
            const bool drawDestination = caseIndex % 3 != 2;
            const std::optional<PixelSelectionOp> pixelOperation =
                makePixelSelectionOp(
                    selection, transform, clearSource, drawDestination);
            QVERIFY(pixelOperation.has_value());
            QCOMPARE(pixelOperation->sampling, SamplingMode::Nearest);
            Stroke operation;
            operation.mode = StrokeMode::PixelSelection;
            operation.pixelSelectionOp = *pixelOperation;
            layer.strokes.append(operation);

            const QSize outputSize(32, 24);
            const QImage preview =
                RenderEngine::renderScaled(document, caseIndex, outputSize);
            const QImage exact = RenderEngine::renderScaled(document,
                caseIndex,
                outputSize,
                RenderEngine::ScaledRenderMode::NativeExact);
            QVERIFY2(preview == exact,
                qPrintable(QStringLiteral("integral replay case %1 diverged")
                        .arg(caseIndex)));
        }
    }

    void replaysCanvasCropAndExpandWithoutNativeSurfaces()
    {
        Document document = Document::createDefault(QSize(128, 96));
        document.background = Qt::transparent;
        document.wobbleAmount = 0.0;
        Layer &layer = document.layers.first();
        layer.initialCanvasSize = document.size;

        Stroke first = makeStroke(StrokeMode::Paint,
            QColor(220, 65, 35),
            32.0,
            911,
            {QPointF(24.0, 28.0)});
        first.brush.tipShape = BrushTipShape::Square;
        first.brush.antialiasing = false;
        first.brush.sizeDynamics = 0.0;
        first.brush.wobbleScale = 0.0;
        layer.strokes.append(first);

        Stroke crop;
        crop.mode = StrokeMode::Reframe;
        crop.reframeOp = ReframeOp{ReframeMode::Canvas,
            SamplingMode::Nearest,
            QSize(128, 96),
            QSize(96, 72),
            QPoint(-16, -8)};
        layer.strokes.append(crop);

        Stroke second = makeStroke(StrokeMode::Paint,
            QColor(35, 165, 220),
            24.0,
            912,
            {QPointF(68.0, 48.0)});
        second.brush.tipShape = BrushTipShape::Square;
        second.brush.antialiasing = false;
        second.brush.sizeDynamics = 0.0;
        second.brush.wobbleScale = 0.0;
        layer.strokes.append(second);

        Stroke expand;
        expand.mode = StrokeMode::Reframe;
        expand.reframeOp = ReframeOp{ReframeMode::Canvas,
            SamplingMode::Nearest,
            QSize(96, 72),
            QSize(128, 96),
            QPoint(12, 8)};
        layer.strokes.append(expand);

        const QSize outputSize(32, 24);
        RenderEngine::ScaledRenderStats stats;
        const QImage preview = RenderEngine::renderScaled(document,
            0,
            outputSize,
            RenderEngine::ScaledRenderMode::DisplayPreview,
            &stats);
        const QImage exact = RenderEngine::renderScaled(document,
            0,
            outputSize,
            RenderEngine::ScaledRenderMode::NativeExact);
        QVERIFY(!preview.isNull());
        QCOMPARE(preview, exact);
        QVERIFY(stats.usedDisplayScaleReplay);
        QCOMPARE(stats.largestIntermediateImageSize, outputSize);
        QVERIFY(stats.largestIntermediateImageBytes
                <= static_cast<quint64>(outputSize.width())
                       * outputSize.height() * sizeof(QRgb));
        // The left part clipped by the crop must not reappear on expansion.
        QCOMPARE(preview.pixelColor(1, 4), QColor(Qt::transparent));
    }

    void previewsSmoothImageResizeNearNativeExactResult()
    {
        Document document = Document::createDefault(QSize(96, 72));
        document.background = Qt::transparent;
        document.wobbleAmount = 0.0;
        Layer &layer = document.layers.first();
        layer.initialCanvasSize = QSize(160, 120);

        Stroke block = makeStroke(StrokeMode::Paint,
            QColor(45, 115, 225),
            56.0,
            921,
            {QPointF(52.0, 48.0)});
        block.brush.tipShape = BrushTipShape::Square;
        block.brush.antialiasing = false;
        block.brush.sizeDynamics = 0.0;
        block.brush.wobbleScale = 0.0;
        layer.strokes.append(block);

        Stroke circle = makeStroke(StrokeMode::Paint,
            QColor(235, 165, 35, 210),
            42.0,
            922,
            {QPointF(112.0, 74.0)});
        circle.brush.antialiasing = true;
        circle.brush.sizeDynamics = 0.0;
        circle.brush.wobbleScale = 0.0;
        layer.strokes.append(circle);

        Stroke resize;
        resize.mode = StrokeMode::Reframe;
        resize.reframeOp = ReframeOp{ReframeMode::Image,
            SamplingMode::Smooth,
            QSize(160, 120),
            document.size,
            QPoint()};
        layer.strokes.append(resize);

        const QSize outputSize(24, 18);
        RenderEngine::ScaledRenderStats stats;
        const QImage preview = RenderEngine::renderScaled(document,
            0,
            outputSize,
            RenderEngine::ScaledRenderMode::DisplayPreview,
            &stats);
        const QImage exact = RenderEngine::renderScaled(document,
            0,
            outputSize,
            RenderEngine::ScaledRenderMode::NativeExact);
        QVERIFY(!preview.isNull());
        QVERIFY(!exact.isNull());
        const ImageDifference difference = imageDifference(preview, exact);
        QVERIFY(difference.comparedChannels > 0);
        const qreal meanChannelDifference =
            static_cast<qreal>(difference.channelDifference)
            / static_cast<qreal>(difference.comparedChannels);
        QVERIFY2(meanChannelDifference < 8.0,
            qPrintable(QStringLiteral("mean channel difference: %1")
                    .arg(meanChannelDifference)));
        QVERIFY(difference.visiblyDifferentPixels
                < outputSize.width() * outputSize.height() / 5);
        QVERIFY(stats.usedDisplayScaleReplay);
        QCOMPARE(stats.largestIntermediateImageSize, QSize(40, 30));
        QVERIFY(stats.largestIntermediateImageBytes
                < static_cast<quint64>(160) * 120 * sizeof(QRgb));
    }

    void previewsAnimatedSelectionTransformInOperationOrder()
    {
        Document document = Document::createDefault(QSize(160, 120));
        document.background = Qt::transparent;
        document.animationFrames = 8;
        document.wobbleAmount = 7.0;
        Layer &layer = document.layers.first();
        layer.initialCanvasSize = document.size;

        Stroke animated = makeStroke(StrokeMode::Paint,
            QColor(205, 55, 120),
            18.0,
            931,
            {QPointF(20.0, 72.0),
                QPointF(54.0, 28.0),
                QPointF(92.0, 76.0),
                QPointF(124.0, 32.0)});
        animated.brush.antialiasing = true;
        animated.brush.animatedJitter = true;
        layer.strokes.append(animated);

        const QImage selection =
            rectangularMask(document.size, QRect(8, 12, 132, 84));
        QTransform transform;
        transform.translate(76.0, 58.0);
        transform.rotate(9.0);
        transform.translate(-68.0, -54.0);
        const std::optional<PixelSelectionOp> pixelOperation =
            makePixelSelectionOp(selection, transform, true, true);
        QVERIFY(pixelOperation.has_value());
        QCOMPARE(pixelOperation->sampling, SamplingMode::Smooth);
        Stroke operation;
        operation.mode = StrokeMode::PixelSelection;
        operation.pixelSelectionOp = *pixelOperation;
        layer.strokes.append(operation);

        Stroke overlay = makeStroke(StrokeMode::Paint,
            QColor(35, 190, 115),
            10.0,
            932,
            {QPointF(24.0, 18.0), QPointF(132.0, 102.0)});
        overlay.brush.antialiasing = true;
        overlay.brush.wobbleScale = 0.0;
        layer.strokes.append(overlay);

        const QSize outputSize(40, 30);
        QVector<QImage> previews;
        for (const int frame : {0, 1, 5})
        {
            const QImage preview =
                RenderEngine::renderScaled(document, frame, outputSize);
            const QImage exact = RenderEngine::renderScaled(document,
                frame,
                outputSize,
                RenderEngine::ScaledRenderMode::NativeExact);
            QVERIFY(!preview.isNull());
            QVERIFY(!exact.isNull());
            const ImageDifference difference =
                imageDifference(preview, exact, 36);
            const qreal meanChannelDifference =
                static_cast<qreal>(difference.channelDifference)
                / static_cast<qreal>(difference.comparedChannels);
            QVERIFY2(meanChannelDifference < 12.0,
                qPrintable(
                    QStringLiteral("frame %1 mean channel difference: %2")
                        .arg(frame)
                        .arg(meanChannelDifference)));
            QVERIFY(difference.visiblyDifferentPixels
                    < outputSize.width() * outputSize.height() / 4);
            previews.append(preview);
        }
        QVERIFY(previews[0] != previews[1]);
        QVERIFY(previews[1] != previews[2]);
    }

    void composesPreviewLayerSplitWithFramebufferOperations()
    {
        Document document = Document::createDefault(QSize(128, 96));
        document.background = QColor(248, 245, 238);
        document.wobbleAmount = 0.0;
        Layer &bottom = document.layers.first();
        bottom.initialCanvasSize = document.size;
        Stroke bottomStroke = makeStroke(StrokeMode::Paint,
            QColor(70, 110, 210),
            28.0,
            941,
            {QPointF(28.0, 28.0)});
        bottomStroke.brush.tipShape = BrushTipShape::Square;
        bottomStroke.brush.sizeDynamics = 0.0;
        bottom.strokes.append(bottomStroke);

        Layer middle;
        middle.name = QStringLiteral("operation layer");
        middle.initialCanvasSize = document.size;
        Stroke middleStroke = makeStroke(StrokeMode::Paint,
            QColor(220, 70, 50),
            32.0,
            942,
            {QPointF(36.0, 52.0)});
        middleStroke.brush.tipShape = BrushTipShape::Square;
        middleStroke.brush.sizeDynamics = 0.0;
        middle.strokes.append(middleStroke);
        const QImage selection =
            rectangularMask(document.size, QRect(20, 36, 32, 32));
        QTransform shift;
        shift.translate(52.0, 8.0);
        const std::optional<PixelSelectionOp> pixelOperation =
            makePixelSelectionOp(selection, shift, true, true);
        QVERIFY(pixelOperation.has_value());
        Stroke operation;
        operation.mode = StrokeMode::PixelSelection;
        operation.pixelSelectionOp = *pixelOperation;
        middle.strokes.append(operation);
        const QUuid middleId = middle.id;
        document.layers.append(middle);

        Layer top;
        top.name = QStringLiteral("top");
        top.initialCanvasSize = document.size;
        top.strokes.append(makeStroke(StrokeMode::Paint,
            QColor(30, 180, 95),
            12.0,
            943,
            {QPointF(12.0, 84.0), QPointF(116.0, 12.0)}));
        document.layers.append(top);

        const QSize outputSize(32, 24);
        RenderEngine::ScaledRenderStats stats;
        const RenderEngine::LayerSplitFrame split =
            RenderEngine::renderLayerSplit(document,
                0,
                outputSize,
                middleId,
                RenderEngine::ScaledRenderMode::DisplayPreview,
                &stats);
        QVERIFY(split.valid);
        const QImage composed =
            RenderEngine::composeLayerSplit(split, split.layerBase);
        const QImage full = RenderEngine::renderScaled(document, 0, outputSize);
        QCOMPARE(composed, full);
        QVERIFY(stats.usedDisplayScaleReplay);
        QVERIFY(stats.packedSelectionSamples > 0);
        QVERIFY(stats.largestIntermediateImageBytes
                <= static_cast<quint64>(outputSize.width())
                       * outputSize.height() * sizeof(QRgb));
        const quint64 frameBytes = static_cast<quint64>(outputSize.width())
                                   * outputSize.height() * sizeof(QRgb);
        QVERIFY(stats.maximumEstimatedWorkingSetBytes <= frameBytes * 4);
    }

    void replaysPendingSelectionOnCachedLayerFramebuffer()
    {
        Document document = Document::createDefault(QSize(128, 96));
        document.background = QColor(248, 245, 238);
        document.wobbleAmount = 0.0;
        Layer &target = document.layers.first();
        const QUuid targetId = target.id;
        target.initialCanvasSize = document.size;
        Stroke block = makeStroke(StrokeMode::Paint,
            QColor(45, 105, 225),
            32.0,
            944,
            {QPointF(32.0, 48.0)});
        block.brush.tipShape = BrushTipShape::Square;
        block.brush.antialiasing = false;
        block.brush.sizeDynamics = 0.0;
        block.brush.wobbleScale = 0.0;
        target.strokes.append(block);

        Layer top;
        top.name = QStringLiteral("top");
        top.initialCanvasSize = document.size;
        top.strokes.append(makeStroke(StrokeMode::Paint,
            QColor(220, 60, 70),
            10.0,
            945,
            {QPointF(8.0, 88.0), QPointF(120.0, 8.0)}));
        document.layers.append(top);

        const QImage selection =
            rectangularMask(document.size, QRect(16, 32, 32, 32));
        QTransform shift;
        shift.translate(64.0, 8.0);
        const std::optional<PixelSelectionOp> operation =
            makePixelSelectionOp(selection, shift, true, true);
        QVERIFY(operation.has_value());

        const QSize outputSize(32, 24);
        const RenderEngine::LayerSplitFrame split =
            RenderEngine::renderLayerSplit(document, 0, outputSize, targetId);
        QVERIFY(split.valid);
        const QImage cachedBase = split.layerBase;
        QImage previewLayer = split.layerBase;
        RenderEngine::ScaledRenderStats previewStats;
        QVERIFY(RenderEngine::replayPixelSelectionOnLayer(previewLayer,
            *operation,
            RenderEngine::ScaledRenderMode::DisplayPreview,
            &previewStats));
        QCOMPARE(split.layerBase, cachedBase);
        QVERIFY(previewStats.usedDisplayScaleReplay);
        QVERIFY(!previewStats.usedNativeExactFallback);
        QCOMPARE(previewStats.primitiveStrokesRendered, quint64(0));
        QCOMPARE(previewStats.pixelSelectionOperationsReplayed, quint64(1));
        QVERIFY(previewStats.packedSelectionSamples > 0);
        const RenderEngine::PixelSelectionPreviewRegion previewRegion =
            RenderEngine::replayPixelSelectionOnLayerRegion(
                split.layerBase, *operation);
        QVERIFY(previewRegion.valid);
        QVERIFY(!previewRegion.bounds.isEmpty());
        QImage regionalPreviewLayer = split.layerBase;
        QPainter regionalPreviewPainter(&regionalPreviewLayer);
        regionalPreviewPainter.setCompositionMode(
            QPainter::CompositionMode_Source);
        regionalPreviewPainter.drawImage(
            previewRegion.bounds.topLeft(), previewRegion.image);
        regionalPreviewPainter.end();
        QCOMPARE(regionalPreviewLayer, previewLayer);
        QVERIFY(
            previewRegion.image.sizeInBytes() < split.layerBase.sizeInBytes());

        Document expectedPreview = document;
        Stroke previewOperation;
        previewOperation.mode = StrokeMode::PixelSelection;
        previewOperation.pixelSelectionOp = *operation;
        expectedPreview.layers.first().strokes.append(previewOperation);
        QCOMPARE(RenderEngine::composeLayerSplit(split, previewLayer),
            RenderEngine::renderScaled(expectedPreview, 0, outputSize));

        const RenderEngine::LayerSplitFrame nativeSplit =
            RenderEngine::renderLayerSplit(
                document, 0, document.size, targetId);
        QVERIFY(nativeSplit.valid);
        QImage nativeLayer = nativeSplit.layerBase;
        RenderEngine::ScaledRenderStats nativeStats;
        QVERIFY(RenderEngine::replayPixelSelectionOnLayer(nativeLayer,
            *operation,
            RenderEngine::ScaledRenderMode::NativeExact,
            &nativeStats));
        QVERIFY(!nativeStats.usedDisplayScaleReplay);
        QVERIFY(nativeStats.usedNativeExactFallback);
        QCOMPARE(nativeStats.primitiveStrokesRendered, quint64(0));
        QCOMPARE(nativeStats.pixelSelectionOperationsReplayed, quint64(1));
        const RenderEngine::PixelSelectionPreviewRegion nativeRegion =
            RenderEngine::replayPixelSelectionOnLayerRegion(
                nativeSplit.layerBase, *operation);
        QVERIFY(nativeRegion.valid);
        QVERIFY(!nativeRegion.bounds.isEmpty());
        QImage regionalNativeLayer = nativeSplit.layerBase;
        QPainter regionalNativePainter(&regionalNativeLayer);
        regionalNativePainter.setCompositionMode(
            QPainter::CompositionMode_Source);
        regionalNativePainter.drawImage(
            nativeRegion.bounds.topLeft(), nativeRegion.image);
        regionalNativePainter.end();
        QCOMPARE(regionalNativeLayer, nativeLayer);
        QVERIFY(nativeRegion.image.sizeInBytes()
                < nativeSplit.layerBase.sizeInBytes());
        QCOMPARE(RenderEngine::composeLayerSplit(nativeSplit, nativeLayer),
            RenderEngine::render(expectedPreview, 0));
    }

    void replaysPendingSelectionWhenOnlyOnePreviewAxisShrinks()
    {
        const auto verify = [](const QSize &canvasSize,
                                const QSize &previewSize,
                                const QRect &selectionBounds,
                                const QPointF &translation)
        {
            Document document = Document::createDefault(canvasSize);
            document.background = Qt::transparent;
            document.wobbleAmount = 0.0;
            Layer &layer = document.layers.first();
            layer.initialCanvasSize = canvasSize;

            Stroke source = makeStroke(StrokeMode::Paint,
                QColor(45, 105, 225),
                1.0,
                946,
                {QPointF(selectionBounds.center())});
            source.brush.tipShape = BrushTipShape::Square;
            source.brush.antialiasing = false;
            source.brush.sizeDynamics = 0.0;
            source.brush.wobbleScale = 0.0;
            layer.strokes.append(source);

            const QImage selection =
                rectangularMask(canvasSize, selectionBounds);
            QTransform transform;
            transform.translate(translation.x(), translation.y());
            const std::optional<PixelSelectionOp> operation =
                makePixelSelectionOp(selection, transform, true, true);
            QVERIFY(operation.has_value());

            RenderEngine::ScaledRenderStats splitStats;
            const RenderEngine::LayerSplitFrame split =
                RenderEngine::renderLayerSplit(document,
                    0,
                    previewSize,
                    layer.id,
                    RenderEngine::ScaledRenderMode::DisplayPreview,
                    &splitStats);
            QVERIFY(split.valid);
            QVERIFY(splitStats.usedDisplayScaleReplay);
            QVERIFY(!splitStats.usedNativeExactFallback);

            QImage previewLayer = split.layerBase;
            RenderEngine::ScaledRenderStats replayStats;
            QVERIFY(RenderEngine::replayPixelSelectionOnLayer(previewLayer,
                *operation,
                RenderEngine::ScaledRenderMode::DisplayPreview,
                &replayStats));
            QVERIFY(replayStats.usedDisplayScaleReplay);
            QVERIFY(!replayStats.usedNativeExactFallback);
            QCOMPARE(replayStats.primitiveStrokesRendered, quint64(0));
            QCOMPARE(replayStats.pixelSelectionOperationsReplayed, quint64(1));

            Document expected = document;
            Stroke selectionOperation;
            selectionOperation.mode = StrokeMode::PixelSelection;
            selectionOperation.pixelSelectionOp = *operation;
            expected.layers.first().strokes.append(selectionOperation);
            QCOMPARE(RenderEngine::composeLayerSplit(split, previewLayer),
                RenderEngine::renderScaled(expected,
                    0,
                    previewSize,
                    RenderEngine::ScaledRenderMode::DisplayPreview));
        };

        verify(QSize(4096, 1),
            QSize(410, 1),
            QRect(512, 0, 1024, 1),
            QPointF(1024.0, 0.0));
        verify(QSize(1, 4096),
            QSize(1, 410),
            QRect(0, 512, 1, 1024),
            QPointF(0.0, 1024.0));
    }

    void cachedSelectionReplayDoesNotWalkTwentyThousandStrokes()
    {
        Document document = Document::createDefault(QSize(128, 128));
        document.background = Qt::transparent;
        document.wobbleAmount = 0.0;
        Layer &layer = document.layers.first();
        layer.initialCanvasSize = document.size;
        layer.strokes.reserve(DocumentLimits::maximumStrokesPerLayer);
        for (int index = 0; index < DocumentLimits::maximumStrokesPerLayer;
            ++index)
        {
            Stroke dot = makeStroke(StrokeMode::Paint,
                QColor(35, 95, 225, 24),
                1.0,
                static_cast<quint64>(index) + 1,
                {QPointF(32.0 + index % 64, 32.0 + (index / 64) % 64)});
            dot.brush.antialiasing = false;
            dot.brush.sizeDynamics = 0.0;
            dot.brush.wobbleScale = 0.0;
            layer.strokes.append(std::move(dot));
        }

        const QSize outputSize(32, 32);
        RenderEngine::ScaledRenderStats splitStats;
        const RenderEngine::LayerSplitFrame split =
            RenderEngine::renderLayerSplit(document,
                0,
                outputSize,
                layer.id,
                RenderEngine::ScaledRenderMode::DisplayPreview,
                &splitStats);
        QVERIFY(split.valid);
        QCOMPARE(splitStats.primitiveStrokesRendered,
            static_cast<quint64>(DocumentLimits::maximumStrokesPerLayer));

        const QImage selection =
            rectangularMask(document.size, QRect(24, 24, 80, 80));
        QTransform shift;
        shift.translate(8.0, 0.0);
        const std::optional<PixelSelectionOp> operation =
            makePixelSelectionOp(selection, shift, true, true);
        QVERIFY(operation.has_value());
        const QImage cachedBase = split.layerBase;
        for (int preview = 0; preview < 4; ++preview)
        {
            QImage layerPreview = split.layerBase;
            RenderEngine::ScaledRenderStats replayStats;
            QVERIFY(RenderEngine::replayPixelSelectionOnLayer(layerPreview,
                *operation,
                RenderEngine::ScaledRenderMode::DisplayPreview,
                &replayStats));
            QCOMPARE(replayStats.primitiveStrokesRendered, quint64(0));
            QCOMPARE(replayStats.pixelSelectionOperationsReplayed, quint64(1));
            QVERIFY(replayStats.packedSelectionSamples > 0);
            QCOMPARE(split.layerBase, cachedBase);
        }
    }

    void fourKOperationPreviewNeverAllocatesNativeFramebuffer()
    {
        constexpr int edge = DocumentLimits::maximumCanvasEdge;
        const QSize canvasSize(edge, edge);
        Document document = Document::createDefault(canvasSize);
        document.background = Qt::transparent;
        document.wobbleAmount = 0.0;
        Layer &layer = document.layers.first();
        layer.initialCanvasSize = canvasSize;

        Stroke block = makeStroke(StrokeMode::Paint,
            QColor(65, 125, 230),
            512.0,
            951,
            {QPointF(edge / 2.0, edge / 2.0)});
        block.brush.tipShape = BrushTipShape::Square;
        block.brush.antialiasing = false;
        block.brush.sizeDynamics = 0.0;
        block.brush.wobbleScale = 0.0;
        layer.strokes.append(block);

        PixelSelectionOp pixelOperation;
        pixelOperation.canvasSize = canvasSize;
        pixelOperation.sourceBounds = QRect(QPoint(), canvasSize);
        const qsizetype packedStride = (static_cast<qsizetype>(edge) + 7) / 8;
        pixelOperation.packedMask =
            QByteArray(packedStride * edge, static_cast<char>(0xff));
        pixelOperation.transform.translate(64.0, 0.0);
        pixelOperation.sampling = SamplingMode::Nearest;
        pixelOperation.clearSource = true;
        pixelOperation.drawDestination = true;
        QVERIFY(isValidPixelSelectionOp(pixelOperation));
        Stroke operation;
        operation.mode = StrokeMode::PixelSelection;
        operation.pixelSelectionOp = pixelOperation;
        layer.strokes.append(operation);

        const QSize outputSize(256, 256);
        RenderEngine::ScaledRenderStats stats;
        const QImage preview = RenderEngine::renderScaled(document,
            0,
            outputSize,
            RenderEngine::ScaledRenderMode::DisplayPreview,
            &stats);
        QVERIFY(!preview.isNull());
        QCOMPARE(preview.size(), outputSize);
        QVERIFY(stats.usedDisplayScaleReplay);
        QVERIFY(!stats.usedNativeExactFallback);
        QCOMPARE(stats.largestIntermediateImageSize, outputSize);
        const quint64 displayFrameBytes =
            static_cast<quint64>(outputSize.width()) * outputSize.height()
            * sizeof(QRgb);
        const quint64 nativeFrameBytes =
            static_cast<quint64>(edge) * edge * sizeof(QRgb);
        QCOMPARE(stats.largestIntermediateImageBytes, displayFrameBytes);
        QVERIFY(stats.maximumEstimatedWorkingSetBytes < nativeFrameBytes);
        QCOMPARE(stats.packedSelectionSamples,
            static_cast<quint64>(outputSize.width()) * outputSize.height());
        QCOMPARE(pixelOperation.packedMask.size(), qsizetype(2 * 1024 * 1024));
    }
    void discardsRendersCancelledBeforeTheFirstBoundary()
    {
        const Document document = animatedDocument();
        const QSize previewSize(48, 36);
        std::atomic_bool cancellation{true};

        QVERIFY(RenderEngine::renderScaled(document,
            5,
            previewSize,
            RenderEngine::ScaledRenderMode::DisplayPreview,
            nullptr,
            &cancellation)
                .isNull());
        QVERIFY(RenderEngine::renderScaled(document,
            5,
            document.size,
            RenderEngine::ScaledRenderMode::NativeExact,
            nullptr,
            &cancellation)
                .isNull());
        QVERIFY(RenderEngine::renderScaledRegion(document,
            5,
            previewSize,
            QRect(4, 4, 16, 12),
            nullptr,
            &cancellation)
                .isNull());
        QVERIFY(!RenderEngine::renderLayerSplit(document,
            5,
            previewSize,
            document.layers.first().id,
            RenderEngine::ScaledRenderMode::DisplayPreview,
            nullptr,
            &cancellation)
                .valid);
        QVERIFY(!RenderEngine::renderLayerRasterFrame(document,
            5,
            previewSize,
            qint64{64} * 1024 * 1024,
            RenderEngine::ScaledRenderMode::DisplayPreview,
            nullptr,
            &cancellation)
                .valid);
    }

    void rendersIdenticallyWithAnUntriggeredCancellationToken()
    {
        const Document document = animatedDocument();
        const QSize previewSize(48, 36);
        std::atomic_bool cancellation{false};

        const QImage baseline =
            RenderEngine::renderScaled(document, 5, previewSize);
        const QImage watched = RenderEngine::renderScaled(document,
            5,
            previewSize,
            RenderEngine::ScaledRenderMode::DisplayPreview,
            nullptr,
            &cancellation);
        QVERIFY(!watched.isNull());
        QCOMPARE(watched, baseline);

        const QRect region(4, 4, 16, 12);
        const QImage baselineRegion =
            RenderEngine::renderScaledRegion(document, 5, previewSize, region);
        const QImage watchedRegion = RenderEngine::renderScaledRegion(
            document, 5, previewSize, region, nullptr, &cancellation);
        QVERIFY(!watchedRegion.isNull());
        QCOMPARE(watchedRegion, baselineRegion);

        const RenderEngine::LayerSplitFrame split =
            RenderEngine::renderLayerSplit(document,
                5,
                previewSize,
                document.layers.first().id,
                RenderEngine::ScaledRenderMode::DisplayPreview,
                nullptr,
                &cancellation);
        QVERIFY(split.valid);
        QCOMPARE(
            RenderEngine::composeLayerSplit(split, split.layerBase), baseline);
    }
};

int runRenderPreviewTests(int argc, char **argv)
{
    RenderPreviewTests tests;
    return QTest::qExec(&tests, argc, argv);
}

}

#include "RenderPreviewTests.moc"

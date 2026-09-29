// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "support/RenderTestHelpers.hpp"
#include "support/RenderTestSuites.hpp"

namespace ugurugu
{

class LayerCompositionTests final : public QObject
{
    Q_OBJECT

private slots:
    void keepsTransparentBackgroundUnpaintedInRenderedFrame()
    {
        Document document = Document::createDefault(QSize(16, 16));
        document.background = QColor(0, 0, 0, 0);
        document.wobbleAmount = 0.0;
        Layer &layer = document.layers.first();

        Stroke stroke = makeStroke(StrokeMode::Paint,
            QColor(210, 70, 125),
            8.0,
            1,
            {QPointF(8.0, 8.0)});
        stroke.brush.tipShape = BrushTipShape::Square;
        stroke.brush.sizeDynamics = 0.0;
        stroke.brush.wobbleScale = 0.0;
        stroke.brush.antialiasing = false;
        layer.strokes.append(stroke);

        const QImage rendered = RenderEngine::render(document, 0);
        QVERIFY(!rendered.isNull());
        QVERIFY(rendered.hasAlphaChannel());
        // The painted centre stays opaque while an untouched corner keeps the
        // transparency that PNG and GIF export rely on.
        QCOMPARE(rendered.pixelColor(8, 8).alpha(), 255);
        QCOMPARE(rendered.pixelColor(0, 0).alpha(), 0);
    }

    void rendersLayerBlendModes_data()
    {
        QTest::addColumn<int>("blendMode");
        QTest::addColumn<int>("compositionMode");

        QTest::newRow("normal")
            << static_cast<int>(LayerBlendMode::Normal)
            << static_cast<int>(QPainter::CompositionMode_SourceOver);
        QTest::newRow("multiply")
            << static_cast<int>(LayerBlendMode::Multiply)
            << static_cast<int>(QPainter::CompositionMode_Multiply);
        QTest::newRow("screen")
            << static_cast<int>(LayerBlendMode::Screen)
            << static_cast<int>(QPainter::CompositionMode_Screen);
        QTest::newRow("overlay")
            << static_cast<int>(LayerBlendMode::Overlay)
            << static_cast<int>(QPainter::CompositionMode_Overlay);
    }

    void rendersLayerBlendModes()
    {
        QFETCH(int, blendMode);
        QFETCH(int, compositionMode);

        const QColor background(60, 130, 210);
        const QColor source(210, 70, 125);
        Document document = Document::createDefault(QSize(16, 16));
        document.background = background;
        document.wobbleAmount = 0.0;
        Layer &layer = document.layers.first();
        layer.opacity = 0.75;
        layer.blendMode = static_cast<LayerBlendMode>(blendMode);
        Stroke stroke =
            makeStroke(StrokeMode::Paint, source, 16.0, 1, {QPointF(8.0, 8.0)});
        stroke.brush.tipShape = BrushTipShape::Square;
        stroke.brush.sizeDynamics = 0.0;
        stroke.brush.wobbleScale = 0.0;
        stroke.brush.antialiasing = false;
        layer.strokes.append(stroke);

        const QImage rendered = RenderEngine::render(document, 0);
        QVERIFY(!rendered.isNull());

        QImage expected(1, 1, QImage::Format_ARGB32_Premultiplied);
        expected.fill(background);
        QPainter painter(&expected);
        painter.setCompositionMode(
            static_cast<QPainter::CompositionMode>(compositionMode));
        painter.setOpacity(layer.opacity);
        painter.fillRect(expected.rect(), source);
        painter.end();

        const QColor actual = rendered.pixelColor(8, 8);
        const QColor reference = expected.pixelColor(0, 0);
        QVERIFY(std::abs(actual.red() - reference.red()) <= 1);
        QVERIFY(std::abs(actual.green() - reference.green()) <= 1);
        QVERIFY(std::abs(actual.blue() - reference.blue()) <= 1);
        QCOMPARE(actual.alpha(), reference.alpha());
    }

    void preservesHierarchyOpacityBlendAndClippingPixels()
    {
        const QSize size(5, 4);
        Document document = Document::createDefault(size);
        document.background = QColor(35, 55, 80, 230);
        document.layers.clear();
        document.activeLayerId = QUuid();

        Layer rootBase;
        rootBase.name = QStringLiteral("Root base");
        rootBase.opacity = 0.8;
        rootBase.blendMode = LayerBlendMode::Multiply;
        rootBase.initialCanvasSize = size;

        Layer group;
        group.name = QStringLiteral("Group");
        group.kind = LayerKind::Group;
        group.opacity = 0.7;
        group.blendMode = LayerBlendMode::Screen;
        group.initialCanvasSize = size;

        Layer childBase;
        childBase.name = QStringLiteral("Child base");
        childBase.parentGroupId = group.id;
        childBase.opacity = 0.65;
        childBase.blendMode = LayerBlendMode::Overlay;
        childBase.initialCanvasSize = size;

        Layer childClipped;
        childClipped.name = QStringLiteral("Child clipped");
        childClipped.parentGroupId = group.id;
        childClipped.clipToLayerBelow = true;
        childClipped.opacity = 0.55;
        childClipped.blendMode = LayerBlendMode::Multiply;
        childClipped.initialCanvasSize = size;

        Layer top;
        top.name = QStringLiteral("Top");
        top.clipToLayerBelow = true;
        top.opacity = 0.35;
        top.blendMode = LayerBlendMode::Screen;
        top.initialCanvasSize = size;

        document.layers = {rootBase, group, childBase, childClipped, top};
        document.activeLayerId = rootBase.id;
        QVERIFY(!DocumentSerializer::toJson(document).isEmpty());

        RenderEngine::LayerRasterFrame frame;
        frame.outputSize = size;
        frame.paintLayers.insert(rootBase.id,
            patternedSurface(
                size, QColor(220, 45, 60, 210), QColor(45, 190, 100, 120)));
        frame.paintLayers.insert(childBase.id,
            patternedSurface(
                size, QColor(40, 95, 225, 190), QColor(230, 170, 40, 90)));
        frame.paintLayers.insert(childClipped.id,
            patternedSurface(
                size, QColor(220, 220, 245, 230), QColor(65, 25, 180, 160)));
        frame.paintLayers.insert(top.id,
            patternedSurface(
                size, QColor(20, 200, 210, 150), QColor(240, 80, 160, 70)));
        frame.valid = true;

        const QImage expected = referenceHierarchyComposition(
            document, frame.paintLayers, frame.outputSize);
        const QImage actual =
            RenderEngine::composeLayerRasterFrame(document, frame, {}, {});
        QVERIFY(!expected.isNull());
        QCOMPARE(actual, expected);
    }

    void preservesHiddenGroupSkipAndClippingResetPixels()
    {
        const QSize size(4, 3);
        Document document = Document::createDefault(size);
        document.background = QColor(30, 45, 65);
        document.layers.clear();

        Layer base;
        base.name = QStringLiteral("Base");
        base.initialCanvasSize = size;

        Layer hiddenGroup;
        hiddenGroup.name = QStringLiteral("Hidden group");
        hiddenGroup.kind = LayerKind::Group;
        hiddenGroup.visible = false;
        hiddenGroup.initialCanvasSize = size;

        Layer hiddenChild;
        hiddenChild.name = QStringLiteral("Hidden child");
        hiddenChild.parentGroupId = hiddenGroup.id;
        hiddenChild.initialCanvasSize = size;

        Layer clipped;
        clipped.name = QStringLiteral("Clipped after hidden group");
        clipped.clipToLayerBelow = true;
        clipped.initialCanvasSize = size;

        document.layers = {base, hiddenGroup, hiddenChild, clipped};
        document.activeLayerId = base.id;
        QVERIFY(!DocumentSerializer::toJson(document).isEmpty());

        RenderEngine::LayerRasterFrame frame;
        frame.outputSize = size;
        frame.paintLayers.insert(base.id,
            patternedSurface(
                size, QColor(210, 50, 65, 220), QColor(45, 175, 95, 130)));
        frame.paintLayers.insert(hiddenChild.id,
            patternedSurface(
                size, QColor(30, 80, 220, 240), QColor(230, 180, 45, 180)));
        frame.paintLayers.insert(clipped.id,
            patternedSurface(
                size, QColor(245, 245, 245, 255), QColor(20, 20, 20, 255)));
        frame.valid = true;

        const QImage expected = referenceHierarchyComposition(
            document, frame.paintLayers, frame.outputSize);
        const QImage actual =
            RenderEngine::composeLayerRasterFrame(document, frame, {}, {});
        QCOMPARE(actual, expected);
        const LayerCompositionMemoryEstimate estimate =
            RenderEngine::estimateHierarchyMemory(document, size);
        QVERIFY(estimate.valid);
        QCOMPARE(estimate.peakSurfaceCount, 2);
    }

    void measuresAdversarialHierarchyPeakSurfaces()
    {
        const Document document = adversarialClippedHierarchy(QSize(2, 2), 8);
        QVERIFY(!DocumentSerializer::toJson(document).isEmpty());
        const LayerCompositionMemoryEstimate estimate =
            RenderEngine::estimateHierarchyMemory(document, QSize(1, 1));
        QVERIFY(estimate.valid);
        QCOMPARE(estimate.peakSurfaceCount, 11);
        QCOMPARE(estimate.bytesPerSurface, 4ULL);
        QCOMPARE(estimate.peakBytes, 44ULL);

        RenderEngine::ScaledRenderStats stats;
        const QImage rendered = RenderEngine::renderScaled(document,
            0,
            QSize(1, 1),
            RenderEngine::ScaledRenderMode::DisplayPreview,
            &stats);
        QVERIFY(!rendered.isNull());
        QCOMPARE(stats.hierarchyPlannedPeakSurfaceCount, 11);
        QCOMPARE(stats.hierarchyPeakSurfaceCount, 11);
        QCOMPARE(stats.hierarchyPeakSurfaceBytes, 44ULL);
        QVERIFY(stats.hierarchySurfaceReuses >= 8);
    }

    void handlesLegacyOverDepthHierarchyIteratively()
    {
        const Document document = adversarialClippedHierarchy(QSize(2, 2), 64);
        QVERIFY(!DocumentSerializer::toJson(document).isEmpty());
        const LayerCompositionMemoryEstimate estimate =
            RenderEngine::estimateHierarchyMemory(document, QSize(1, 1));
        QVERIFY(estimate.valid);
        QCOMPARE(estimate.peakSurfaceCount, 67);

        RenderEngine::ScaledRenderStats stats;
        const QImage rendered = RenderEngine::renderScaled(document,
            0,
            QSize(1, 1),
            RenderEngine::ScaledRenderMode::DisplayPreview,
            &stats);
        QVERIFY(!rendered.isNull());
        QCOMPARE(stats.hierarchyPlannedPeakSurfaceCount, 67);
        QCOMPARE(stats.hierarchyPeakSurfaceCount, 67);
    }

    void reusesCompositionSurfacesAcrossSiblingGroups()
    {
        Document document = Document::createDefault(QSize(2, 2));
        document.layers.clear();
        document.activeLayerId = QUuid();
        for (int index = 0; index < 2; ++index)
        {
            Layer group;
            group.name = QStringLiteral("Group %1").arg(index);
            group.kind = LayerKind::Group;
            group.initialCanvasSize = document.size;
            Layer child;
            child.name = QStringLiteral("Child %1").arg(index);
            child.parentGroupId = group.id;
            child.initialCanvasSize = document.size;
            document.layers.append(group);
            document.layers.append(child);
            if (document.activeLayerId.isNull())
            {
                document.activeLayerId = child.id;
            }
        }
        QVERIFY(!DocumentSerializer::toJson(document).isEmpty());

        RenderEngine::ScaledRenderStats stats;
        const QImage rendered = RenderEngine::renderScaled(document,
            0,
            QSize(1, 1),
            RenderEngine::ScaledRenderMode::DisplayPreview,
            &stats);
        QVERIFY(!rendered.isNull());
        QVERIFY(stats.hierarchySurfaceReuses > 0);
        QCOMPARE(stats.hierarchyPeakSurfaceCount,
            stats.hierarchyPlannedPeakSurfaceCount);
    }

    void includesHierarchyTransientSurfacesInFourKPreviewBudget()
    {
        const QSize fourK(4096, 4096);
        const Document document = adversarialClippedHierarchy(fourK, 64);
        QVERIFY(!DocumentSerializer::toJson(document).isEmpty());
        const LayerCompositionMemoryEstimate estimate =
            RenderEngine::estimateHierarchyMemory(document, fourK);
        QVERIFY(estimate.valid);
        QCOMPARE(estimate.peakSurfaceCount, 67);
        QCOMPARE(estimate.bytesPerSurface, 64ULL * 1024ULL * 1024ULL);
        // Against the ceiling rather than the granted budget: the granted one
        // follows installed memory, and the point being made is that a deep 4K
        // hierarchy overflows even the most memory this app will ever take.
        QVERIFY(estimate.peakBytes
                > static_cast<quint64>(MemoryBudget::maximumPreviewCacheKiB)
                      * 1024ULL);

        const QSize preview = PreviewRenderPolicy::renderSize(
            fourK, 16.0, 1, estimate.peakSurfaceCount);
        QVERIFY(preview.isValid());
        QVERIFY(preview.width() < fourK.width());
        QCOMPARE(preview.width(), preview.height());
        const quint64 concurrentSurfaceCount =
            static_cast<quint64>(estimate.peakSurfaceCount)
            + static_cast<quint64>(
                LayerCompositionPlan::paintOperationScratchSurfaceCount);
        const quint64 previewBytes = static_cast<quint64>(preview.width())
                                     * static_cast<quint64>(preview.height())
                                     * sizeof(quint32) * concurrentSurfaceCount;
        QVERIFY(previewBytes
                <= static_cast<quint64>(PreviewRenderPolicy::maximumCacheKiB())
                       * 1024ULL);
    }

    void rejectsOverflowingHierarchyMemoryEstimate()
    {
        const Document document = Document::createDefault(QSize(1, 1));
        const int maximum = std::numeric_limits<int>::max();
        const LayerCompositionMemoryEstimate estimate =
            RenderEngine::estimateHierarchyMemory(
                document, QSize(maximum, maximum));
        QVERIFY(!estimate.valid);
    }

    void rejectsClippedGroupCompositionPlan()
    {
        Document document = Document::createDefault(QSize(16, 16));
        Layer group;
        group.name = QStringLiteral("Invalid clipped group");
        group.kind = LayerKind::Group;
        group.clipToLayerBelow = true;
        group.initialCanvasSize = document.size;
        document.layers.append(group);
        QVERIFY(!RenderEngine::estimateHierarchyMemory(document, document.size)
                .valid);
        QVERIFY(RenderEngine::render(document, 0).isNull());
    }

    void clipsLayerToBaseAlphaWithinGroup()
    {
        Document document = Document::createDefault(QSize(32, 32));
        document.background = Qt::white;
        document.wobbleAmount = 0.0;
        Layer &base = document.layers.first();
        Stroke baseStroke = makeStroke(StrokeMode::Paint,
            QColor(220, 40, 50),
            8.0,
            1,
            {QPointF(16.0, 16.0)});
        baseStroke.brush.tipShape = BrushTipShape::Square;
        baseStroke.brush.sizeDynamics = 0.0;
        baseStroke.brush.wobbleScale = 0.0;
        baseStroke.brush.antialiasing = false;
        base.strokes.append(baseStroke);

        Layer clipped;
        clipped.name = QStringLiteral("Clipped");
        clipped.initialCanvasSize = document.size;
        clipped.clipToLayerBelow = true;
        Stroke clippedStroke = makeStroke(StrokeMode::Paint,
            QColor(30, 80, 220),
            24.0,
            2,
            {QPointF(16.0, 16.0)});
        clippedStroke.brush.tipShape = BrushTipShape::Square;
        clippedStroke.brush.sizeDynamics = 0.0;
        clippedStroke.brush.wobbleScale = 0.0;
        clippedStroke.brush.antialiasing = false;
        clipped.strokes.append(clippedStroke);
        document.layers.append(clipped);

        const QImage rendered = RenderEngine::render(document, 0);
        QVERIFY(!rendered.isNull());
        QCOMPARE(rendered.pixelColor(16, 16), QColor(30, 80, 220));
        QCOMPARE(rendered.pixelColor(7, 16), QColor(Qt::white));

        document.layers.last().clipToLayerBelow = false;
        const QImage unclipped = RenderEngine::render(document, 0);
        QCOMPARE(unclipped.pixelColor(7, 16), QColor(30, 80, 220));
    }

    void isolatesLayerGroupComposition()
    {
        Document document = Document::createDefault(QSize(24, 24));
        document.background = Qt::white;
        document.wobbleAmount = 0.0;
        Layer &child = document.layers.first();
        Stroke stroke = makeStroke(StrokeMode::Paint,
            QColor(200, 20, 40),
            12.0,
            3,
            {QPointF(12.0, 12.0)});
        stroke.brush.tipShape = BrushTipShape::Square;
        stroke.brush.sizeDynamics = 0.0;
        stroke.brush.wobbleScale = 0.0;
        stroke.brush.antialiasing = false;
        child.strokes.append(stroke);

        Layer group;
        group.name = QStringLiteral("Group");
        group.kind = LayerKind::Group;
        group.opacity = 0.5;
        group.initialCanvasSize = document.size;
        child.parentGroupId = group.id;
        const QUuid childId = child.id;
        document.layers.append(group);

        const QImage rendered = RenderEngine::render(document, 0);
        QVERIFY(!rendered.isNull());
        const QColor center = rendered.pixelColor(12, 12);
        QVERIFY(std::abs(center.red() - 227) <= 1);
        QVERIFY(std::abs(center.green() - 137) <= 1);
        QVERIFY(std::abs(center.blue() - 147) <= 1);

        const RenderEngine::LayerSplitFrame split =
            RenderEngine::renderLayerSplit(document, 0, document.size, childId);
        QVERIFY(!split.valid);
    }

    void cachesHierarchicalLayerRastersForInteraction()
    {
        Document document = Document::createDefault(QSize(48, 48));
        document.background = Qt::white;
        document.wobbleAmount = 0.0;
        Layer &base = document.layers.first();
        base.strokes.append(makeStroke(StrokeMode::Paint,
            QColor(220, 50, 60),
            20.0,
            1,
            {QPointF(24.0, 24.0)}));

        Layer clipped;
        clipped.name = QStringLiteral("Clipped");
        clipped.initialCanvasSize = document.size;
        clipped.clipToLayerBelow = true;
        clipped.opacity = 0.75;
        clipped.blendMode = LayerBlendMode::Overlay;
        clipped.strokes.append(makeStroke(StrokeMode::Paint,
            QColor(40, 90, 220),
            8.0,
            2,
            {QPointF(24.0, 24.0)}));

        Layer group;
        group.name = QStringLiteral("Group");
        group.kind = LayerKind::Group;
        group.initialCanvasSize = document.size;
        group.opacity = 0.65;
        base.parentGroupId = group.id;
        clipped.parentGroupId = group.id;
        const QUuid clippedId = clipped.id;
        document.layers.append(clipped);
        document.layers.append(group);

        RenderEngine::ScaledRenderStats stats;
        const RenderEngine::LayerRasterFrame frame =
            RenderEngine::renderLayerRasterFrame(document,
                0,
                document.size,
                4LL * 1024 * 1024,
                RenderEngine::ScaledRenderMode::DisplayPreview,
                &stats);
        QVERIFY(frame.valid);
        QCOMPARE(frame.paintLayers.size(), 2);

        Stroke active = makeStroke(StrokeMode::Paint,
            QColor(40, 180, 90),
            6.0,
            3,
            {QPointF(24.0, 24.0)});
        Document expected = document;
        expected.layer(clippedId)->strokes.append(active);
        QImage activeLayer = frame.paintLayers.value(clippedId);
        QVERIFY(RenderEngine::renderStrokesOnLayer(
            activeLayer, document, {active}, 0, document.size));
        QCOMPARE(RenderEngine::composeLayerRasterFrame(
                     document, frame, clippedId, activeLayer),
            RenderEngine::render(expected, 0));

        const QSize previewSize(31, 29);
        const RenderEngine::LayerRasterFrame previewFrame =
            RenderEngine::renderLayerRasterFrame(document,
                0,
                previewSize,
                4LL * 1024 * 1024,
                RenderEngine::ScaledRenderMode::DisplayPreview);
        QVERIFY(previewFrame.valid);
        QImage fullPreviewLayer = previewFrame.paintLayers.value(clippedId);
        QVERIFY(RenderEngine::renderStrokesOnLayer(
            fullPreviewLayer, document, {active}, 0, previewSize));
        const QImage fullPreview = RenderEngine::composeLayerRasterFrame(
            document, previewFrame, clippedId, fullPreviewLayer);
        QCOMPARE(
            fullPreview, RenderEngine::renderScaled(expected, 0, previewSize));

        const QRect previewBounds =
            RenderEngine::strokePreviewBounds(document, active, previewSize);
        QVERIFY(!previewBounds.isEmpty());
        QVERIFY(previewBounds != QRect(QPoint(), previewSize));
        QImage regionalLayer =
            previewFrame.paintLayers.value(clippedId).copy(previewBounds);
        QVERIFY(RenderEngine::renderStrokesOnLayerRegion(
            regionalLayer, document, {active}, 0, previewSize, previewBounds));
        QCOMPARE(RenderEngine::composeLayerRasterFrameRegion(document,
                     previewFrame,
                     clippedId,
                     regionalLayer,
                     previewBounds),
            fullPreview.copy(previewBounds));
        QCOMPARE(RenderEngine::composeLayerRasterFrameRegion(document,
                     previewFrame,
                     clippedId,
                     fullPreviewLayer,
                     QRect(QPoint(), previewSize)),
            fullPreview);

        const RenderEngine::LayerRasterFrame constrained =
            RenderEngine::renderLayerRasterFrame(document, 0, document.size, 1);
        QVERIFY(!constrained.valid);
    }
};

int runLayerCompositionTests(int argc, char **argv)
{
    LayerCompositionTests tests;
    return QTest::qExec(&tests, argc, argv);
}

}

#include "LayerCompositionTests.moc"

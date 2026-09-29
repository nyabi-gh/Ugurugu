// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "support/RenderTestHelpers.hpp"
#include "support/RenderTestSuites.hpp"

namespace ugurugu
{

class LayerSplitPreviewTests final : public QObject
{
    Q_OBJECT

private slots:
    void handlesBlendModesInLayerSplitPreviews()
    {
        Document document = Document::createDefault(QSize(48, 36));
        document.wobbleAmount = 0.0;
        Layer &target = document.layers.first();
        target.blendMode = LayerBlendMode::Overlay;
        target.strokes.append(makeStroke(StrokeMode::Paint,
            QColor(220, 70, 90),
            14.0,
            1,
            {QPointF(24.0, 18.0)}));
        const QUuid targetId = target.id;

        Layer top;
        top.name = QStringLiteral("Top");
        top.initialCanvasSize = document.size;
        top.strokes.append(makeStroke(StrokeMode::Paint,
            QColor(40, 180, 100),
            8.0,
            2,
            {QPointF(10.0, 10.0)}));
        document.layers.append(top);

        RenderEngine::LayerSplitFrame split = RenderEngine::renderLayerSplit(
            document, 0, document.size, targetId);
        QVERIFY(split.valid);
        QCOMPARE(RenderEngine::composeLayerSplit(split, split.layerBase),
            RenderEngine::render(document, 0));

        document.layers.last().blendMode = LayerBlendMode::Screen;
        split = RenderEngine::renderLayerSplit(
            document, 0, document.size, targetId);
        QVERIFY(!split.valid);
    }

    // A group or a clip chain that sits wholly on one side of the target is
    // self-contained, so it does not stop the split. Only one that straddles
    // the target does.
    void splitsAroundGroupsAndClipChains()
    {
        Document document = Document::createDefault(QSize(64, 48));
        document.wobbleAmount = 0.0;
        document.layers.clear();

        Layer group;
        group.name = QStringLiteral("Group");
        group.kind = LayerKind::Group;
        group.opacity = 0.7;
        group.blendMode = LayerBlendMode::Screen;
        group.initialCanvasSize = document.size;

        Layer childBase;
        childBase.name = QStringLiteral("Child base");
        childBase.parentGroupId = group.id;
        childBase.initialCanvasSize = document.size;
        childBase.strokes.append(makeStroke(StrokeMode::Paint,
            QColor(40, 180, 100),
            16.0,
            101,
            {QPointF(20.0, 20.0)}));

        Layer childClipped;
        childClipped.name = QStringLiteral("Child clipped");
        childClipped.parentGroupId = group.id;
        childClipped.clipToLayerBelow = true;
        childClipped.blendMode = LayerBlendMode::Multiply;
        childClipped.initialCanvasSize = document.size;
        childClipped.strokes.append(makeStroke(StrokeMode::Paint,
            QColor(200, 60, 40),
            14.0,
            102,
            {QPointF(24.0, 24.0)}));

        Layer target;
        target.name = QStringLiteral("Target");
        target.initialCanvasSize = document.size;
        target.strokes.append(makeStroke(StrokeMode::Paint,
            QColor(220, 200, 60),
            10.0,
            103,
            {QPointF(40.0, 24.0)}));

        Layer above;
        above.name = QStringLiteral("Above");
        above.opacity = 0.6;
        above.initialCanvasSize = document.size;
        above.strokes.append(makeStroke(StrokeMode::Paint,
            QColor(60, 90, 220),
            8.0,
            104,
            {QPointF(12.0, 40.0)}));

        document.layers = {childBase, childClipped, group, target, above};
        document.activeLayerId = target.id;
        const QUuid targetId = target.id;

        QVERIFY(RenderEngine::supportsLayerSplit(document, targetId));
        const RenderEngine::LayerSplitFrame split =
            RenderEngine::renderLayerSplit(
                document, 0, document.size, targetId);
        QVERIFY(split.valid);
        QCOMPARE(RenderEngine::composeLayerSplit(split, split.layerBase),
            RenderEngine::render(document, 0));

        Stroke active = makeStroke(StrokeMode::Paint,
            QColor(250, 240, 200),
            6.0,
            105,
            {QPointF(30.0, 12.0), QPointF(52.0, 36.0)});
        Document full = document;
        full.layer(targetId)->strokes.append(active);
        QImage layerImage = split.layerBase;
        QVERIFY(RenderEngine::renderStrokesOnLayer(
            layerImage, document, {active}, 0, document.size));
        QCOMPARE(RenderEngine::composeLayerSplit(split, layerImage),
            RenderEngine::render(full, 0));
    }

    void refusesLayerSplitWhereTheCompositeStraddlesTheTarget()
    {
        Document document = Document::createDefault(QSize(48, 36));
        document.wobbleAmount = 0.0;
        document.layers.clear();

        Layer base;
        base.name = QStringLiteral("Base");
        base.initialCanvasSize = document.size;
        base.strokes.append(makeStroke(StrokeMode::Paint,
            QColor(40, 180, 100),
            12.0,
            201,
            {QPointF(16.0, 16.0)}));

        Layer target;
        target.name = QStringLiteral("Target");
        target.initialCanvasSize = document.size;
        target.strokes.append(makeStroke(StrokeMode::Paint,
            QColor(220, 200, 60),
            10.0,
            202,
            {QPointF(28.0, 18.0)}));

        Layer clipped;
        clipped.name = QStringLiteral("Clipped");
        clipped.clipToLayerBelow = true;
        clipped.initialCanvasSize = document.size;
        clipped.strokes.append(makeStroke(StrokeMode::Paint,
            QColor(60, 90, 220),
            8.0,
            203,
            {QPointF(30.0, 20.0)}));

        document.layers = {base, target, clipped};
        document.activeLayerId = target.id;
        // The layer above clips to the target, so its pixels depend on the
        // half the split puts on the other side.
        QVERIFY(!RenderEngine::supportsLayerSplit(document, target.id));

        document.layers = {base, target};
        QVERIFY(RenderEngine::supportsLayerSplit(document, target.id));

        Layer group;
        group.name = QStringLiteral("Group above");
        group.kind = LayerKind::Group;
        group.blendMode = LayerBlendMode::Multiply;
        group.initialCanvasSize = document.size;
        Layer child = clipped;
        child.clipToLayerBelow = false;
        child.parentGroupId = group.id;
        document.layers = {base, target, child, group};
        // A group above the target multiplies against everything under it,
        // which is not a source-over stack the split can rebuild.
        QVERIFY(!RenderEngine::supportsLayerSplit(document, target.id));

        group.blendMode = LayerBlendMode::Normal;
        document.layers = {base, target, child, group};
        QVERIFY(RenderEngine::supportsLayerSplit(document, target.id));

        // A target inside a group reaches the frame through the group's own
        // opacity and blend rather than directly.
        Layer nested = target;
        nested.parentGroupId = group.id;
        document.layers = {base, nested, child, group};
        QVERIFY(!RenderEngine::supportsLayerSplit(document, nested.id));
    }

    void composesLayerSplitLikeFullRender()
    {
        Document document = Document::createDefault(QSize(96, 72));
        document.animationFrames = 6;
        document.wobbleAmount = 3.0;
        document.layers.first().strokes.append(makeStroke(StrokeMode::Paint,
            QColor(200, 40, 40),
            8.0,
            11,
            {QPointF(10.0, 10.0), QPointF(80.0, 60.0)}));
        Layer middle;
        middle.name = QStringLiteral("Middle");
        middle.strokes.append(makeStroke(StrokeMode::Paint,
            QColor(40, 200, 40),
            7.0,
            22,
            {QPointF(10.0, 60.0), QPointF(80.0, 10.0)}));
        document.layers.append(middle);
        Layer top;
        top.name = QStringLiteral("Top");
        top.opacity = 0.65;
        top.strokes.append(makeStroke(StrokeMode::Paint,
            QColor(40, 40, 200),
            9.0,
            33,
            {QPointF(48.0, 6.0), QPointF(48.0, 66.0)}));
        document.layers.append(top);
        const QUuid middleId = document.layers[1].id;

        const QVector<Stroke> activeStrokes = {
            makeStroke(StrokeMode::Paint,
                QColor(250, 210, 60),
                5.0,
                44,
                {QPointF(20.0, 20.0), QPointF(70.0, 50.0)}),
            makeStroke(StrokeMode::Erase,
                QColor(Qt::black),
                12.0,
                55,
                {QPointF(30.0, 50.0), QPointF(60.0, 20.0)})};
        for (const Stroke &active : activeStrokes)
        {
            Document full = document;
            full.layer(middleId)->strokes.append(active);
            const QImage expected = RenderEngine::render(full, 2);

            const RenderEngine::LayerSplitFrame split =
                RenderEngine::renderLayerSplit(
                    document, 2, document.size, middleId);
            QVERIFY(split.valid);
            QImage layerImage = split.layerBase;
            QVERIFY(RenderEngine::renderStrokesOnLayer(
                layerImage, document, {active}, 2, document.size));
            const QImage actual =
                RenderEngine::composeLayerSplit(split, layerImage);

            QCOMPARE(actual.size(), expected.size());
            for (int y = 0; y < expected.height(); ++y)
            {
                for (int x = 0; x < expected.width(); ++x)
                {
                    const QColor a = actual.pixelColor(x, y);
                    const QColor b = expected.pixelColor(x, y);
                    QVERIFY(std::abs(a.red() - b.red()) <= 2);
                    QVERIFY(std::abs(a.green() - b.green()) <= 2);
                    QVERIFY(std::abs(a.blue() - b.blue()) <= 2);
                    QVERIFY(std::abs(a.alpha() - b.alpha()) <= 2);
                }
            }
        }
    }

    void composesActiveStrokeRegionsLikeFullPreviews()
    {
        Document document = Document::createDefault(QSize(256, 192));
        document.background = Qt::transparent;
        document.animationFrames = 8;
        document.wobbleAmount = 2.0;
        Layer &target = document.layers.first();
        target.strokes.append(makeStroke(StrokeMode::Paint,
            QColor(210, 55, 75),
            42.0,
            71,
            {QPointF(42.0, 96.0), QPointF(214.0, 96.0)}));
        const QUuid targetId = target.id;

        Layer top;
        top.name = QStringLiteral("Top");
        top.initialCanvasSize = document.size;
        top.opacity = 0.7;
        top.strokes.append(makeStroke(StrokeMode::Paint,
            QColor(45, 95, 220),
            18.0,
            72,
            {QPointF(128.0, 28.0), QPointF(128.0, 164.0)}));
        document.layers.append(top);

        QVector<Stroke> activeStrokes{makeStroke(StrokeMode::Paint,
                                          QColor(235, 195, 45, 180),
                                          14.0,
                                          73,
                                          {QPointF(78.0, 58.0),
                                              QPointF(116.0, 76.0),
                                              QPointF(156.0, 54.0)}),
            makeStroke(StrokeMode::Erase,
                Qt::black,
                24.0,
                74,
                {QPointF(82.0, 112.0), QPointF(172.0, 128.0)})};
        for (Stroke &stroke : activeStrokes)
        {
            stroke.clipMask =
                rectangularMask(document.size, QRect(56, 36, 144, 120));
        }

        for (const QSize outputSize : {document.size, QSize(128, 96)})
        {
            const RenderEngine::LayerSplitFrame split =
                RenderEngine::renderLayerSplit(
                    document, 3, outputSize, targetId);
            QVERIFY(split.valid);
            for (const Stroke &active : activeStrokes)
            {
                QImage fullLayer = split.layerBase;
                QVERIFY(RenderEngine::renderStrokesOnLayer(
                    fullLayer, document, {active}, 3, outputSize));
                const QImage full =
                    RenderEngine::composeLayerSplit(split, fullLayer);

                const QRect bounds = RenderEngine::strokePreviewBounds(
                    document, active, outputSize);
                QVERIFY(!bounds.isEmpty());
                QVERIFY(bounds.size() != outputSize);
                QImage regionalLayer = split.layerBase.copy(bounds);
                RenderEngine::StrokeRenderCache cache;
                QVERIFY(RenderEngine::renderStrokesOnLayerRegion(regionalLayer,
                    document,
                    {active},
                    3,
                    outputSize,
                    bounds,
                    &cache));
                const QImage regional = RenderEngine::composeLayerSplitRegion(
                    split, regionalLayer, bounds);

                QCOMPARE(regional, full.copy(bounds));
                QCOMPARE(cache.clipPaths.size(), 1);
                QImage reusedLayer = split.layerBase.copy(bounds);
                QVERIFY(RenderEngine::renderStrokesOnLayerRegion(reusedLayer,
                    document,
                    {active},
                    3,
                    outputSize,
                    bounds,
                    &cache));
                QCOMPARE(cache.clipPaths.size(), 1);
                QCOMPARE(RenderEngine::composeLayerSplitRegion(
                             split, reusedLayer, bounds),
                    regional);
            }
        }
    }

    void benchmarksFourKActiveStrokeRegions()
    {
        const QSize canvasSize(4096, 4096);
        Document document = Document::createDefault(canvasSize);
        document.background = Qt::transparent;
        document.wobbleAmount = 0.0;

        QVector<QPointF> positions;
        constexpr int pointCount = 3000;
        positions.reserve(pointCount);
        for (int index = 0; index < pointCount; ++index)
        {
            const qreal progress = static_cast<qreal>(index) / (pointCount - 1);
            positions.append(QPointF(256.0 + progress * 3584.0,
                2048.0 + std::sin(progress * 80.0) * 5.0));
        }
        const Stroke active = makeStroke(
            StrokeMode::Paint, QColor(45, 180, 105), 12.0, 82, positions);
        QImage layerBase(canvasSize, QImage::Format_ARGB32_Premultiplied);
        QVERIFY(!layerBase.isNull());
        layerBase.fill(Qt::transparent);
        const QRect bounds =
            RenderEngine::strokePreviewBounds(document, active, canvasSize);
        QVERIFY(!bounds.isEmpty());

        QElapsedTimer timer;
        timer.start();
        QImage fullLayer = layerBase;
        QVERIFY(RenderEngine::renderStrokesOnLayer(
            fullLayer, document, {active}, 0, canvasSize));
        const qint64 fullNanoseconds = timer.nsecsElapsed();
        const quint64 fullBytes = static_cast<quint64>(fullLayer.sizeInBytes());
        const QImage expectedRegion = fullLayer.copy(bounds);
        fullLayer = {};

        timer.restart();
        QImage regionalLayer = layerBase.copy(bounds);
        QVERIFY(RenderEngine::renderStrokesOnLayerRegion(
            regionalLayer, document, {active}, 0, canvasSize, bounds));
        const qint64 regionalNanoseconds = timer.nsecsElapsed();

        QCOMPARE(regionalLayer, expectedRegion);
        const quint64 regionalBytes =
            static_cast<quint64>(regionalLayer.sizeInBytes());
        QVERIFY(regionalBytes * 10 < fullBytes);
        qInfo().nospace() << "4K active-stroke layer replay: full "
                          << static_cast<qreal>(fullNanoseconds) / 1000000.0
                          << " ms / " << fullBytes << " bytes, region "
                          << static_cast<qreal>(regionalNanoseconds) / 1000000.0
                          << " ms / " << regionalBytes << " bytes";
    }

    void blendsSelfCrossingLineStrokesOncePerTile()
    {
        Document document = Document::createDefault(QSize(512, 512));
        document.background = Qt::transparent;
        document.animationFrames = 12;
        document.wobbleAmount = 2.5;
        QImage base(document.size, QImage::Format_ARGB32_Premultiplied);
        base.fill(QColor(60, 90, 140, 255));

        // Ink that does not survive being painted over: a full render strokes
        // the line once, so a crossing has to blend once here too however the
        // line is spread over the tiles.
        for (const bool antialiasing : {false, true})
        {
            for (const int alpha : {255, 185})
            {
                for (const StrokeMode mode :
                    {StrokeMode::Paint, StrokeMode::Erase})
                {
                    Stroke stroke;
                    stroke.mode = mode;
                    stroke.color = QColor(240, 210, 60, alpha);
                    stroke.width = 9.0;
                    stroke.seed = 0x2b71c0deULL;
                    stroke.brush.antialiasing = antialiasing;

                    IncrementalStrokeRenderer renderer;
                    // Sweeps back and forth across a tile boundary and over
                    // its own ink, which is what a shaken pointer does.
                    for (int index = 0; index < 24; ++index)
                    {
                        const qreal phase = static_cast<qreal>(index) * 3.9;
                        stroke.points.append(
                            {QPointF(270.0 + std::sin(phase) * 62.0,
                                 120.0 + static_cast<qreal>(index) * 3.0),
                                1.0});
                        QVERIFY(renderer
                                .update(
                                    base, document, stroke, 7, document.size)
                                .valid);
                        QImage actual = base;
                        QVERIFY(renderer.applyTo(actual));
                        QImage expected = base;
                        QVERIFY(RenderEngine::renderStrokesOnLayer(
                            expected, document, {stroke}, 7, document.size));
                        QCOMPARE(actual, expected);
                    }
                }
            }
        }
    }

    void flattensAnUncutTileForOpaqueAliasedLineStrokes()
    {
        Document document = Document::createDefault(QSize(512, 512));
        document.background = Qt::transparent;
        document.animationFrames = 12;
        document.wobbleAmount = 2.5;
        QImage base(document.size, QImage::Format_ARGB32_Premultiplied);
        base.fill(QColor(60, 90, 140, 255));

        for (const StrokeMode mode : {StrokeMode::Paint, StrokeMode::Erase})
        {
            Stroke stroke;
            stroke.mode = mode;
            stroke.color = QColor(240, 210, 60);
            stroke.width = 7.0;
            stroke.seed = 0x51ce7a11ULL;

            IncrementalStrokeRenderer streaming;
            quint64 peakInstances = 0;
            quint64 lastInstances = 0;
            // Shakes inside a single tile, so the primitives arrive as one
            // unbroken run and there is no gap to cut on. Saturated aliased
            // ink can be painted over instead, which is what lets the prefix
            // be flattened anywhere here.
            for (int index = 0; index < 60; ++index)
            {
                const qreal phase = static_cast<qreal>(index) * 2.7;
                stroke.points.append({QPointF(390.0 + std::sin(phase) * 40.0,
                                          390.0 + std::cos(phase * 0.7) * 30.0),
                    1.0});
                const IncrementalStrokeRenderer::Update update =
                    streaming.update(base, document, stroke, 4, document.size);
                QVERIFY(update.valid);
                peakInstances =
                    std::max(peakInstances, update.primitiveInstancesRendered);
                lastInstances = update.primitiveInstancesRendered;

                IncrementalStrokeRenderer fromScratch;
                QVERIFY(
                    fromScratch.update(base, document, stroke, 4, document.size)
                        .valid);
                QImage flattened = base;
                QVERIFY(streaming.applyTo(flattened));
                QImage replayed = base;
                QVERIFY(fromScratch.applyTo(replayed));
                QCOMPARE(flattened, replayed);
            }
            QVERIFY(lastInstances < peakInstances);
        }
    }

    void incrementallyRendersActiveStrokeTilesLikeFullReplay()
    {
        Document document = Document::createDefault(QSize(640, 480));
        document.background = Qt::transparent;
        document.animationFrames = 12;
        document.wobbleAmount = 2.5;
        QImage base(document.size, QImage::Format_ARGB32_Premultiplied);
        base.fill(QColor(180, 70, 55, 210));
        QPainter basePainter(&base);
        basePainter.setCompositionMode(QPainter::CompositionMode_Source);
        basePainter.fillRect(
            QRect(220, 120, 190, 230), QColor(40, 130, 210, 170));
        basePainter.end();

        QVector<Stroke> strokes;
        Stroke line;
        line.mode = StrokeMode::Paint;
        line.color = QColor(245, 215, 35, 190);
        line.width = 13.0;
        line.seed = 0x12345678ULL;
        line.brush.antialiasing = true;
        line.brush.opacity = 0.72;
        strokes.append(line);

        Stroke pressureLine = line;
        pressureLine.id = QUuid::createUuid();
        pressureLine.seed = 0x23456789ULL;
        pressureLine.brush.opacity = 1.0;
        strokes.append(pressureLine);

        Stroke airbrush = line;
        airbrush.id = QUuid::createUuid();
        airbrush.seed = 0x3456789aULL;
        airbrush.brush.engine = BrushEngine::Airbrush;
        airbrush.brush.spacing = 0.22;
        airbrush.brush.hardness = 0.35;
        airbrush.brush.flow = 0.25;
        strokes.append(airbrush);

        Stroke spray = line;
        spray.id = QUuid::createUuid();
        spray.seed = 0x456789abULL;
        spray.brush.engine = BrushEngine::Spray;
        spray.brush.spacing = 0.35;
        spray.brush.scatter = 1.4;
        spray.brush.particleSize = 0.16;
        spray.brush.sizeJitter = 0.6;
        spray.brush.density = 1.5;
        strokes.append(spray);

        Stroke eraser = airbrush;
        eraser.id = QUuid::createUuid();
        eraser.seed = 0x56789abcULL;
        eraser.mode = StrokeMode::Erase;
        eraser.width = 24.0;
        strokes.append(eraser);

        QVector<QPointF> positions;
        for (int index = 0; index < 180; ++index)
        {
            const qreal progress = static_cast<qreal>(index) / 179.0;
            positions.append(QPointF(18.0 + progress * 604.0,
                240.0 + std::sin(progress * 7.0 * std::numbers::pi) * 150.0));
        }

        for (int strokeIndex = 0; strokeIndex < strokes.size(); ++strokeIndex)
        {
            Stroke stroke = strokes[strokeIndex];
            IncrementalStrokeRenderer renderer;
            for (int index = 0; index < positions.size(); ++index)
            {
                const qreal pressure =
                    strokeIndex == 1 ? 0.2 + 0.8 * ((index % 17) / 16.0) : 0.75;
                stroke.points.append({positions[index], pressure});
                const IncrementalStrokeRenderer::Update update =
                    renderer.update(base, document, stroke, 5, document.size);
                QVERIFY(update.valid);
                QVERIFY(update.sourcePointsProcessed <= 2);

                QImage actual = base;
                QVERIFY(renderer.applyTo(actual));
                QImage expected = base;
                QVERIFY(RenderEngine::renderStrokesOnLayer(
                    expected, document, {stroke}, 5, document.size));
                QCOMPARE(actual, expected);
            }
        }
    }

    void includesSquareLineCapsAndJoinsInIncrementalTileBounds()
    {
        Document document = Document::createDefault(QSize(512, 512));
        document.background = Qt::transparent;
        document.wobbleAmount = 0.0;
        QImage base(document.size, QImage::Format_ARGB32_Premultiplied);
        base.fill(Qt::transparent);

        const auto verify = [&](qreal width, const QVector<QPointF> &points)
        {
            Stroke stroke;
            stroke.color = Qt::black;
            stroke.width = width;
            stroke.brush.tipShape = BrushTipShape::Square;
            stroke.brush.sizeDynamics = 0.0;
            stroke.brush.antialiasing = false;
            for (const QPointF &point : points)
            {
                stroke.points.append({point, 1.0});
            }

            IncrementalStrokeRenderer renderer;
            const IncrementalStrokeRenderer::Update update =
                renderer.update(base, document, stroke, 0, document.size);
            QVERIFY(update.valid);
            QImage actual = base;
            QVERIFY(renderer.applyTo(actual));
            QImage expected = base;
            QVERIFY(RenderEngine::renderStrokesOnLayer(
                expected, document, {stroke}, 0, document.size));
            QCOMPARE(actual, expected);
        };

        verify(200.0, {QPointF(100.0, 100.0), QPointF(140.0, 140.0)});
        verify(80.0,
            {QPointF(190.0, 150.0),
                QPointF(250.0, 250.0),
                QPointF(190.0, 252.0),
                QPointF(250.0, 360.0)});
    }

    void checkpointsLocalizedHundredPixelDabEraserReplay()
    {
        const QSize canvasSize(4096, 4096);
        const QSize outputSize(1024, 1024);
        Document document = Document::createDefault(canvasSize);
        document.background = Qt::transparent;
        document.animationFrames = 30;
        document.wobbleAmount = 1.6;
        QImage base(outputSize, QImage::Format_ARGB32_Premultiplied);
        base.fill(QColor(65, 120, 210, 230));

        Stroke stroke;
        stroke.mode = StrokeMode::Erase;
        stroke.seed = 0x13579bdfULL;
        stroke.width = 100.0;
        stroke.brush.engine = BrushEngine::Airbrush;
        stroke.brush.spacing = 0.08;
        stroke.brush.hardness = 0.25;
        stroke.brush.flow = 0.18;
        stroke.brush.opacity = 0.9;
        stroke.brush.antialiasing = true;

        constexpr int pointCount = 1200;
        IncrementalStrokeRenderer renderer;
        quint64 primitiveInstancesRendered = 0;
        QElapsedTimer timer;
        timer.start();
        for (int index = 0; index < pointCount; ++index)
        {
            const qreal angle = index * 0.075;
            stroke.points.append({QPointF(2048.0 + std::cos(angle) * 120.0,
                                      2048.0 + std::sin(angle) * 120.0),
                1.0});
            const IncrementalStrokeRenderer::Update update =
                renderer.update(base, document, stroke, 4, outputSize);
            QVERIFY(update.valid);
            primitiveInstancesRendered += update.primitiveInstancesRendered;
        }
        const qint64 elapsed = timer.elapsed();

        QImage actual = base;
        QVERIFY(renderer.applyTo(actual));
        QImage expected = base;
        QVERIFY(RenderEngine::renderStrokesOnLayer(
            expected, document, {stroke}, 4, outputSize));
        QCOMPARE(actual, expected);
        QVERIFY(primitiveInstancesRendered
                < static_cast<quint64>(pointCount) * 512ULL);
        QVERIFY(renderer.cachedTileBytes()
                <= static_cast<quint64>(outputSize.width())
                       * outputSize.height() * sizeof(QRgb) * 2ULL);
        qInfo().nospace()
            << "4K-localized 100px dab eraser replay took " << elapsed
            << " ms and rendered " << primitiveInstancesRendered
            << " primitive instances with stable-prefix checkpoints";
    }

    void boundsLongPrefixReplayWorkAtFourK()
    {
        const QSize canvasSize(4096, 4096);
        Document document = Document::createDefault(canvasSize);
        document.background = Qt::transparent;
        document.animationFrames = 12;
        document.wobbleAmount = 3.0;

        for (const int pointCount : {10000, 50000})
        {
            Stroke stroke;
            stroke.seed = 0x6f5e4d3cULL;
            stroke.width = 9.0;
            stroke.brush.antialiasing = false;
            stroke.points.reserve(pointCount);
            StrokeRenderer::IncrementalGeometry geometry;
            quint64 processedPoints = 0;
            quint64 rebuilds = 0;
            QElapsedTimer timer;
            timer.start();
            for (int index = 0; index < pointCount; ++index)
            {
                const int row = index / 256;
                const int column = index % 256;
                const qreal x =
                    row % 2 == 0 ? column * 16.0 : 4080.0 - column * 16.0;
                const qreal y = 8.0 + row * 20.0;
                stroke.points.append({QPointF(x, std::min(y, 4088.0)),
                    0.35 + (index % 13) * 0.05});
                const StrokeRenderer::GeometryUpdate update = geometry.update(
                    stroke, 4, document.animationFrames, document.wobbleAmount);
                QVERIFY(update.valid);
                processedPoints += update.sourcePointsProcessed;
                rebuilds += update.rebuilt ? 1 : 0;
            }
            const qint64 elapsed = timer.elapsed();
            const StrokeRenderer::PreparedStroke expected =
                StrokeRenderer::prepare(
                    stroke, 4, document.animationFrames, document.wobbleAmount);
            QCOMPARE(geometry.prepared().points, expected.points);
            QCOMPARE(geometry.prepared().width, expected.width);
            QCOMPARE(processedPoints, static_cast<quint64>(pointCount));
            QCOMPARE(rebuilds, 1ULL);
            qInfo().nospace()
                << "4K " << pointCount
                << "-point incremental geometry replay took " << elapsed
                << " ms and processed " << processedPoints << " source points";
        }

        QImage base(canvasSize, QImage::Format_ARGB32_Premultiplied);
        QVERIFY(!base.isNull());
        base.fill(Qt::transparent);
        const auto benchmarkRasterPrefix = [&](int pointCount)
        {
            Stroke stroke;
            stroke.seed = 0x7a6b5c4dULL;
            stroke.width = 9.0;
            stroke.brush.antialiasing = false;
            stroke.points.reserve(pointCount);
            IncrementalStrokeRenderer renderer;
            quint64 processedPoints = 0;
            quint64 renderedPixels = 0;
            quint64 cachedTileBytes = 0;
            QVector<qint64> updateNanoseconds;
            updateNanoseconds.reserve(pointCount);
            QElapsedTimer timer;
            timer.start();
            for (int index = 0; index < pointCount; ++index)
            {
                const int row = index / 256;
                const int column = index % 256;
                const qreal x =
                    row % 2 == 0 ? column * 16.0 : 4080.0 - column * 16.0;
                stroke.points.append(
                    {QPointF(x, 8.0 + row * 20.0), 0.4 + (index % 11) * 0.05});
                QElapsedTimer updateTimer;
                updateTimer.start();
                const IncrementalStrokeRenderer::Update update =
                    renderer.update(base, document, stroke, 4, canvasSize);
                updateNanoseconds.append(updateTimer.nsecsElapsed());
                QVERIFY(update.valid);
                processedPoints += update.sourcePointsProcessed;
                renderedPixels += update.pixelsRendered;
                cachedTileBytes = update.cachedTileBytes;
            }
            const qint64 elapsed = timer.elapsed();
            QImage actual = base;
            QVERIFY(renderer.applyTo(actual));
            QImage expected = base;
            QVERIFY(RenderEngine::renderStrokesOnLayer(
                expected, document, {stroke}, 4, canvasSize));
            QCOMPARE(actual, expected);
            QCOMPARE(processedPoints, static_cast<quint64>(pointCount));
            QVERIFY(renderedPixels <= static_cast<quint64>(pointCount) * 4ULL
                                          * 256ULL * 256ULL);
            QVERIFY(cachedTileBytes <= static_cast<quint64>(canvasSize.width())
                                           * canvasSize.height()
                                           * sizeof(QRgb));
            std::sort(updateNanoseconds.begin(), updateNanoseconds.end());
            const qreal p50Milliseconds =
                static_cast<qreal>(
                    updateNanoseconds[updateNanoseconds.size() * 50 / 100])
                / 1000000.0;
            const qreal p95Milliseconds =
                static_cast<qreal>(
                    updateNanoseconds[updateNanoseconds.size() * 95 / 100])
                / 1000000.0;
            qInfo().nospace()
                << "4K " << pointCount << "-point incremental tile replay took "
                << elapsed << " ms, processed " << renderedPixels
                << " pixels, and retained " << cachedTileBytes << " bytes; p50 "
                << p50Milliseconds << " ms, p95 " << p95Milliseconds << " ms";
        };

#ifdef NDEBUG
        constexpr std::array rasterPointCounts{10000, 50000};
#else
        constexpr std::array rasterPointCounts{1000, 5000};
#endif
        for (const int pointCount : rasterPointCounts)
        {
            benchmarkRasterPrefix(pointCount);
        }
    }
};

int runLayerSplitPreviewTests(int argc, char **argv)
{
    LayerSplitPreviewTests tests;
    return QTest::qExec(&tests, argc, argv);
}

}

#include "LayerSplitPreviewTests.moc"

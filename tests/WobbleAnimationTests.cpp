// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "support/RenderTestHelpers.hpp"
#include "support/RenderTestSuites.hpp"

#include "app/MemoryBudget.hpp"
#include "render/engine/StaticLayerCache.hpp"

#include <QtConcurrentMap>

#include <numeric>

namespace ugurugu
{

namespace
{

// Every brush engine plus a fill and an erase on one still layer, next to a
// wobbling layer, so the static cache sees each kind of operation.
Document mixedStillDocument()
{
    Document document = animatedDocument();
    Layer still;
    still.name = QStringLiteral("Still");
    still.wobbleAmount = 0.0;
    still.initialCanvasSize = document.size;
    Stroke line = makeStroke(StrokeMode::Paint,
        QColor(200, 40, 60),
        7.0,
        0x11ULL,
        {QPointF(6.0, 8.0), QPointF(40.0, 30.0), QPointF(90.0, 12.0)});
    line.points[1].pressure = 0.3;
    Stroke airbrush = makeStroke(StrokeMode::Paint,
        QColor(40, 160, 90, 200),
        14.0,
        0x22ULL,
        {QPointF(10.0, 60.0), QPointF(50.0, 40.0), QPointF(86.0, 66.0)});
    airbrush.brush.engine = BrushEngine::Airbrush;
    Stroke spray = makeStroke(StrokeMode::Paint,
        QColor(30, 60, 210),
        18.0,
        0x33ULL,
        {QPointF(20.0, 20.0), QPointF(70.0, 50.0)});
    spray.brush.engine = BrushEngine::Spray;
    spray.brush.sizeJitter = 0.6;
    Stroke erase = makeStroke(StrokeMode::Erase,
        Qt::black,
        6.0,
        0x44ULL,
        {QPointF(0.0, 36.0), QPointF(96.0, 36.0)});
    still.strokes = {line, airbrush, spray, erase};
    document.layers.prepend(still);
    return document;
}

class StaticLayerCacheBudget final
{
public:
    explicit StaticLayerCacheBudget(qint64 bytes)
    {
        RenderEngine::clearStaticLayerCache();
        RenderEngine::setStaticLayerCacheBudget(bytes);
    }
    ~StaticLayerCacheBudget()
    {
        RenderEngine::setStaticLayerCacheBudget(
            MemoryBudget::staticLayerCacheBytes);
        RenderEngine::clearStaticLayerCache();
    }
    StaticLayerCacheBudget(const StaticLayerCacheBudget &) = delete;
    StaticLayerCacheBudget &operator=(const StaticLayerCacheBudget &) = delete;
};

}

class WobbleAnimationTests final : public QObject
{
    Q_OBJECT

private slots:
    void classifiesFrameInvariantLayers()
    {
        const StaticLayerCacheBudget uncached(0);
        Document document = mixedStillDocument();
        const Layer &still = document.layers.first();
        const Document stillDocument = documentForLayer(document, still);
        QVERIFY(render_detail::isLayerFrameInvariant(stillDocument, still));

        // The predicate is only as good as the renderer agreeing with it:
        // an invariant layer must rasterize identically on every frame.
        Document alone = document;
        alone.layers = {still};
        alone.activeLayerId = still.id;
        const QImage first = RenderEngine::render(alone, 0);
        QVERIFY(!first.isNull());
        for (int frame = 1; frame < document.animationFrames; ++frame)
        {
            QCOMPARE(RenderEngine::render(alone, frame), first);
        }

        const Layer &moving = document.layers.last();
        QVERIFY(!render_detail::isLayerFrameInvariant(
            documentForLayer(document, moving), moving));

        Layer jittered = still;
        jittered.strokes[2].brush.animatedJitter = true;
        QVERIFY(!render_detail::isLayerFrameInvariant(
            documentForLayer(document, jittered), jittered));

        Layer broken = still;
        MotionSettings motion;
        motion.brokenLine = true;
        broken.motion = motion;
        QVERIFY(!render_detail::isLayerFrameInvariant(
            documentForLayer(document, broken), broken));

        Layer unscaled = moving;
        for (Stroke &stroke : unscaled.strokes)
        {
            stroke.brush.wobbleScale = 0.0;
        }
        QVERIFY(render_detail::isLayerFrameInvariant(
            documentForLayer(document, unscaled), unscaled));
    }

    void staticLayerCacheMatchesUncachedFrames()
    {
        Document document = mixedStillDocument();
        const QSize preview(48, 36);
        const auto renderAll = [&document, &preview]()
        {
            QVector<QImage> frames;
            for (int frame = 0; frame < document.animationFrames; ++frame)
            {
                frames.append(RenderEngine::renderScaled(document,
                    frame,
                    preview,
                    RenderEngine::ScaledRenderMode::DisplayPreview));
                frames.append(RenderEngine::renderScaled(document,
                    frame,
                    document.size,
                    RenderEngine::ScaledRenderMode::NativeExact));
            }
            return frames;
        };

        QVector<QImage> expected;
        {
            const StaticLayerCacheBudget uncached(0);
            expected = renderAll();
        }
        const StaticLayerCacheBudget cached(
            MemoryBudget::staticLayerCacheBytes);
        QCOMPARE(renderAll(), expected);
        QVERIFY(render_detail::StaticLayerCache::residentBytes() > 0);

        // Editing the still layer must never be answered from the raster
        // cached for its previous strokes.
        document.layers.first().strokes.append(makeStroke(StrokeMode::Paint,
            QColor(250, 200, 30),
            12.0,
            0x55ULL,
            {QPointF(4.0, 4.0), QPointF(92.0, 68.0)}));
        const QVector<QImage> edited = renderAll();
        QVERIFY(edited != expected);
        {
            const StaticLayerCacheBudget uncached(0);
            QCOMPARE(edited, renderAll());
        }
    }

    void staticLayerCacheIsSharedAcrossConcurrentFrames()
    {
        const Document document = mixedStillDocument();
        const QSize preview(48, 36);
        QVector<QImage> expected;
        {
            const StaticLayerCacheBudget uncached(0);
            for (int frame = 0; frame < document.animationFrames; ++frame)
            {
                expected.append(
                    RenderEngine::renderScaled(document, frame, preview));
            }
        }
        const StaticLayerCacheBudget cached(
            MemoryBudget::staticLayerCacheBytes);
        QVector<int> frames(document.animationFrames);
        std::iota(frames.begin(), frames.end(), 0);
        const QList<QImage> rendered = QtConcurrent::blockingMapped(frames,
            [&document, &preview](int frame)
            {
                return RenderEngine::renderScaled(document, frame, preview);
            });
        QCOMPARE(QVector<QImage>(rendered.cbegin(), rendered.cend()), expected);
    }

    void loopsAtFrameCount()
    {
        const Document document = animatedDocument();
        const QImage first = RenderEngine::render(document, 0);
        const QImage looped =
            RenderEngine::render(document, document.animationFrames);

        QVERIFY(first == looped);
    }

    void changesAcrossWobbleFrames()
    {
        const Document document = animatedDocument();
        const QImage first = RenderEngine::render(document, 0);
        const QImage second = RenderEngine::render(document, 1);

        QVERIFY(first != second);
    }

    void staysStillWhenWobbleIsZero()
    {
        Document document = animatedDocument();
        document.wobbleAmount = 0.0;
        document.layers.first().strokes.first().width = 40.0;

        const QImage first = RenderEngine::render(document, 0);
        QVERIFY(!first.isNull());
        for (int frame = 1; frame < document.animationFrames; ++frame)
        {
            QCOMPARE(RenderEngine::render(document, frame), first);
        }
    }

    void staysStillWhenBrushWobbleScaleIsZero()
    {
        Document document = animatedDocument();
        document.layers.first().strokes.first().brush.wobbleScale = 0.0;

        const QImage first = RenderEngine::render(document, 0);
        QVERIFY(!first.isNull());
        for (int frame = 1; frame < document.animationFrames; ++frame)
        {
            QCOMPARE(RenderEngine::render(document, frame), first);
        }
    }

    void scalesWobbleByBrushWobbleScale()
    {
        Document document = animatedDocument();
        const QImage normal = RenderEngine::render(document, 0);
        document.layers.first().strokes.first().brush.wobbleScale = 2.0;
        const QImage rough = RenderEngine::render(document, 0);

        QVERIFY(!normal.isNull());
        QVERIFY(normal != rough);
    }

    void rendersSmoothMotionDeterministicallyAcrossLoop()
    {
        Document document = animatedDocument();
        document.motion.style = MotionStyle::Smooth;
        document.motion.poseCount = 5;
        document.motion.detail = 18;
        document.motion.linked = 0.35;
        document.motion.randomness = 0.6;

        const QImage first = RenderEngine::render(document, 0);
        QCOMPARE(RenderEngine::render(document, 0), first);
        QCOMPARE(
            RenderEngine::render(document, document.animationFrames), first);
        QVERIFY(RenderEngine::render(document, 1) != first);
    }

    void holdsSteppedMotionForBalancedFrameRanges()
    {
        Document document = animatedDocument();
        document.motion.style = MotionStyle::Stepped;
        document.motion.poseCount = 3;

        const QImage firstPose = RenderEngine::render(document, 0);
        for (int frame = 1; frame < 4; ++frame)
        {
            QCOMPARE(RenderEngine::render(document, frame), firstPose);
        }
        QVERIFY(RenderEngine::render(document, 4) != firstPose);
        QCOMPARE(RenderEngine::render(document, document.animationFrames),
            firstPose);
    }

    void canHideEverySegmentWithBrokenLineAmount()
    {
        Document document = animatedDocument();
        document.background = Qt::transparent;
        document.motion.brokenLine = true;
        document.motion.breakAmount = 1.0;
        document.motion.breakRange = 8.0;

        const QImage hidden = RenderEngine::render(document, 0);
        QVERIFY(!hidden.isNull());
        for (int y = 0; y < hidden.height(); ++y)
        {
            const auto *line =
                reinterpret_cast<const QRgb *>(hidden.constScanLine(y));
            for (int x = 0; x < hidden.width(); ++x)
            {
                QCOMPARE(qAlpha(line[x]), 0);
            }
        }
    }
};

int runWobbleAnimationTests(int argc, char **argv)
{
    WobbleAnimationTests tests;
    return QTest::qExec(&tests, argc, argv);
}

}

#include "WobbleAnimationTests.moc"

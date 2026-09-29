// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "support/RenderTestHelpers.hpp"
#include "support/RenderTestSuites.hpp"

namespace ugurugu
{

class WobbleAnimationTests final : public QObject
{
    Q_OBJECT

private slots:
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

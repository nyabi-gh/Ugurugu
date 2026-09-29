// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "render/ClassicStrokeMotion.hpp"
#include "support/RenderTestSuites.hpp"

#include <QtTest>

#include <array>
#include <cmath>

namespace ugurugu
{

class ClassicStrokeMotionTests final : public QObject
{
    Q_OBJECT

private slots:
    void clampsDisplacementAmplitude()
    {
        QCOMPARE(ClassicStrokeMotion::displacementAmplitude(20.0, -2.0), 0.0);
        QCOMPARE(ClassicStrokeMotion::displacementAmplitude(20.0, 0.0), 0.0);
        QCOMPARE(ClassicStrokeMotion::displacementAmplitude(40.0, 2.0),
            ClassicStrokeMotion::displacementAmplitude(80.0, 2.0));
    }

    void keepsZeroAmplitudeSamplesStill()
    {
        const StrokePoint sample{QPointF(23.5, 47.25), 0.4};
        const StrokePoint displaced = ClassicStrokeMotion::displacedSample(
            sample, QPointF(0.6, 0.8), 37.0, 0.0, 0xfeedbeefULL, 5);

        QCOMPARE(displaced, sample);
    }

    void reportsConservativeDisplacementBound()
    {
        constexpr std::array<qreal, 3> pressures = {0.0, 0.5, 1.0};
        constexpr std::array<qreal, 4> arcLengths = {0.0, 8.5, 31.0, 117.25};
        constexpr std::array<int, 3> frames = {0, 5, 11};
        const qreal strokeWidth = 18.0;
        const qreal wobbleAmount = 3.5;
        const qreal amplitude = ClassicStrokeMotion::displacementAmplitude(
            strokeWidth, wobbleAmount);
        const qreal maximum =
            ClassicStrokeMotion::maximumDisplacement(strokeWidth, wobbleAmount);

        for (const qreal pressure : pressures)
        {
            const StrokePoint sample{QPointF(10.0, 20.0), pressure};
            for (const qreal arcLength : arcLengths)
            {
                for (const int frame : frames)
                {
                    const StrokePoint displaced =
                        ClassicStrokeMotion::displacedSample(sample,
                            QPointF(0.6, 0.8),
                            arcLength,
                            amplitude,
                            0x12345678ULL,
                            frame);
                    const QPointF offset = displaced.position - sample.position;
                    QVERIFY(std::hypot(offset.x(), offset.y()) <= maximum);
                }
            }
        }
    }

    void keepsWidthStableWithoutWobble()
    {
        for (int frame = 0; frame < 12; ++frame)
        {
            QCOMPARE(ClassicStrokeMotion::renderedWidth(
                         7.5, 0xfeedbeefULL, frame, 0.0),
                7.5);
        }
    }
};

int runClassicStrokeMotionTests(int argc, char **argv)
{
    ClassicStrokeMotionTests tests;
    return QTest::qExec(&tests, argc, argv);
}

}

#include "ClassicStrokeMotionTests.moc"

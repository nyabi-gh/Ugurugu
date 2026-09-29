// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "document/DocumentLimits.hpp"
#include "render/BrokenLineModel.hpp"
#include "render/DeterministicNoise.hpp"
#include "support/RenderTestSuites.hpp"

#include <QtTest>

#include <algorithm>
#include <limits>

namespace ugurugu
{

class BrokenLineModelTests final : public QObject
{
    Q_OBJECT

private slots:
    void preservesContinuousLineAtZeroAmount()
    {
        const QVector<StrokePoint> points = linePoints(12);
        const QVector<quint8> segments =
            BrokenLineModel::visibleSegments(points, 17, 3, 0.0, 8.0);
        QCOMPARE(segments.size(), points.size() - 1);
        QVERIFY(std::all_of(segments.cbegin(),
            segments.cend(),
            [](quint8 visible)
            {
                return visible != 0;
            }));
        const QVector<BrokenLineModel::VisibleRun> expected{
            {0, points.size() - 1}};
        QCOMPARE(BrokenLineModel::visibleRuns(segments), expected);
    }

    void hidesEverySegmentAtMaximumAmount()
    {
        const QVector<StrokePoint> points = linePoints(12);
        const QVector<quint8> segments =
            BrokenLineModel::visibleSegments(points, 17, 3, 1.0, 8.0);
        QVERIFY(std::none_of(segments.cbegin(),
            segments.cend(),
            [](quint8 visible)
            {
                return visible != 0;
            }));
        QVERIFY(BrokenLineModel::visibleRuns(segments).isEmpty());
    }

    void isDeterministicAndPoseDependent()
    {
        const QVector<StrokePoint> points = linePoints(240);
        const QVector<quint8> first =
            BrokenLineModel::visibleSegments(points, 9182, 5, 0.45, 11.0);
        QCOMPARE(BrokenLineModel::visibleSegments(points, 9182, 5, 0.45, 11.0),
            first);
        QVERIFY(BrokenLineModel::visibleSegments(points, 9182, 6, 0.45, 11.0)
                != first);
    }

    void keepsExistingSegmentsStableWhenPointsAppend()
    {
        const QVector<StrokePoint> points = linePoints(180);
        const QVector<StrokePoint> prefix = points.sliced(0, 120);
        const QVector<quint8> before =
            BrokenLineModel::visibleSegments(prefix, 8821, 7, 0.4, 13.0);
        const QVector<quint8> after =
            BrokenLineModel::visibleSegments(points, 8821, 7, 0.4, 13.0);
        QCOMPARE(after.sliced(0, before.size()), before);
    }

    void buildsPointRangesFromVisibleSegments()
    {
        const QVector<quint8> segments{0, 1, 1, 0, 1, 0};
        const QVector<BrokenLineModel::VisibleRun> expected{{1, 3}, {4, 5}};
        QCOMPARE(BrokenLineModel::visibleRuns(segments), expected);
    }

    void handlesValidatedArcLengthsBeyondIntRange()
    {
        QVector<StrokePoint> points;
        points.reserve(50000);
        for (int index = 0; index < 50000; ++index)
        {
            const qreal coordinate =
                index % 2 == 0
                    ? -DocumentLimits::maximumStoredCoordinateMagnitude
                    : DocumentLimits::maximumStoredCoordinateMagnitude;
            points.append({QPointF(coordinate, coordinate), 1.0});
        }

        const QVector<quint8> segments =
            BrokenLineModel::visibleSegments(points, 7, 0, 0.45, 2.0);
        QCOMPARE(segments.size(), points.size() - 1);
        QCOMPARE(BrokenLineModel::visibleSegments(points, 7, 0, 0.45, 2.0),
            segments);
        const auto tail = segments.sliced(48000);
        QVERIFY(std::any_of(tail.cbegin(),
            tail.cend(),
            [](quint8 value)
            {
                return value == 0;
            }));
        QVERIFY(std::any_of(tail.cbegin(),
            tail.cend(),
            [](quint8 value)
            {
                return value != 0;
            }));

        const qreal coordinate =
            static_cast<qreal>(std::numeric_limits<int>::max()) + 1.5;
        const qreal first =
            DeterministicNoise::smoothValue(7, 0, coordinate, 0x1234ULL);
        QVERIFY(std::isfinite(first));
        QCOMPARE(DeterministicNoise::smoothValue(7, 0, coordinate, 0x1234ULL),
            first);
    }

    void rejectsInvalidSettings()
    {
        const QVector<StrokePoint> points = linePoints(3);
        QVERIFY(BrokenLineModel::visibleSegments(points, 1, 0, -0.1, 10.0)
                .isEmpty());
        QVERIFY(
            BrokenLineModel::visibleSegments(points, 1, 0, 0.2, 0.0).isEmpty());
    }

private:
    static QVector<StrokePoint> linePoints(qsizetype count)
    {
        QVector<StrokePoint> points;
        points.reserve(count);
        for (qsizetype index = 0; index < count; ++index)
        {
            points.append({QPointF(static_cast<qreal>(index) * 2.0, 0.0), 1.0});
        }
        return points;
    }
};

int runBrokenLineModelTests(int argc, char **argv)
{
    BrokenLineModelTests tests;
    return QTest::qExec(&tests, argc, argv);
}

}

#include "BrokenLineModelTests.moc"

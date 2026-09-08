// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "render/MotionTimeModel.hpp"
#include "support/RenderTestSuites.hpp"

#include <QPointF>
#include <QtTest>

#include <algorithm>
#include <array>
#include <cmath>
#include <limits>

namespace ugurugu
{

namespace
{

constexpr std::array<qreal, 5> poseValues = {0.7, -0.4, 0.2, -0.9, 0.5};

qreal sampledValue(const MotionTimeModel::SmoothPoseSample &sample)
{
    const qreal first = poseValues[static_cast<std::size_t>(sample.firstPose)];
    const qreal second =
        poseValues[static_cast<std::size_t>(sample.secondPose)];
    return first + (second - first) * sample.blend;
}

}

class MotionTimeModelTests final : public QObject
{
    Q_OBJECT

private slots:
    void smoothSamplesLoopForPositiveAndNegativeTime()
    {
        constexpr int frameCount = 30;
        constexpr int poseCount = 5;
        for (const qreal position : {-61.25, -0.5, 0.0, 7.25, 29.75, 60.5})
        {
            const auto sample =
                MotionTimeModel::smoothSample(position, frameCount, poseCount);
            const auto looped = MotionTimeModel::smoothSample(
                position + frameCount, frameCount, poseCount);
            QVERIFY(sample.has_value());
            QVERIFY(looped.has_value());
            QCOMPARE(sample->firstPose, looped->firstPose);
            QCOMPARE(sample->secondPose, looped->secondPose);
            QVERIFY(std::abs(sample->blend - looped->blend) < 1e-12);
        }
    }

    void smoothSamplesMeetContinuouslyAtEveryPose()
    {
        constexpr int frameCount = 30;
        constexpr int poseCount = 5;
        constexpr qreal epsilon = 1e-6;
        for (int pose = 0; pose < poseCount; ++pose)
        {
            const qreal boundary =
                static_cast<qreal>(pose * frameCount) / poseCount;
            const auto before = MotionTimeModel::smoothSample(
                boundary - epsilon, frameCount, poseCount);
            const auto at =
                MotionTimeModel::smoothSample(boundary, frameCount, poseCount);
            const auto after = MotionTimeModel::smoothSample(
                boundary + epsilon, frameCount, poseCount);
            QVERIFY(before.has_value());
            QVERIFY(at.has_value());
            QVERIFY(after.has_value());
            QCOMPARE(at->firstPose, pose);
            QCOMPARE(at->blend, 0.0);
            QVERIFY(std::abs(sampledValue(*before) - sampledValue(*at)) < 1e-9);
            QVERIFY(std::abs(sampledValue(*after) - sampledValue(*at)) < 1e-9);
        }
    }

    void smoothBlendDoesNotExpandCoverage()
    {
        constexpr qreal maximumDisplacement = 4.0;
        const QPointF first(maximumDisplacement, 0.0);
        const QPointF second(0.0, -maximumDisplacement);
        for (int frame = 0; frame < 30; ++frame)
        {
            const auto sample = MotionTimeModel::smoothSample(frame, 30, 7);
            QVERIFY(sample.has_value());
            QVERIFY(sample->blend >= 0.0 && sample->blend <= 1.0);
            const QPointF blended = first + (second - first) * sample->blend;
            QVERIFY(
                std::hypot(blended.x(), blended.y()) <= maximumDisplacement);
        }
    }

    void steppedPosesBalanceTheWholeLoop()
    {
        constexpr int frameCount = 30;
        constexpr int poseCount = 8;
        std::array<int, poseCount> framesPerPose{};
        for (int frame = 0; frame < frameCount; ++frame)
        {
            const auto pose =
                MotionTimeModel::steppedPose(frame, frameCount, poseCount);
            QVERIFY(pose.has_value());
            ++framesPerPose[static_cast<std::size_t>(*pose)];
        }
        const auto [minimum, maximum] =
            std::minmax_element(framesPerPose.cbegin(), framesPerPose.cend());
        QVERIFY(*maximum - *minimum <= 1);
        QCOMPARE(
            MotionTimeModel::steppedPose(frameCount, frameCount, poseCount),
            std::optional<int>(0));
        QCOMPARE(MotionTimeModel::steppedPose(-1, frameCount, poseCount),
            std::optional<int>(poseCount - 1));
    }

    void rejectsInvalidSamplingContracts()
    {
        QVERIFY(!MotionTimeModel::smoothSample(0.0, 0, 1).has_value());
        QVERIFY(!MotionTimeModel::smoothSample(0.0, 4, 0).has_value());
        QVERIFY(!MotionTimeModel::smoothSample(0.0, 4, 5).has_value());
        QVERIFY(!MotionTimeModel::smoothSample(
            std::numeric_limits<qreal>::infinity(), 4, 2)
                .has_value());
        QVERIFY(!MotionTimeModel::steppedPose(0, 0, 1).has_value());
        QVERIFY(!MotionTimeModel::steppedPose(0, 4, 5).has_value());
    }
};

int runMotionTimeModelTests(int argc, char **argv)
{
    MotionTimeModelTests tests;
    return QTest::qExec(&tests, argc, argv);
}

}

#include "MotionTimeModelTests.moc"

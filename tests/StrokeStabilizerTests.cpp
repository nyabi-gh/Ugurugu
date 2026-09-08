// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "input/StrokeStabilizer.hpp"

#include <QtTest>

#include <cmath>
#include <limits>

namespace ugurugu
{

class StrokeStabilizerTests final : public QObject
{
    Q_OBJECT

private slots:
    void passesSamplesThroughWhenDisabled()
    {
        StrokeStabilizer stabilizer;
        QCOMPARE(stabilizer.begin(QPointF(4.0, 7.0), 10), QPointF(4.0, 7.0));
        QCOMPARE(
            stabilizer.update(QPointF(18.0, 23.0), 18), QPointF(18.0, 23.0));
        QCOMPARE(
            stabilizer.update(QPointF(31.0, 29.0), 26), QPointF(31.0, 29.0));
    }

    void reducesAlternatingJitter()
    {
        StrokeStabilizer stabilizer;
        stabilizer.setStrength(1.0);
        stabilizer.begin({}, 0);

        qreal rawDeviation = 0.0;
        qreal filteredDeviation = 0.0;
        for (int sample = 1; sample <= 160; ++sample)
        {
            const qreal y = sample % 2 == 0 ? 3.0 : -3.0;
            const QPointF filtered = stabilizer.update(
                QPointF(sample, y), static_cast<quint64>(sample) * 8);
            if (sample > 20)
            {
                rawDeviation += std::abs(y);
                filteredDeviation += std::abs(filtered.y());
            }
        }

        QVERIFY(filteredDeviation < rawDeviation * 0.25);
    }

    void adaptsToFastMovement()
    {
        StrokeStabilizer slow;
        StrokeStabilizer fast;
        slow.setStrength(1.0);
        fast.setStrength(1.0);
        slow.begin({}, 0);
        fast.begin({}, 0);

        const qreal slowRatio = slow.update(QPointF(1.0, 0.0), 8).x();
        const qreal fastRatio = fast.update(QPointF(100.0, 0.0), 8).x() / 100.0;

        QVERIFY(fastRatio > slowRatio * 2.0);
    }

    void resetsAtTheFirstPointOfEachStroke()
    {
        StrokeStabilizer stabilizer;
        stabilizer.setStrength(1.0);
        stabilizer.begin({}, 0);
        stabilizer.update(QPointF(100.0, 100.0), 8);
        stabilizer.reset();

        QCOMPARE(stabilizer.begin(QPointF(9.0, 12.0), 100), QPointF(9.0, 12.0));
    }

    void finishesAtTheRawPointerPosition()
    {
        StrokeStabilizer stabilizer;
        stabilizer.setStrength(1.0);
        stabilizer.begin({}, 0);
        const QPointF filtered = stabilizer.update(QPointF(100.0, 0.0), 8);
        QVERIFY(filtered.x() < 100.0);
        QCOMPARE(
            stabilizer.finish(QPointF(100.0, 0.0), 8), QPointF(100.0, 0.0));
    }

    void clampsStrength()
    {
        StrokeStabilizer stabilizer;
        stabilizer.setStrength(4.0);
        QCOMPARE(stabilizer.strength(), 1.0);
        stabilizer.setStrength(-2.0);
        QCOMPARE(stabilizer.strength(), 0.0);
        stabilizer.setStrength(std::numeric_limits<qreal>::quiet_NaN());
        QCOMPARE(stabilizer.strength(), 0.0);
    }
};

int runStrokeStabilizerTests(int argc, char **argv)
{
    StrokeStabilizerTests tests;
    return QTest::qExec(&tests, argc, argv);
}

}

#include "StrokeStabilizerTests.moc"

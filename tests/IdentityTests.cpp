// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "document/DocumentController.hpp"
#include "document/Identity.hpp"
#include "support/DocumentTestSuites.hpp"

#include <QtTest>

namespace ugurugu
{

namespace
{

struct Draw
{
    QVector<QUuid> ids;
    QVector<quint64> seeds;

    bool operator==(const Draw &) const = default;
};

Draw draw(int count)
{
    Draw result;
    for (int index = 0; index < count; ++index)
    {
        result.ids.append(Identity::newId());
        result.seeds.append(Identity::newSeed());
    }
    return result;
}

QVector<QUuid> idsAfterDuplicatingAPaintedLayer()
{
    DocumentController controller;
    const QUuid layerId = controller.document().activeLayerId;
    Stroke stroke;
    stroke.seed = Identity::newSeed();
    stroke.points = {{QPointF(10, 10), 1.0}, {QPointF(40, 30), 1.0}};
    const auto added = controller.addStroke(layerId, stroke);
    if (added != DocumentController::AddStrokeResult::Added)
    {
        return {};
    }
    controller.duplicateLayer(layerId);
    QVector<QUuid> ids;
    for (const Layer &layer : controller.document().layers)
    {
        ids.append(layer.id);
        for (const Stroke &layerStroke : layer.strokes)
        {
            ids.append(layerStroke.id);
        }
    }
    return ids;
}

}

class IdentityTests final : public QObject
{
    Q_OBJECT

private slots:
    void sameSeedRepeatsTheSequence()
    {
        Draw first;
        {
            const Identity::DeterministicScope scope(42);
            first = draw(4);
        }
        Draw second;
        {
            const Identity::DeterministicScope scope(42);
            second = draw(4);
        }
        Draw other;
        {
            const Identity::DeterministicScope scope(43);
            other = draw(4);
        }
        QCOMPARE(first, second);
        QVERIFY(first.ids != other.ids);
        QVERIFY(first.seeds != other.seeds);
    }

    void pinsTheFirstValuesOfTheSequence()
    {
        const Identity::DeterministicScope scope(0);
        QCOMPARE(Identity::newSeed(), 0xe220a8397b1dcdafULL);
        QCOMPARE(Identity::newId(),
            QUuid(QStringLiteral("{63cfc62a-2b09-4592-9c07-46b419466aec}")));
    }

    void deterministicIdsAreVersionFourUuids()
    {
        const Identity::DeterministicScope scope(7);
        for (int index = 0; index < 64; ++index)
        {
            const QUuid id = Identity::newId();
            QVERIFY(!id.isNull());
            QCOMPARE(id.version(), QUuid::Random);
            QCOMPARE(id.variant(), QUuid::DCE);
        }
    }

    void idsDoNotShiftTheSeedSequence()
    {
        QVector<quint64> plain;
        {
            const Identity::DeterministicScope scope(9);
            for (int index = 0; index < 3; ++index)
            {
                plain.append(Identity::newSeed());
            }
        }
        QVector<quint64> interleaved;
        {
            const Identity::DeterministicScope scope(9);
            for (int index = 0; index < 3; ++index)
            {
                Identity::newId();
                interleaved.append(Identity::newSeed());
            }
        }
        QCOMPARE(interleaved, plain);
    }

    void nestedScopesRestoreTheOuterSequence()
    {
        QVector<quint64> straight;
        {
            const Identity::DeterministicScope scope(5);
            straight = {Identity::newSeed(), Identity::newSeed()};
        }
        QVector<quint64> nested;
        {
            const Identity::DeterministicScope outer(5);
            nested.append(Identity::newSeed());
            {
                const Identity::DeterministicScope inner(6);
                Identity::newSeed();
                Identity::newId();
            }
            nested.append(Identity::newSeed());
        }
        QCOMPARE(nested, straight);
        QVERIFY(Identity::newId() != Identity::newId());
    }

    void documentEditsDrawTheirIdsFromTheScope()
    {
        QVector<QUuid> first;
        {
            const Identity::DeterministicScope scope(11);
            first = idsAfterDuplicatingAPaintedLayer();
        }
        QVector<QUuid> second;
        {
            const Identity::DeterministicScope scope(11);
            second = idsAfterDuplicatingAPaintedLayer();
        }
        QVERIFY(first.size() >= 4);
        QCOMPARE(first, second);
        QCOMPARE(
            QSet<QUuid>(first.cbegin(), first.cend()).size(), first.size());
    }
};

int runIdentityTests(int argc, char **argv)
{
    IdentityTests tests;
    return QTest::qExec(&tests, argc, argv);
}

}

#include "IdentityTests.moc"

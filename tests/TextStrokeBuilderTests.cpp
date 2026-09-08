// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyattic)

#include "document/SelectionOperation.hpp"
#include "document/TextStrokeBuilder.hpp"
#include "support/DocumentTestHelpers.hpp"
#include "support/DocumentTestSuites.hpp"

#include <QFont>

namespace ugurugu
{

namespace
{

TextStrokeBuilder::Options defaultOptions()
{
    TextStrokeBuilder::Options options;
    options.text = QStringLiteral("Ug");
    options.font.setPixelSize(64);
    options.anchor = QPointF(20.0, 20.0);
    options.color = QColor(200, 40, 40);
    options.outlineWidth = 4.0;
    options.canvasSize = QSize(256, 192);
    options.baseSeed = 7;
    return options;
}

bool pointsInsideCanvas(const Stroke &stroke, const QSize &canvasSize)
{
    for (const StrokePoint &point : stroke.points)
    {
        if (point.position.x() < 0.0 || point.position.x() > canvasSize.width()
            || point.position.y() < 0.0
            || point.position.y() > canvasSize.height())
        {
            return false;
        }
    }
    return true;
}

}

class TextStrokeBuilderTests final : public QObject
{
    Q_OBJECT

private slots:
    void suppliedGlyphPathMatchesFontLayout()
    {
        auto options = defaultOptions();
        options.filled = true;
        const auto native = TextStrokeBuilder::build(options);
        const auto path =
            TextStrokeBuilder::layoutPath(options.text, options.font);
        options.text.clear();
        const auto headless = TextStrokeBuilder::buildFromPath(path, options);
        QCOMPARE(headless.size(), native.size());
        for (qsizetype index = 0; index < native.size(); ++index)
        {
            QCOMPARE(headless[index].mode, native[index].mode);
            QCOMPARE(headless[index].points, native[index].points);
            QCOMPARE(headless[index].fillCoverage, native[index].fillCoverage);
            QCOMPARE(headless[index].seed, native[index].seed);
        }
    }

    void buildsClosedOutlineContoursDeterministically()
    {
        const TextStrokeBuilder::Options options = defaultOptions();
        const QVector<Stroke> strokes = TextStrokeBuilder::build(options);
        QVERIFY(!strokes.isEmpty());

        QSet<quint64> seeds;
        for (const Stroke &stroke : strokes)
        {
            QCOMPARE(stroke.mode, StrokeMode::Paint);
            QCOMPARE(stroke.color, options.color);
            QCOMPARE(stroke.width, options.outlineWidth);
            QVERIFY(stroke.points.size() >= 4);
            QVERIFY(pointsInsideCanvas(stroke, options.canvasSize));
            seeds.insert(stroke.seed);

            int closureIndex = -1;
            for (int index = 1; index < stroke.points.size(); ++index)
            {
                if (stroke.points[index] == stroke.points.first())
                {
                    closureIndex = index;
                    break;
                }
            }
            QVERIFY(closureIndex > 0);
            QVERIFY(closureIndex < stroke.points.size() - 1);
        }
        QCOMPARE(seeds.size(), strokes.size());

        const QVector<Stroke> again = TextStrokeBuilder::build(options);
        QCOMPARE(again.size(), strokes.size());
        for (int index = 0; index < strokes.size(); ++index)
        {
            QCOMPARE(again[index].points, strokes[index].points);
            QCOMPARE(again[index].seed, strokes[index].seed);
        }
    }

    void fillsGlyphInteriorsWithFrozenCoverage()
    {
        TextStrokeBuilder::Options options = defaultOptions();
        const qsizetype outlineCount = TextStrokeBuilder::build(options).size();
        options.filled = true;
        const QVector<Stroke> strokes = TextStrokeBuilder::build(options);
        QCOMPARE(strokes.size(), outlineCount + 1);

        const Stroke &fill = strokes.first();
        QCOMPARE(fill.mode, StrokeMode::Fill);
        QVERIFY(fill.fillCoverage.has_value());
        const QImage coverage = unpackBinaryMask(*fill.fillCoverage);
        int covered = 0;
        for (int y = 0; y < coverage.height(); ++y)
        {
            const uchar *line = coverage.constScanLine(y);
            for (int x = 0; x < coverage.width(); ++x)
            {
                covered += line[x] >= 128 ? 1 : 0;
            }
        }
        QVERIFY(covered > 0);
        QVERIFY(pointsInsideCanvas(fill, options.canvasSize));
    }

    void rejectsEmptyTextAndCanvas()
    {
        TextStrokeBuilder::Options options = defaultOptions();
        options.text.clear();
        QVERIFY(TextStrokeBuilder::build(options).isEmpty());
        options.text = QStringLiteral("  \n  ");
        QVERIFY(TextStrokeBuilder::build(options).isEmpty());
        options.text = QStringLiteral("Ug");
        options.canvasSize = QSize();
        QVERIFY(TextStrokeBuilder::build(options).isEmpty());
    }

    void clampsPointsNearTheCanvasEdge()
    {
        TextStrokeBuilder::Options options = defaultOptions();
        options.canvasSize = QSize(128, 96);
        options.anchor = QPointF(100.0, 60.0);
        const QVector<Stroke> strokes = TextStrokeBuilder::build(options);
        QVERIFY(!strokes.isEmpty());
        for (const Stroke &stroke : strokes)
        {
            QVERIFY(pointsInsideCanvas(stroke, options.canvasSize));
        }
    }

    void commitsThroughTheControllerAsOneUndoStep()
    {
        Document document = Document::createDefault(QSize(256, 192));
        DocumentController controller;
        controller.loadDocument(document);
        const QUuid layerId = controller.document().activeLayerId;

        TextStrokeBuilder::Options options = defaultOptions();
        options.filled = true;
        const QVector<Stroke> strokes = TextStrokeBuilder::build(options);
        QVERIFY(strokes.size() > 1);

        controller.undoStack()->beginMacro(QStringLiteral("Add text"));
        for (const Stroke &stroke : strokes)
        {
            QCOMPARE(controller.addStroke(layerId, stroke),
                DocumentController::AddStrokeResult::Added);
        }
        controller.undoStack()->endMacro();
        QCOMPARE(controller.document().layers.first().strokes.size(),
            strokes.size());

        controller.undoStack()->undo();
        QCOMPARE(controller.document().layers.first().strokes.size(), 0);
        controller.undoStack()->redo();
        QCOMPARE(controller.document().layers.first().strokes.size(),
            strokes.size());
    }
};

int runTextStrokeBuilderTests(int argc, char **argv)
{
    TextStrokeBuilderTests tests;
    return QTest::qExec(&tests, argc, argv);
}

}

#include "TextStrokeBuilderTests.moc"

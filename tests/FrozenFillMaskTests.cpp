// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "document/FrozenFillMask.hpp"
#include "document/SelectionOperation.hpp"
#include "render/FloodFillMask.hpp"
#include "render/RenderEngine.hpp"
#include "render/StrokeCoverageRenderer.hpp"
#include "support/RenderTestSuites.hpp"

#include <QPainter>
#include <QPainterPath>
#include <QtTest>

#include <limits>

namespace ugurugu
{

class FrozenFillMaskTests final : public QObject
{
    Q_OBJECT

private slots:
    void matchesTheExistingLassoRasterContract()
    {
        const QSize canvasSize(96, 72);
        const QVector<QPointF> polygon = {
            {12.5, 9.0}, {78.0, 14.5}, {67.0, 61.0}, {24.0, 55.5}};
        const auto actual = FrozenFillMask::fromPolygon(canvasSize, polygon);
        QVERIFY(actual.has_value());

        QPainterPath path;
        path.setFillRule(Qt::OddEvenFill);
        path.moveTo(polygon.first());
        for (int index = 1; index < polygon.size(); ++index)
        {
            path.lineTo(polygon[index]);
        }
        path.closeSubpath();
        QImage expected(canvasSize, QImage::Format_Grayscale8);
        expected.fill(0);
        QPainter painter(&expected);
        painter.setRenderHint(QPainter::Antialiasing, false);
        painter.setCompositionMode(QPainter::CompositionMode_Source);
        painter.fillPath(path, Qt::white);
        painter.end();

        QCOMPARE(*actual, expected);
    }

    void producesFrameIndependentFillAndTightCoverage()
    {
        Document document = Document::createDefault(QSize(128, 96));
        document.background = Qt::transparent;
        document.wobbleAmount = 8.0;
        const auto mask = FrozenFillMask::fromPolygon(document.size,
            {{20.0, 18.0}, {92.0, 22.0}, {84.0, 70.0}, {26.0, 74.0}});
        QVERIFY(mask.has_value());

        Stroke fill;
        fill.mode = StrokeMode::Fill;
        fill.color = QColor(45, 120, 220, 190);
        fill.points = {{QPointF(40.0, 40.0), 1.0}};
        fill.fillMask = *mask;
        document.layers.first().strokes = {fill};

        QCOMPARE(RenderEngine::render(document, 0),
            RenderEngine::render(document, document.animationFrames - 1));
        const auto plan =
            StrokeCoverageRenderer::prepare(document, document.layers.first());
        QVERIFY(plan.valid);
        const QRect bounds = StrokeCoverageRenderer::conservativeBounds(
            document, document.layers.first(), 0, plan);
        QVERIFY(!bounds.isEmpty());
        QVERIFY(bounds.width() < document.size.width());
        QVERIFY(bounds.height() < document.size.height());
    }

    void packsFrozenCoverageWithoutChangingPixels()
    {
        const QSize canvasSize(512, 384);
        const QVector<QPointF> polygon = {
            {80.0, 40.0}, {430.0, 70.0}, {380.0, 330.0}, {120.0, 300.0}};
        const auto full = FrozenFillMask::fromPolygon(canvasSize, polygon);
        const auto packed =
            FrozenFillMask::packedFromPolygon(canvasSize, polygon);
        QVERIFY(full.has_value());
        QVERIFY(packed.has_value());
        QCOMPARE(unpackBinaryMask(*packed), *full);
        QVERIFY(static_cast<quint64>(packed->packedMask.size())
                < static_cast<quint64>(full->sizeInBytes()) / 8);
    }

    void rejectsInvalidPolygonContracts()
    {
        QVERIFY(!FrozenFillMask::fromPolygon(
            QSize(), {{0.0, 0.0}, {1.0, 0.0}, {0.0, 1.0}}));
        QVERIFY(!FrozenFillMask::fromPolygon(
            QSize(64, 64), {{0.0, 0.0}, {1.0, 0.0}}));
        QVERIFY(!FrozenFillMask::fromPolygon(QSize(64, 64),
            {{0.0, 0.0},
                {std::numeric_limits<qreal>::infinity(), 0.0},
                {0.0, 1.0}}));
    }

    void sharesAlphaBoundaryFloodFillWithLegacyRendering()
    {
        QImage image(QSize(24, 20), QImage::Format_ARGB32_Premultiplied);
        image.fill(Qt::transparent);
        QPainter painter(&image);
        painter.fillRect(QRect(6, 5, 12, 2), Qt::black);
        painter.fillRect(QRect(6, 13, 12, 2), Qt::black);
        painter.fillRect(QRect(6, 5, 2, 10), Qt::black);
        painter.fillRect(QRect(16, 5, 2, 10), Qt::black);
        painter.end();

        const QPoint seed(10, 10);
        const QImage legacy = RenderEngine::fillRegionMask(image, seed);
        QCOMPARE(FloodFillMask::fromImage(
                     image, seed, FloodFillMask::Comparison::AlphaBoundary, 0),
            legacy);
        QCOMPARE(FloodFillMask::fromImage(
                     image, seed, FloodFillMask::Comparison::Color, 0),
            legacy);
    }

    void appliesColorToleranceToAllRgbaChannels()
    {
        QImage image(QSize(4, 1), QImage::Format_ARGB32_Premultiplied);
        image.setPixelColor(0, 0, QColor(100, 100, 100, 255));
        image.setPixelColor(1, 0, QColor(105, 96, 103, 250));
        image.setPixelColor(2, 0, QColor(112, 100, 100, 255));
        image.setPixelColor(3, 0, QColor(100, 100, 100, 200));

        const QImage tight = FloodFillMask::fromImage(
            image, QPoint(0, 0), FloodFillMask::Comparison::Color, 5);
        QCOMPARE(tight.constScanLine(0)[0], uchar(255));
        QCOMPARE(tight.constScanLine(0)[1], uchar(255));
        QCOMPARE(tight.constScanLine(0)[2], uchar(0));
        QCOMPARE(tight.constScanLine(0)[3], uchar(0));

        const QImage maximum = FloodFillMask::fromImage(
            image, QPoint(0, 0), FloodFillMask::Comparison::Color, 255);
        for (int x = 0; x < image.width(); ++x)
        {
            QCOMPARE(maximum.constScanLine(0)[x], uchar(255));
        }
    }
};

int runFrozenFillMaskTests(int argc, char **argv)
{
    FrozenFillMaskTests tests;
    return QTest::qExec(&tests, argc, argv);
}

}

#include "FrozenFillMaskTests.moc"

// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "render/engine/LayerOperationReplay.hpp"
#include "support/RenderTestHelpers.hpp"
#include "support/RenderTestSuites.hpp"

namespace ugurugu
{

class StrokeRenderingTests final : public QObject
{
    Q_OBJECT

private slots:
    void displacesWobbleLinearlyWithAmount()
    {
        QVector<QPointF> positions;
        for (int index = 0; index <= 20; ++index)
        {
            positions.append(QPointF(10.0 + index * 9.0, 60.0));
        }
        const Stroke stroke = makeStroke(StrokeMode::Paint,
            QColor(10, 20, 30),
            6.0,
            0xfeedbeefULL,
            positions);

        const QPainterPath still = RenderEngine::strokePath(stroke, 3, 12, 0.0);
        const QPainterPath single =
            RenderEngine::strokePath(stroke, 3, 12, 2.0);
        const QPainterPath doubled =
            RenderEngine::strokePath(stroke, 3, 12, 4.0);

        QCOMPARE(single.elementCount(), still.elementCount());
        QCOMPARE(doubled.elementCount(), still.elementCount());
        qreal largestOffset = 0.0;
        for (int index = 0; index < still.elementCount(); ++index)
        {
            const QPointF base(
                still.elementAt(index).x, still.elementAt(index).y);
            const QPointF offsetSingle =
                QPointF(single.elementAt(index).x, single.elementAt(index).y)
                - base;
            const QPointF offsetDouble =
                QPointF(doubled.elementAt(index).x, doubled.elementAt(index).y)
                - base;
            largestOffset = std::max(
                largestOffset, std::hypot(offsetSingle.x(), offsetSingle.y()));
            QVERIFY(std::abs(offsetDouble.x() - offsetSingle.x() * 2.0) < 1e-6);
            QVERIFY(std::abs(offsetDouble.y() - offsetSingle.y() * 2.0) < 1e-6);
        }
        QVERIFY(largestOffset > 0.05);
    }

    void rendersCrispPixelEdges()
    {
        Document document = Document::createDefault(QSize(64, 64));
        document.wobbleAmount = 0.0;
        const QColor strokeColor(255, 120, 120);
        document.layers.first().strokes.append(makeStroke(StrokeMode::Paint,
            strokeColor,
            3.0,
            45,
            {QPointF(7.25, 51.75),
                QPointF(22.5, 11.25),
                QPointF(55.75, 44.5)}));

        const QImage image = RenderEngine::render(document, 0);
        QVERIFY(!image.isNull());
        int paintedPixels = 0;
        for (int y = 0; y < image.height(); ++y)
        {
            for (int x = 0; x < image.width(); ++x)
            {
                const QColor pixel = image.pixelColor(x, y);
                QVERIFY(pixel == document.background || pixel == strokeColor);
                if (pixel == strokeColor)
                {
                    ++paintedPixels;
                }
            }
        }
        QVERIFY(paintedPixels > 0);
    }

    void rendersSmoothEdgesWithAntialiasing()
    {
        Document document = Document::createDefault(QSize(64, 64));
        document.wobbleAmount = 0.0;
        const QColor strokeColor(255, 120, 120);
        Stroke stroke = makeStroke(StrokeMode::Paint,
            strokeColor,
            3.0,
            45,
            {QPointF(7.25, 51.75), QPointF(22.5, 11.25), QPointF(55.75, 44.5)});
        stroke.brush.antialiasing = true;
        document.layers.first().strokes.append(stroke);

        const QImage image = RenderEngine::render(document, 0);
        QVERIFY(!image.isNull());
        int blendedPixels = 0;
        for (int y = 0; y < image.height(); ++y)
        {
            for (int x = 0; x < image.width(); ++x)
            {
                const QColor pixel = image.pixelColor(x, y);
                if (pixel != document.background && pixel != strokeColor)
                {
                    ++blendedPixels;
                }
            }
        }
        QVERIFY(blendedPixels > 0);
    }

    void rejectsUnallocatableCanvas()
    {
        Document document = Document::createDefault(QSize(
            std::numeric_limits<int>::max(), std::numeric_limits<int>::max()));
        QVERIFY(RenderEngine::render(document, 0).isNull());
    }

    void ignoresUnsafeStrokeCoordinates()
    {
        Document document = Document::createDefault(QSize(64, 64));
        Stroke stroke;
        stroke.points = {
            {QPointF(std::numeric_limits<qreal>::infinity(), 16.0), 1.0}};
        document.layers.first().strokes.append(stroke);

        const QImage image = RenderEngine::render(document, 0);
        QVERIFY(!image.isNull());
        QCOMPARE(image.pixelColor(16, 16), QColor(Qt::white));
        QVERIFY(RenderEngine::strokePath(
            stroke, 0, document.animationFrames, document.wobbleAmount)
                .isEmpty());
    }

    void keepsErasersLocalToTheirLayer()
    {
        Document document;
        document.size = QSize(80, 64);
        document.background = QColor(10, 20, 30);
        document.animationFrames = 8;
        document.wobbleAmount = 0.0;

        Layer bottom;
        bottom.name = QStringLiteral("Bottom");
        bottom.strokes.append(makeStroke(StrokeMode::Paint,
            QColor(20, 180, 80),
            20.0,
            10,
            {QPointF(8.0, 32.0), QPointF(72.0, 32.0)}));

        Layer middle;
        middle.name = QStringLiteral("Middle");
        middle.strokes.append(makeStroke(StrokeMode::Paint,
            QColor(220, 40, 50),
            20.0,
            20,
            {QPointF(8.0, 32.0), QPointF(72.0, 32.0)}));
        middle.strokes.append(makeStroke(
            StrokeMode::Erase, Qt::black, 14.0, 30, {QPointF(48.0, 32.0)}));

        Layer top;
        top.name = QStringLiteral("Top");
        top.strokes.append(makeStroke(
            StrokeMode::Erase, Qt::black, 14.0, 40, {QPointF(20.0, 32.0)}));

        document.layers = {bottom, middle, top};
        document.activeLayerId = top.id;

        const QImage image = RenderEngine::render(document, 0);
        QCOMPARE(image.pixelColor(48, 32), QColor(20, 180, 80));
        QCOMPARE(image.pixelColor(20, 32), QColor(220, 40, 50));
    }

    void doesNotFillOpenStrokes()
    {
        Document document = Document::createDefault(QSize(64, 64));
        document.wobbleAmount = 0.0;
        document.layers.first().strokes.append(makeStroke(StrokeMode::Paint,
            Qt::black,
            4.0,
            50,
            {QPointF(8.0, 52.0), QPointF(32.0, 8.0), QPointF(56.0, 52.0)}));

        const QImage image = RenderEngine::render(document, 0);
        QCOMPARE(image.pixelColor(32, 46), QColor(Qt::white));
        QCOMPARE(image.pixelColor(32, 8), QColor(Qt::black));
    }

    void clipsPaintAndFillToSelectionMasks()
    {
        QImage clipMask(QSize(64, 64), QImage::Format_Grayscale8);
        clipMask.fill(0);
        for (int y = 12; y < 52; ++y)
        {
            std::fill_n(clipMask.scanLine(y) + 12, 20, 255);
        }

        Document paintDocument = Document::createDefault(QSize(64, 64));
        paintDocument.wobbleAmount = 0.0;
        Stroke paint = makeStroke(StrokeMode::Paint,
            QColor(220, 30, 40),
            16.0,
            60,
            {QPointF(4.0, 32.0), QPointF(60.0, 32.0)});
        paint.clipMask = clipMask;
        paintDocument.layers.first().strokes.append(paint);
        const QImage painted = RenderEngine::render(paintDocument, 0);
        QCOMPARE(painted.pixelColor(20, 32), paint.color);
        QCOMPARE(painted.pixelColor(45, 32), QColor(Qt::white));

        Document fillDocument = Document::createDefault(QSize(64, 64));
        Stroke fill;
        fill.mode = StrokeMode::Fill;
        fill.color = QColor(30, 80, 220);
        fill.points = {{QPointF(20.0, 32.0), 1.0}};
        fill.clipMask = clipMask;
        fillDocument.layers.first().strokes.append(fill);
        const QImage filled = RenderEngine::render(fillDocument, 0);
        QCOMPARE(filled.pixelColor(20, 32), fill.color);
        QCOMPARE(filled.pixelColor(45, 32), QColor(Qt::white));

        const QImage scaled =
            RenderEngine::renderScaled(fillDocument, 0, QSize(32, 32));
        QCOMPARE(scaled.pixelColor(10, 16), fill.color);
        QCOMPARE(scaled.pixelColor(22, 16), QColor(Qt::white));
    }

    void treatsOnlyFillMaskSamplesAtLeastHalfOpaqueAsIncluded()
    {
        Document document = Document::createDefault(QSize(16, 16));
        document.background = Qt::transparent;
        Stroke fill;
        fill.mode = StrokeMode::Fill;
        fill.color = QColor(30, 80, 220);
        fill.points = {{QPointF(8.0, 8.0), 1.0}};
        fill.fillMask = QImage(document.size, QImage::Format_Grayscale8);
        fill.fillMask.fill(0);
        fill.fillMask.scanLine(3)[3] = 127;
        fill.fillMask.scanLine(11)[11] = 128;
        document.layers.first().strokes.append(fill);

        const QImage rendered = RenderEngine::render(document, 0);
        QCOMPARE(rendered.pixelColor(3, 3), QColor(Qt::transparent));
        QCOMPARE(rendered.pixelColor(11, 11), fill.color);
    }

    void releasesTransientPackedFillMasksBetweenStrokes()
    {
        Document document = Document::createDefault(QSize(32, 32));
        document.background = Qt::transparent;
        document.wobbleAmount = 0.0;
        QVector<Stroke> fills;
        for (const QPoint &point : {QPoint(8, 8), QPoint(24, 24)})
        {
            QImage mask(document.size, QImage::Format_Grayscale8);
            mask.fill(0);
            mask.scanLine(point.y())[point.x()] = 255;
            const std::optional<PackedMaskRegion> packed = packBinaryMask(mask);
            QVERIFY(packed.has_value());
            Stroke fill;
            fill.mode = StrokeMode::Fill;
            fill.color = Qt::black;
            fill.brush.antialiasing = false;
            fill.points = {{QPointF(point), 1.0}};
            fill.fillCoverage = *packed;
            fills.append(fill);
        }

        QImage layer(document.size, QImage::Format_ARGB32_Premultiplied);
        layer.fill(Qt::transparent);
        QHash<qint64, QPainterPath> clipPaths;
        QHash<qint64, QImage> scaledMasks;
        render_detail::LayerStrokeReplayStats stats;
        render_detail::renderLayerStrokes(layer,
            document,
            fills,
            0,
            document.animationFrames,
            1.0,
            1.0,
            clipPaths,
            scaledMasks,
            {},
            &stats);

        QCOMPARE(stats.peakStrokeMaskBackingCount, 1);
        QVERIFY(scaledMasks.isEmpty());
        QCOMPARE(qAlpha(layer.pixel(8, 8)), 255);
        QCOMPARE(qAlpha(layer.pixel(24, 24)), 255);
    }

    void canvasCropPreservesDisconnectedFillPixels()
    {
        Document document = Document::createDefault(QSize(10, 10));
        document.wobbleAmount = 0.0;

        Stroke barrier = makeStroke(StrokeMode::Paint,
            Qt::black,
            1.0,
            61,
            {QPointF(5.0, 0.0), QPointF(5.0, 7.0)});
        barrier.brush.antialiasing = false;
        Stroke fill;
        fill.mode = StrokeMode::Fill;
        fill.color = QColor(30, 80, 220);
        fill.points = {{QPointF(4.0, 8.0), 1.0}};
        document.layers.first().strokes = {barrier, fill};

        const QImage before = RenderEngine::render(document, 0);
        QCOMPARE(before.pixelColor(2, 4), fill.color);
        QCOMPARE(before.pixelColor(8, 4), fill.color);

        DocumentController controller;
        controller.loadDocument(document);
        QVERIFY(controller.resizeCanvas(QSize(10, 8), QPoint()));

        const QImage cropped = RenderEngine::render(controller.document(), 0);
        const QImage expected = before.copy(QRect(0, 0, 10, 8));
        QCOMPARE(cropped, expected);
    }
};

int runStrokeRenderingTests(int argc, char **argv)
{
    StrokeRenderingTests tests;
    return QTest::qExec(&tests, argc, argv);
}

}

#include "StrokeRenderingTests.moc"

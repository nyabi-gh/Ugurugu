// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "support/RenderTestHelpers.hpp"
#include "support/RenderTestSuites.hpp"

namespace ugurugu
{

class BrushRenderingTests final : public QObject
{
    Q_OBJECT

private slots:
    void usesTabletPressureForWidth()
    {
        auto renderPressure = [](qreal pressure)
        {
            Document document = Document::createDefault(QSize(80, 64));
            document.wobbleAmount = 0.0;
            Stroke stroke;
            stroke.color = Qt::black;
            stroke.width = 20.0;
            stroke.points = {{QPointF(10.0, 32.0), pressure},
                {QPointF(70.0, 32.0), pressure}};
            document.layers.first().strokes.append(stroke);
            return RenderEngine::render(document, 0);
        };

        const QImage light = renderPressure(0.1);
        const QImage heavy = renderPressure(1.0);
        int lightPixels = 0;
        int heavyPixels = 0;
        for (int y = 0; y < light.height(); ++y)
        {
            for (int x = 0; x < light.width(); ++x)
            {
                if (light.pixelColor(x, y) != QColor(Qt::white))
                {
                    ++lightPixels;
                }
                if (heavy.pixelColor(x, y) != QColor(Qt::white))
                {
                    ++heavyPixels;
                }
            }
        }
        QVERIFY(heavyPixels > lightPixels * 2);
    }

    void rendersEveryBuiltInBrushDeterministically()
    {
        QSet<QString> ids;
        for (const BrushPreset &preset : BrushPresetCatalog::builtIns())
        {
            QVERIFY2(!ids.contains(preset.id), qPrintable(preset.id));
            ids.insert(preset.id);
            QVERIFY(isValidBrushSettings(preset.settings));

            Document document = Document::createDefault(QSize(128, 96));
            document.wobbleAmount = 0.0;
            Stroke stroke;
            stroke.seed = 0x123456789abcdef0ULL;
            stroke.color = QColor(20, 40, 80);
            stroke.width = std::min(64.0, preset.defaultSize);
            stroke.brush = preset.settings;
            stroke.points = {
                {QPointF(24.0, 48.0), 0.45}, {QPointF(104.0, 48.0), 1.0}};
            document.layers.first().strokes.append(stroke);

            const QImage first = RenderEngine::render(document, 3);
            const QImage second = RenderEngine::render(document, 3);
            QVERIFY2(!first.isNull(), qPrintable(preset.id));
            QVERIFY2(first == second, qPrintable(preset.id));
            QVERIFY2(std::any_of(first.constBits(),
                         first.constBits() + first.sizeInBytes(),
                         [](uchar value)
                         {
                             return value != 255;
                         }),
                qPrintable(preset.id));
        }
        QCOMPARE(ids.size(), BrushPresetCatalog::builtIns().size());
    }

    void rendersSoftAirbrushWithPartialAlpha()
    {
        Document document = Document::createDefault(QSize(80, 80));
        document.background = Qt::transparent;
        document.wobbleAmount = 0.0;
        Stroke stroke;
        stroke.seed = 77;
        stroke.color = Qt::black;
        stroke.width = 48.0;
        stroke.brush =
            BrushPresetCatalog::find(QStringLiteral("soft-airbrush"))->settings;
        stroke.points = {{QPointF(40.0, 40.0), 1.0}};
        document.layers.first().strokes.append(stroke);

        const QImage image = RenderEngine::render(document, 0);
        QVERIFY(!image.isNull());
        const int centerAlpha = image.pixelColor(40, 40).alpha();
        const int middleAlpha = image.pixelColor(52, 40).alpha();
        const int edgeAlpha = image.pixelColor(64, 40).alpha();
        QVERIFY(centerAlpha > middleAlpha);
        QVERIFY(middleAlpha > edgeAlpha);
        QVERIFY(centerAlpha > 0 && centerAlpha < 255);
    }

    void onlyAnimatedSprayChangesWithoutWobble()
    {
        auto renderPreset =
            [](const QString &presetId, int frame, qreal wobbleScale = 1.0)
        {
            Document document = Document::createDefault(QSize(96, 72));
            document.wobbleAmount = 0.0;
            Stroke stroke;
            stroke.seed = 91;
            stroke.color = Qt::black;
            stroke.width = 44.0;
            stroke.brush = BrushPresetCatalog::find(presetId)->settings;
            stroke.brush.wobbleScale = wobbleScale;
            stroke.points = {
                {QPointF(18.0, 36.0), 1.0}, {QPointF(78.0, 36.0), 1.0}};
            document.layers.first().strokes.append(stroke);
            return RenderEngine::render(document, frame);
        };

        QCOMPARE(renderPreset(QStringLiteral("pixel-spray"), 0),
            renderPreset(QStringLiteral("pixel-spray"), 1));
        QVERIFY(renderPreset(QStringLiteral("wobble-spray"), 0)
                != renderPreset(QStringLiteral("wobble-spray"), 1));
        QCOMPARE(renderPreset(QStringLiteral("wobble-spray"), 0, 0.0),
            renderPreset(QStringLiteral("wobble-spray"), 1, 0.0));
    }

    void handlesDotsAndDuplicatePoints()
    {
        Document document = Document::createDefault(QSize(64, 64));
        document.wobbleAmount = 5.0;

        const Stroke dot = makeStroke(
            StrokeMode::Paint, Qt::black, 8.0, 100, {QPointF(16.0, 16.0)});
        const Stroke duplicates = makeStroke(StrokeMode::Paint,
            QColor(200, 20, 40),
            8.0,
            200,
            {QPointF(40.0, 40.0), QPointF(40.0, 40.0), QPointF(40.0, 40.0)});
        document.layers.first().strokes = {dot, duplicates};

        const QPainterPath dotPath =
            RenderEngine::strokePath(dot, 3, document.animationFrames, 5.0);
        const QPainterPath duplicatePath = RenderEngine::strokePath(
            duplicates, 3, document.animationFrames, 5.0);
        QVERIFY(dotPath.elementCount() > 0);
        QVERIFY(duplicatePath.elementCount() > 0);
        const QPainterPath::Element dotElement = dotPath.elementAt(0);
        const QPainterPath::Element duplicateElement =
            duplicatePath.elementAt(0);
        QVERIFY(std::isfinite(dotElement.x));
        QVERIFY(std::isfinite(dotElement.y));
        QVERIFY(std::isfinite(duplicateElement.x));
        QVERIFY(std::isfinite(duplicateElement.y));

        const QImage image = RenderEngine::render(document, 3);
        QVERIFY(!image.isNull());
        QCOMPARE(image.size(), document.size);
        bool containsPaint = false;
        for (int y = 0; y < image.height() && !containsPaint; ++y)
        {
            for (int x = 0; x < image.width(); ++x)
            {
                if (image.pixelColor(x, y) != document.background)
                {
                    containsPaint = true;
                    break;
                }
            }
        }
        QVERIFY(containsPaint);
    }
};

int runBrushRenderingTests(int argc, char **argv)
{
    BrushRenderingTests tests;
    return QTest::qExec(&tests, argc, argv);
}

}

#include "BrushRenderingTests.moc"

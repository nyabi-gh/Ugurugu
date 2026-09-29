// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "document/Document.hpp"
#include "document/DocumentController.hpp"
#include "document/DocumentLimits.hpp"
#include "document/SelectionOperation.hpp"
#include "document/StrokeMask.hpp"
#include "io/DocumentSerializer.hpp"

#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QPainter>
#include <QtTest>

#include <algorithm>
#include <cstring>
#include <limits>

namespace ugurugu
{

namespace
{

Stroke testStroke(const QPointF &start, const QPointF &end)
{
    Stroke stroke;
    stroke.points = {{start, 1.0}, {end, 1.0}};
    return stroke;
}

void setRowPadding(QImage &image, uchar value)
{
    for (int y = 0; y < image.height(); ++y)
    {
        std::fill(image.scanLine(y) + image.width(),
            image.scanLine(y) + image.bytesPerLine(),
            value);
    }
}

QImage legacyTransformedSelectionSupport(const QImage &selectionMask,
    const QSize &targetSize,
    const QTransform &transform,
    SamplingMode sampling)
{
    const std::optional<PackedMaskRegion> packed =
        packBinaryMask(selectionMask);
    if (!packed)
    {
        return {};
    }
    const QRect bounds = packed->bounds;
    QImage source(bounds.size(), QImage::Format_ARGB32_Premultiplied);
    QImage transformed(targetSize, QImage::Format_ARGB32_Premultiplied);
    source.fill(Qt::transparent);
    transformed.fill(Qt::transparent);
    for (int y = bounds.top(); y <= bounds.bottom(); ++y)
    {
        const uchar *input = selectionMask.constScanLine(y);
        auto *output =
            reinterpret_cast<QRgb *>(source.scanLine(y - bounds.top()));
        for (int x = bounds.left(); x <= bounds.right(); ++x)
        {
            if (input[x] >= 128)
            {
                output[x - bounds.left()] =
                    qPremultiply(qRgba(255, 255, 255, 255));
            }
        }
    }
    QPainter painter(&transformed);
    painter.setRenderHint(QPainter::Antialiasing, false);
    painter.setRenderHint(
        QPainter::SmoothPixmapTransform, sampling == SamplingMode::Smooth);
    painter.setCompositionMode(QPainter::CompositionMode_SourceOver);
    painter.setTransform(transform);
    painter.drawImage(bounds.topLeft(), source);
    painter.end();

    QImage support(targetSize, QImage::Format_Grayscale8);
    support.fill(0);
    for (int y = 0; y < transformed.height(); ++y)
    {
        const auto *input =
            reinterpret_cast<const QRgb *>(transformed.constScanLine(y));
        uchar *output = support.scanLine(y);
        for (int x = 0; x < transformed.width(); ++x)
        {
            output[x] = qAlpha(input[x]) >= 128 ? 255 : 0;
        }
    }
    return support;
}

std::optional<PackedMaskRegion> legacyTransformedPackedMask(
    const PackedMaskRegion &region,
    const QSize &targetSize,
    const QTransform &transform)
{
    const QImage source = unpackBinaryMask(region);
    if (source.isNull())
    {
        return std::nullopt;
    }
    QImage target(targetSize, QImage::Format_Grayscale8);
    target.fill(0);
    QPainter painter(&target);
    painter.setRenderHint(QPainter::SmoothPixmapTransform, false);
    painter.setTransform(transform);
    painter.drawImage(QPointF(), source);
    painter.end();
    return packBinaryMask(target);
}

QImage irregularSelectionMask(const QSize &size)
{
    QImage mask(size, QImage::Format_Grayscale8);
    mask.fill(0);
    for (int y = 5; y < size.height() - 5; ++y)
    {
        uchar *line = mask.scanLine(y);
        for (int x = 7; x < size.width() - 7; ++x)
        {
            if (((x * 13 + y * 7) % 11) > 2 && ((x + y) % 9) != 0)
            {
                line[x] = 255;
            }
        }
    }
    return mask;
}

}

class MaskRegressionTests final : public QObject
{
    Q_OBJECT

private slots:
    void canonicalizesFullCanvasVisibilityClips()
    {
        const QSize canvasSize(16, 12);
        const QRect canvasRect(QPoint(), canvasSize);

        Stroke exact = testStroke(QPointF(2.0, 2.0), QPointF(13.0, 9.0));
        exact.visibilityClip = canvasRect;
        QVERIFY(canonicalizeStrokeVisibility(exact, canvasSize));
        QVERIFY(!exact.visibilityClip.has_value());

        Stroke enclosing = exact;
        enclosing.visibilityClip = QRect(-4, -3, 24, 20);
        QVERIFY(canonicalizeStrokeVisibility(enclosing, canvasSize));
        QVERIFY(!enclosing.visibilityClip.has_value());

        Stroke partial = exact;
        partial.visibilityClip = QRect(1, 2, 10, 7);
        QVERIFY(canonicalizeStrokeVisibility(partial, canvasSize));
        QCOMPARE(
            partial.visibilityClip, std::optional<QRect>(QRect(1, 2, 10, 7)));
    }

    void schemaFiveDeduplicatesMasksByContent()
    {
        Document document = Document::createDefault(QSize(65, 17));
        QImage mask(document.size, QImage::Format_Grayscale8);
        mask.fill(0);
        for (int y = 3; y < 12; ++y)
        {
            std::fill(mask.scanLine(y) + 5, mask.scanLine(y) + 45, 255);
        }

        Stroke first = testStroke(QPointF(5.0, 5.0), QPointF(40.0, 5.0));
        first.clipMask = mask;
        Stroke second = testStroke(QPointF(5.0, 8.0), QPointF(40.0, 8.0));
        second.clipMask = mask.copy();
        QVERIFY(first.clipMask.cacheKey() != second.clipMask.cacheKey());
        document.layers.first().strokes = {first, second};

        const QByteArray json = DocumentSerializer::toJson(document);
        QVERIFY(!json.isEmpty());
        const QJsonObject root = QJsonDocument::fromJson(json).object();
        QCOMPARE(root.value(QStringLiteral("schemaVersion")).toInt(), 13);
        const QJsonArray masks =
            root.value(QStringLiteral("clipMasks")).toArray();
        QCOMPARE(masks.size(), 1);

        const QJsonArray strokes = root.value(QStringLiteral("layers"))
                                       .toArray()
                                       .first()
                                       .toObject()
                                       .value(QStringLiteral("strokes"))
                                       .toArray();
        QCOMPARE(strokes[0].toObject().value(QStringLiteral("clipMaskId")),
            strokes[1].toObject().value(QStringLiteral("clipMaskId")));
        QVERIFY(!strokes[0].toObject().contains(QStringLiteral("clipMask")));

        QString error;
        const std::optional<Document> loaded =
            DocumentSerializer::fromJson(json, &error);
        QVERIFY2(loaded.has_value(), qPrintable(error));
        const QVector<Stroke> &loadedStrokes = loaded->layers.first().strokes;
        QCOMPARE(loadedStrokes[0].clipMask, mask);
        QCOMPARE(loadedStrokes[1].clipMask, mask);
        QCOMPARE(loadedStrokes[0].clipMask.cacheKey(),
            loadedStrokes[1].clipMask.cacheKey());
    }

    void schemaFiveIgnoresImageRowPadding()
    {
        Document first = Document::createDefault(QSize(65, 9));
        Stroke stroke = testStroke(QPointF(2.0, 2.0), QPointF(60.0, 6.0));
        stroke.clipMask = QImage(first.size, QImage::Format_Grayscale8);
        stroke.clipMask.fill(127);
        setRowPadding(stroke.clipMask, 0x11);
        first.layers.first().strokes = {stroke};

        Document second = first;
        second.layers.first().strokes[0].clipMask =
            first.layers.first().strokes[0].clipMask.copy();
        setRowPadding(second.layers.first().strokes[0].clipMask, 0xEE);

        QCOMPARE(DocumentSerializer::toJson(first),
            DocumentSerializer::toJson(second));
    }

    void loadsLegacySchemaThreeInlineMask()
    {
        Document source = Document::createDefault(QSize(13, 7));
        Stroke stroke = testStroke(QPointF(1.0, 1.0), QPointF(10.0, 5.0));
        stroke.clipMask = QImage(source.size, QImage::Format_Grayscale8);
        stroke.clipMask.fill(0);
        for (int y = 2; y < 5; ++y)
        {
            std::fill(stroke.clipMask.scanLine(y) + 3,
                stroke.clipMask.scanLine(y) + 9,
                255);
        }
        source.layers.first().strokes = {stroke};

        QJsonObject root =
            QJsonDocument::fromJson(DocumentSerializer::toJson(source))
                .object();
        root.insert(QStringLiteral("schemaVersion"), 3);
        root.remove(QStringLiteral("clipMasks"));
        QJsonArray layers = root.value(QStringLiteral("layers")).toArray();
        QJsonObject layer = layers.first().toObject();
        QJsonArray strokes = layer.value(QStringLiteral("strokes")).toArray();
        QJsonObject legacyStroke = strokes.first().toObject();
        legacyStroke.remove(QStringLiteral("clipMaskId"));
        const QByteArray bytes(
            reinterpret_cast<const char *>(stroke.clipMask.constBits()),
            stroke.clipMask.sizeInBytes());
        QJsonObject legacyMask;
        legacyMask.insert(QStringLiteral("width"), stroke.clipMask.width());
        legacyMask.insert(QStringLiteral("height"), stroke.clipMask.height());
        legacyMask.insert(QStringLiteral("data"),
            QString::fromLatin1(qCompress(bytes, 6).toBase64()));
        legacyStroke.insert(QStringLiteral("clipMask"), legacyMask);
        strokes[0] = legacyStroke;
        layer.insert(QStringLiteral("strokes"), strokes);
        layers[0] = layer;
        root.insert(QStringLiteral("layers"), layers);

        QString error;
        const std::optional<Document> loaded = DocumentSerializer::fromJson(
            QJsonDocument(root).toJson(QJsonDocument::Compact), &error);
        QVERIFY2(loaded.has_value(), qPrintable(error));
        QCOMPARE(
            loaded->layers.first().strokes.first().clipMask, stroke.clipMask);
    }

    void rejectsTamperedContentAddress()
    {
        Document source = Document::createDefault(QSize(16, 16));
        Stroke stroke = testStroke(QPointF(1.0, 1.0), QPointF(14.0, 14.0));
        stroke.clipMask = QImage(source.size, QImage::Format_Grayscale8);
        stroke.clipMask.fill(255);
        source.layers.first().strokes = {stroke};

        QJsonObject root =
            QJsonDocument::fromJson(DocumentSerializer::toJson(source))
                .object();
        QJsonArray masks = root.value(QStringLiteral("clipMasks")).toArray();
        QJsonObject mask = masks.first().toObject();
        const QString tamperedId(64, QLatin1Char('0'));
        mask.insert(QStringLiteral("id"), tamperedId);
        masks[0] = mask;
        root.insert(QStringLiteral("clipMasks"), masks);
        QJsonArray layers = root.value(QStringLiteral("layers")).toArray();
        QJsonObject layer = layers.first().toObject();
        QJsonArray strokes = layer.value(QStringLiteral("strokes")).toArray();
        QJsonObject tamperedStroke = strokes.first().toObject();
        tamperedStroke.insert(QStringLiteral("clipMaskId"), tamperedId);
        strokes[0] = tamperedStroke;
        layer.insert(QStringLiteral("strokes"), strokes);
        layers[0] = layer;
        root.insert(QStringLiteral("layers"), layers);

        QString error;
        QVERIFY(!DocumentSerializer::fromJson(
            QJsonDocument(root).toJson(QJsonDocument::Compact), &error));
        QVERIFY(!error.isEmpty());
    }

    void partialSelectionReusesDerivedMasks()
    {
        Document document = Document::createDefault(QSize(128, 128));
        Stroke first = testStroke(QPointF(10.0, 50.0), QPointF(110.0, 50.0));
        Stroke second = testStroke(QPointF(10.0, 70.0), QPointF(110.0, 70.0));
        const QUuid firstId = first.id;
        const QUuid secondId = second.id;
        document.layers.first().strokes = {first, second};

        QImage selection(document.size, QImage::Format_Grayscale8);
        selection.fill(0);
        for (int y = 30; y < 90; ++y)
        {
            std::fill(
                selection.scanLine(y) + 40, selection.scanLine(y) + 90, 255);
        }

        DocumentController controller;
        controller.loadDocument(document);
        QVERIFY(controller.moveStrokes(document.activeLayerId,
            {firstId, secondId},
            QPointF(5.0, 0.0),
            selection));

        const QVector<Stroke> &strokes =
            controller.document().layers.first().strokes;
        QCOMPARE(strokes.size(), 3);
        QCOMPARE(strokes[0].id, firstId);
        QCOMPARE(strokes[1].id, secondId);
        QVERIFY(strokes[0].clipMask.isNull());
        QVERIFY(strokes[1].clipMask.isNull());
        QCOMPARE(strokes.last().mode, StrokeMode::PixelSelection);
        QVERIFY(strokes.last().pixelSelectionOp.has_value());
        QVERIFY(!strokes.last().pixelSelectionOp->packedMask.isEmpty());
    }

    void transformedSelectionSupportMatchesLegacyRasterization()
    {
        const QImage mask = irregularSelectionMask(QSize(79, 61));
        struct TransformCase
        {
            QSize targetSize;
            QTransform transform;
            SamplingMode sampling = SamplingMode::Smooth;
        };
        QVector<TransformCase> cases;

        QTransform integerTranslation;
        integerTranslation.translate(11.0, -9.0);
        cases.append(
            {QSize(79, 61), integerTranslation, SamplingMode::Nearest});

        QTransform flip;
        flip.translate(79.0, 0.0);
        flip.scale(-1.0, 1.0);
        cases.append({QSize(79, 61), flip, SamplingMode::Nearest});

        QTransform fractionalTranslation;
        fractionalTranslation.translate(3.25, 7.75);
        cases.append(
            {QSize(91, 73), fractionalTranslation, SamplingMode::Smooth});

        QTransform rotation;
        rotation.translate(39.5, 30.5);
        rotation.rotate(27.0);
        rotation.translate(-39.5, -30.5);
        cases.append({QSize(79, 61), rotation, SamplingMode::Smooth});

        QTransform scale;
        scale.translate(9.3, -2.4);
        scale.scale(1.37, 0.68);
        cases.append({QSize(103, 57), scale, SamplingMode::Smooth});

        QTransform partialOutside;
        partialOutside.translate(-48.5, 42.25);
        partialOutside.rotate(-11.0);
        cases.append({QSize(47, 83), partialOutside, SamplingMode::Smooth});

        QTransform sizeChange;
        sizeChange.translate(6.5, 4.25);
        sizeChange.scale(0.8, 1.3);
        cases.append({QSize(113, 89), sizeChange, SamplingMode::Smooth});

        // This combination is not emitted by makePixelSelectionOp(), but
        // remains supported for serialized compatibility.
        cases.append({QSize(79, 61), rotation, SamplingMode::Nearest});

        int falsePositives = 0;
        int falseNegatives = 0;
        for (const TransformCase &entry : cases)
        {
            const QImage expected = legacyTransformedSelectionSupport(
                mask, entry.targetSize, entry.transform, entry.sampling);
            SelectionTransformMemoryStats stats;
            const QImage actual = transformedSelectionSupport(mask,
                entry.targetSize,
                entry.transform,
                entry.sampling,
                &stats);
            QVERIFY(!actual.isNull());
            QCOMPARE(actual.size(), expected.size());
            for (int y = 0; y < actual.height(); ++y)
            {
                const uchar *actualLine = actual.constScanLine(y);
                const uchar *expectedLine = expected.constScanLine(y);
                for (int x = 0; x < actual.width(); ++x)
                {
                    falsePositives +=
                        actualLine[x] >= 128 && expectedLine[x] < 128 ? 1 : 0;
                    falseNegatives +=
                        actualLine[x] < 128 && expectedLine[x] >= 128 ? 1 : 0;
                }
            }
            QVERIFY(!stats.usedFullTargetFallback);
            QVERIFY(!stats.usedArgbSource);
            QVERIFY(!stats.usedArgbTarget);
        }
        qInfo("deterministic selection support differs from Qt at >=128 by "
              "%d false positives and %d false negatives",
            falsePositives,
            falseNegatives);
        QVERIFY(falsePositives <= 100);
        QVERIFY(falseNegatives <= 60);
    }

    void transformedPackedMaskMatchesLegacyRasterization()
    {
        const QImage mask = irregularSelectionMask(QSize(79, 61));
        const std::optional<PackedMaskRegion> packed = packBinaryMask(mask);
        QVERIFY(packed.has_value());

        struct TransformCase
        {
            const char *name = nullptr;
            QSize targetSize;
            QTransform transform;
            bool expectsFallback = false;
        };
        QVector<TransformCase> cases;
        QTransform translation;
        translation.translate(7.0, 9.0);
        cases.append({"translation", QSize(91, 73), translation, false});
        QTransform flip;
        flip.translate(79.0, 0.0);
        flip.scale(-1.0, 1.0);
        cases.append({"flip", QSize(79, 61), flip, false});
        QTransform rotation;
        rotation.translate(39.5, 30.5);
        rotation.rotate(19.0);
        rotation.translate(-39.5, -30.5);
        cases.append({"rotation", QSize(79, 61), rotation, true});
        QTransform scale;
        scale.translate(-13.5, 8.25);
        scale.scale(1.4, 0.7);
        cases.append({"scale", QSize(57, 89), scale, true});

        for (const TransformCase &entry : cases)
        {
            const std::optional<PackedMaskRegion> expected =
                legacyTransformedPackedMask(
                    *packed, entry.targetSize, entry.transform);
            SelectionTransformMemoryStats stats;
            const std::optional<PackedMaskRegion> actual =
                transformedPackedMask(
                    *packed, entry.targetSize, entry.transform, &stats);
            QVERIFY2(actual == expected, entry.name);
            QCOMPARE(stats.usedFullTargetFallback, entry.expectsFallback);
            QVERIFY(!stats.usedArgbSource);
            QVERIFY(!stats.usedArgbTarget);
        }
    }

    void boundsFourKSelectionTransformWorkingSurfaces()
    {
        constexpr quint64 mebibyte = 1024ULL * 1024ULL;
        const QSize canvasSize(4096, 4096);
        QImage mask(canvasSize, QImage::Format_Grayscale8);
        mask.fill(255);

        QTransform fractionalTranslation;
        fractionalTranslation.translate(0.25, 0.5);
        SelectionTransformMemoryStats fullStats;
        QImage fullResult = transformedSelectionSupport(mask,
            canvasSize,
            fractionalTranslation,
            SamplingMode::Smooth,
            &fullStats);
        QVERIFY(!fullResult.isNull());
        QCOMPARE(fullResult.size(), canvasSize);
        QCOMPARE(fullResult.format(), QImage::Format_Grayscale8);
        QCOMPARE(fullStats.sourceImageBytes, 16 * mebibyte);
        QCOMPARE(fullStats.targetImageBytes, 16 * mebibyte);
        QCOMPARE(fullStats.resultBytes, 16 * mebibyte);
        QVERIFY(fullStats.peakLiveImageBytes <= 32 * mebibyte);
        QVERIFY(!fullStats.usedArgbSource);
        QVERIFY(!fullStats.usedArgbTarget);
        QVERIFY(!fullStats.usedFullTargetFallback);
        fullResult = {};

        mask.fill(0);
        for (int y = 2016; y < 2080; ++y)
        {
            std::fill(mask.scanLine(y) + 2016, mask.scanLine(y) + 2080, 255);
        }
        QTransform rotation;
        rotation.translate(2048.0, 2048.0);
        rotation.rotate(37.0);
        rotation.translate(-2048.0, -2048.0);
        SelectionTransformMemoryStats roiStats;
        const QImage roiResult = transformedSelectionSupport(
            mask, canvasSize, rotation, SamplingMode::Smooth, &roiStats);
        QVERIFY(!roiResult.isNull());
        QVERIFY(roiStats.sourceImageBytes < mebibyte);
        QVERIFY(roiStats.targetImageBytes < mebibyte);
        QVERIFY(roiStats.resultBytes == 16 * mebibyte);
        QVERIFY(roiStats.peakLiveImageBytes < 17 * mebibyte);
        QVERIFY(roiStats.targetBounds.size() != canvasSize);
        QVERIFY(!roiStats.usedArgbSource);
        QVERIFY(!roiStats.usedArgbTarget);
        QVERIFY(!roiStats.usedFullTargetFallback);

        mask.fill(255);
        const std::optional<PackedMaskRegion> packed = packBinaryMask(mask);
        QVERIFY(packed.has_value());
        SelectionTransformMemoryStats packedStats;
        const std::optional<PackedMaskRegion> transformed =
            transformedPackedMask(
                *packed, canvasSize, QTransform(), &packedStats);
        QVERIFY(transformed.has_value());
        QVERIFY(packedStats.peakLiveImageBytes <= 32 * mebibyte);
        QCOMPARE(packedStats.sourceImageBytes, 16 * mebibyte);
        QCOMPARE(packedStats.targetImageBytes, 16 * mebibyte);
        QCOMPARE(packedStats.resultBytes,
            static_cast<quint64>(transformed->packedMask.size()));
        QVERIFY(!packedStats.usedArgbSource);
        QVERIFY(!packedStats.usedArgbTarget);
        QVERIFY(!packedStats.usedFullTargetFallback);
    }

    void rejectsUnsafeSelectionTransformInputs()
    {
        const QSize canvasSize(32, 32);
        const QImage mask = irregularSelectionMask(canvasSize);
        const std::optional<PackedMaskRegion> packed = packBinaryMask(mask);
        QVERIFY(packed.has_value());

        const QSize oversizedTarget(
            DocumentLimits::maximumCanvasEdge + 1, canvasSize.height());
        SelectionTransformMemoryStats stats;
        stats.sourceImageBytes = 123;
        QVERIFY(transformedSelectionSupport(
            mask, oversizedTarget, QTransform(), SamplingMode::Nearest, &stats)
                .isNull());
        QCOMPARE(stats.sourceImageBytes, quint64(0));

        stats.targetImageBytes = 123;
        QVERIFY(!transformedPackedMask(
            *packed, oversizedTarget, QTransform(), &stats)
                .has_value());
        QCOMPARE(stats.targetImageBytes, quint64(0));

        const qreal nan = std::numeric_limits<qreal>::quiet_NaN();
        const QVector<QTransform> invalidTransforms = {
            QTransform(nan, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0),
            QTransform(1.0, 0.0, 0.001, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0),
            QTransform::fromScale(0.0, 1.0)};
        for (const QTransform &transform : invalidTransforms)
        {
            QVERIFY(transformedSelectionSupport(
                mask, canvasSize, transform, SamplingMode::Nearest)
                    .isNull());
            QVERIFY(!transformedPackedMask(*packed, canvasSize, transform)
                    .has_value());
        }
    }

    void canvasResizeReusesTransformedMasks()
    {
        Document document = Document::createDefault(QSize(80, 60));
        QImage mask(document.size, QImage::Format_Grayscale8);
        mask.fill(0);
        for (int y = 15; y < 35; ++y)
        {
            std::fill(mask.scanLine(y) + 20, mask.scanLine(y) + 50, 255);
        }

        Stroke first = testStroke(QPointF(20.0, 20.0), QPointF(50.0, 20.0));
        first.clipMask = mask;
        Stroke second = testStroke(QPointF(20.0, 30.0), QPointF(50.0, 30.0));
        second.clipMask = mask;
        document.layers.first().strokes = {first, second};

        DocumentController controller;
        controller.loadDocument(document);
        QVERIFY(controller.resizeCanvas(QSize(90, 70), QPoint(7, 9)));

        const QVector<Stroke> &strokes =
            controller.document().layers.first().strokes;
        QCOMPARE(strokes.size(), 3);
        QCOMPARE(strokes[0].clipMask.size(), document.size);
        QCOMPARE(
            strokes[0].clipMask.cacheKey(), strokes[1].clipMask.cacheKey());
        QCOMPARE(strokes.last().mode, StrokeMode::Reframe);
        QVERIFY(strokes.last().reframeOp.has_value());
        QCOMPARE(strokes.last().reframeOp->contentOffset, QPoint(7, 9));
    }
};

int runMaskRegressionTests(int argc, char **argv)
{
    MaskRegressionTests tests;
    return QTest::qExec(&tests, argc, argv);
}

}

#include "MaskRegressionTests.moc"

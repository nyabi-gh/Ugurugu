// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "io/DocumentSerializer.hpp"
#include "io/WawaV10Importer.hpp"
#include "io/WawaV10Reader.hpp"
#include "render/RenderEngine.hpp"
#include "support/DocumentTestSuites.hpp"

#include <QBuffer>
#include <QtEndian>
#include <QtTest>

#include <bit>

namespace ugurugu
{

namespace
{

void appendByte(QByteArray &data, quint8 value)
{
    data.append(static_cast<char>(value));
}

void appendInt32(QByteArray &data, qint32 value)
{
    const quint32 encoded = qToLittleEndian(static_cast<quint32>(value));
    data.append(reinterpret_cast<const char *>(&encoded), sizeof(encoded));
}

void appendSingle(QByteArray &data, float value)
{
    appendInt32(data, static_cast<qint32>(std::bit_cast<quint32>(value)));
}

void appendString(QByteArray &data, const QString &value)
{
    const QByteArray utf8 = value.toUtf8();
    quint32 length = static_cast<quint32>(utf8.size());
    do
    {
        quint8 byte = static_cast<quint8>(length & 0x7fU);
        length >>= 7U;
        if (length != 0)
        {
            byte |= 0x80U;
        }
        appendByte(data, byte);
    } while (length != 0);
    data.append(utf8);
}

void appendPoints(QByteArray &data, const QVector<QPointF> &points)
{
    appendInt32(data, static_cast<qint32>(points.size()));
    for (const QPointF &point : points)
    {
        appendSingle(data, static_cast<float>(point.x()));
        appendSingle(data, static_cast<float>(point.y()));
    }
}

void appendStroke(QByteArray &data,
    quint32 argb,
    int size,
    int opacity,
    bool airbrush,
    int seed,
    int order,
    const QVector<QPointF> &points)
{
    appendInt32(data, static_cast<qint32>(argb));
    appendInt32(data, size);
    appendInt32(data, opacity);
    appendByte(data, static_cast<quint8>(airbrush));
    appendInt32(data, seed);
    appendInt32(data, order);
    appendPoints(data, points);
}

void appendFill(QByteArray &data,
    quint32 argb,
    int opacity,
    int seed,
    int order,
    const QVector<QPointF> &points)
{
    appendInt32(data, static_cast<qint32>(argb));
    appendInt32(data, opacity);
    appendInt32(data, seed);
    appendInt32(data, order);
    appendPoints(data, points);
}

QByteArray encodedLayerImage(
    const QSize &size, const QColor &color = QColor(12, 34, 56, 78))
{
    QImage image(size, QImage::Format_RGBA8888);
    image.fill(color);
    QByteArray encoded;
    QBuffer buffer(&encoded);
    if (!buffer.open(QIODevice::WriteOnly) || !image.save(&buffer, "PNG"))
    {
        return {};
    }
    return encoded;
}

QByteArray projectHeader(int layerCount)
{
    QByteArray data;
    appendString(data, QStringLiteral("WAWA"));
    appendInt32(data, 10);
    appendInt32(data, 8);
    appendInt32(data, 6);
    appendInt32(data, 0);
    appendInt32(data, 14);
    appendInt32(data, 83);
    appendInt32(data, 32);
    appendInt32(data, 16);
    appendInt32(data, 8);
    appendInt32(data, 12);
    appendInt32(data, 2);
    appendInt32(data, 4);
    appendInt32(data, 7);
    appendInt32(data, 25);
    appendByte(data, 1);
    appendByte(data, 1);
    appendInt32(data, 45);
    appendInt32(data, 50);
    appendByte(data, 1);
    appendInt32(data, static_cast<qint32>(0xff112233U));
    appendInt32(data, static_cast<qint32>(0x80445566U));
    appendByte(data, 0);
    appendByte(data, 1);
    appendInt32(data, layerCount);
    return data;
}

QByteArray representativeProject()
{
    QByteArray data = projectHeader(1);
    appendString(data, QStringLiteral("Ink"));
    appendByte(data, 1);
    const QByteArray image = encodedLayerImage(QSize(8, 6));
    appendInt32(data, static_cast<qint32>(image.size()));
    data.append(image);
    appendInt32(data, 1);
    appendStroke(
        data, 0xff123456U, 9, 75, false, 1234, 2, {{1.0, 2.0}, {3.0, 4.0}});
    appendInt32(data, 1);
    appendStroke(
        data, 0xff000000U, 12, 100, true, 5678, 3, {{4.0, 3.0}, {5.0, 2.0}});
    appendInt32(data, 1);
    appendFill(
        data, 0x80123456U, 60, 9012, 1, {{1.0, 1.0}, {6.0, 1.0}, {3.0, 5.0}});
    return data;
}

QByteArray imageOnlyProject(const QVector<QByteArray> &images)
{
    QByteArray data = projectHeader(static_cast<int>(images.size()));
    for (int index = 0; index < images.size(); ++index)
    {
        appendString(data, QStringLiteral("Layer %1").arg(index + 1));
        appendByte(data, 1);
        const QByteArray &image = images[index];
        appendInt32(data, static_cast<qint32>(image.size()));
        data.append(image);
        appendInt32(data, 0);
        appendInt32(data, 0);
        appendInt32(data, 0);
    }
    return data;
}

}

class WawaV10ReaderTestAccess final
{
public:
    static std::optional<WawaProject> read(const QByteArray &data,
        quint64 maximumDecodedImageBytes,
        QString *error = nullptr)
    {
        return WawaV10Reader::read(data, maximumDecodedImageBytes, error);
    }
};

class WawaV10ReaderTests final : public QObject
{
    Q_OBJECT

private slots:
    void readsNativeVersionTen()
    {
        QString error;
        const std::optional<WawaProject> project =
            WawaV10Reader::read(representativeProject(), &error);
        QVERIFY2(project.has_value(), qPrintable(error));
        QCOMPARE(project->canvasSize, QSize(8, 6));
        QCOMPARE(project->settings.activeLayer, 0);
        QCOMPARE(project->settings.wobbleMode, 2);
        QCOMPARE(project->settings.wobbleHoldFrames, 4);
        QCOMPARE(project->settings.wobbleRandomness, 25);
        QVERIFY(project->settings.linkedWiggle);
        QVERIFY(project->settings.brokenLine);
        QVERIFY(project->settings.wobbleEraser);
        QCOMPARE(project->settings.brushColor, QColor(0x11, 0x22, 0x33));
        QCOMPARE(
            project->settings.backgroundColor, QColor(0x44, 0x55, 0x66, 0x80));
        QCOMPARE(project->layers.size(), 1);
        const WawaLayer &layer = project->layers.first();
        QCOMPARE(layer.name, QStringLiteral("Ink"));
        QVERIFY(layer.visible);
        QCOMPARE(layer.baseImage.size(), project->canvasSize);
        QCOMPARE(layer.baseImage.format(), QImage::Format_RGBA8888);
        QCOMPARE(layer.paintStrokes.size(), 1);
        QCOMPARE(layer.eraserStrokes.size(), 1);
        QCOMPARE(layer.fills.size(), 1);
        QCOMPARE(layer.paintStrokes.first().order, 2);
        QCOMPARE(layer.eraserStrokes.first().order, 3);
        QCOMPARE(layer.fills.first().order, 1);
        QCOMPARE(layer.fills.first().points.size(), 3);
    }

    void rejectsWebJson()
    {
        QString error;
        QVERIFY(!WawaV10Reader::read(
            QByteArrayLiteral("{\"version\":10}"), &error));
        QVERIFY(error.contains(QStringLiteral("Web JSON")));
    }

    void rejectsOtherNativeVersions()
    {
        QByteArray data = representativeProject();
        data[5] = 9;
        QString error;
        QVERIFY(!WawaV10Reader::read(data, &error));
        QVERIFY(error.contains(QStringLiteral("version 10")));
    }

    void rejectsTruncatedAndTrailingData()
    {
        QByteArray truncated = representativeProject();
        truncated.chop(1);
        QVERIFY(!WawaV10Reader::read(truncated));

        QByteArray trailing = representativeProject();
        trailing.append('\0');
        QString error;
        QVERIFY(!WawaV10Reader::read(trailing, &error));
        QVERIFY(error.contains(QStringLiteral("trailing")));
    }

    void deduplicatesLayerImagesBeforeEnforcingTheDecodedBudget()
    {
        constexpr quint64 decodedLayerBytes = 8ULL * 6ULL * 4ULL;
        const QByteArray first = encodedLayerImage(QSize(8, 6));
        const QByteArray duplicateData = imageOnlyProject({first, first});
        QString error;
        const std::optional<WawaProject> duplicates =
            WawaV10ReaderTestAccess::read(
                duplicateData, decodedLayerBytes, &error);
        QVERIFY2(duplicates.has_value(), qPrintable(error));
        QCOMPARE(duplicates->layers.size(), 2);
        QCOMPARE(duplicates->layers[0].baseImage.cacheKey(),
            duplicates->layers[1].baseImage.cacheKey());

        const QByteArray second =
            encodedLayerImage(QSize(8, 6), QColor(90, 40, 210, 255));
        const QByteArray distinctData = imageOnlyProject({first, second});
        error.clear();
        QVERIFY(!WawaV10ReaderTestAccess::read(
            distinctData, decodedLayerBytes, &error));
        QVERIFY(!error.isEmpty());

        error.clear();
        const std::optional<WawaProject> project =
            WawaV10ReaderTestAccess::read(
                distinctData, decodedLayerBytes * 2ULL, &error);
        QVERIFY2(project.has_value(), qPrintable(error));
        QCOMPARE(project->layers.size(), 2);

        const QByteArray transparentFirst =
            encodedLayerImage(QSize(8, 6), QColor(10, 20, 30, 0));
        const QByteArray transparentSecond =
            encodedLayerImage(QSize(8, 6), QColor(200, 100, 50, 0));
        error.clear();
        const std::optional<WawaProject> transparent =
            WawaV10ReaderTestAccess::read(
                imageOnlyProject({transparentFirst, transparentSecond}),
                0,
                &error);
        QVERIFY2(transparent.has_value(), qPrintable(error));
        QCOMPARE(transparent->layers[0].baseImage.cacheKey(),
            transparent->layers[1].baseImage.cacheKey());
    }

    void importsNativeOperationsAsAnUnsavedWwpDocument()
    {
        QString error;
        const std::optional<WawaImportResult> imported =
            WawaV10Importer::import(representativeProject(), &error);
        QVERIFY2(imported.has_value(), qPrintable(error));
        const Document &document = imported->document;
        QCOMPARE(document.size, QSize(8, 6));
        QCOMPARE(document.background, QColor(0x44, 0x55, 0x66, 0x80));
        QCOMPARE(document.motion.style, MotionStyle::Stepped);
        QCOMPARE(document.motion.poseCount, 8);
        QCOMPARE(document.motion.detail, 12);
        QCOMPARE(document.motion.randomness, 0.25);
        QVERIFY(document.motion.brokenLine);
        QCOMPARE(document.motion.breakAmount, 0.45);
        QCOMPARE(document.motion.breakRange, 50.0);
        QCOMPARE(document.layers.size(), 1);
        QCOMPARE(document.activeLayerId, document.layers.first().id);
        QCOMPARE(document.rasterAssets.size(), 1);

        const QVector<Stroke> &operations = document.layers.first().strokes;
        QCOMPARE(operations.size(), 4);
        QCOMPARE(operations[0].mode, StrokeMode::Image);
        QCOMPARE(operations[1].mode, StrokeMode::Fill);
        QCOMPARE(operations[2].mode, StrokeMode::Paint);
        QCOMPARE(operations[3].mode, StrokeMode::Erase);
        QVERIFY(operations[1].fillCoverage.has_value());
        QCOMPARE(operations[2].width, 9.0);
        QCOMPARE(operations[2].color.alpha(), 191);
        QCOMPARE(operations[3].brush.engine, BrushEngine::Airbrush);
        QCOMPARE(operations[3].brush.wobbleScale, 1.0);

        QCOMPARE(imported->summary.layers, 1);
        QCOMPARE(imported->summary.baseImages, 1);
        QCOMPARE(imported->summary.paintStrokes, 1);
        QCOMPARE(imported->summary.eraserStrokes, 1);
        QCOMPARE(imported->summary.polygonFills, 1);
        QCOMPARE(imported->summary.skippedOperations, 0);
        QVERIFY(!DocumentSerializer::toJson(document).isEmpty());
        QVERIFY(!RenderEngine::render(document, 0).isNull());
    }
};

int runWawaV10ReaderTests(int argc, char **argv)
{
    WawaV10ReaderTests tests;
    return QTest::qExec(&tests, argc, argv);
}

}

#include "WawaV10ReaderTests.moc"

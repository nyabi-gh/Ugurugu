// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "app/MemoryBudget.hpp"
#include "io/AnimationExportPolicy.hpp"
#include "io/DocumentSerializer.hpp"
#include "io/GifWriter.hpp"
#include "io/RenderExportPolicy.hpp"
#include "render/LayerCompositionPlan.hpp"
#include "render/RenderEngine.hpp"
#include "ui/GifExportDialog.hpp"

#include <QFile>
#include <QFileInfo>
#include <QImageReader>
#include <QTemporaryDir>
#include <QtTest>

#include <cmath>
#include <limits>

namespace ugurugu
{

namespace
{

Document deepClippedExportDocument(const QSize &size)
{
    Document document = Document::createDefault(size);
    document.layers.clear();
    document.activeLayerId = QUuid();
    document.animationFrames = 2;

    QUuid parentGroupId;
    for (int depth = 0; depth <= 8; ++depth)
    {
        Layer base;
        base.name = QStringLiteral("Base %1").arg(depth);
        base.parentGroupId = parentGroupId;
        base.initialCanvasSize = size;
        document.layers.append(base);
        if (document.activeLayerId.isNull())
        {
            document.activeLayerId = base.id;
        }
        if (depth == 8)
        {
            Layer clipped;
            clipped.name = QStringLiteral("Clipped");
            clipped.parentGroupId = parentGroupId;
            clipped.clipToLayerBelow = true;
            clipped.initialCanvasSize = size;
            document.layers.append(clipped);
            break;
        }
        Layer group;
        group.name = QStringLiteral("Group %1").arg(depth);
        group.kind = LayerKind::Group;
        group.parentGroupId = parentGroupId;
        group.initialCanvasSize = size;
        document.layers.append(group);
        parentGroupId = group.id;
    }
    return document;
}

}

class GifWriterTests final : public QObject
{
    Q_OBJECT

private slots:
    void enforcesSharedAnimationMemoryBudget()
    {
        QVERIFY(AnimationExportPolicy::fitsMemoryBudget(QSize(4096, 4096), 2));
        QVERIFY(!AnimationExportPolicy::fitsMemoryBudget(QSize(4096, 4096), 3));
        QVERIFY(
            AnimationExportPolicy::estimatedWorkingBytes(QSize(4096, 4096), 3)
            > static_cast<long double>(
                MemoryBudget::animationExportWorkingBytes));
    }

    void includesHierarchyAndPaintScratchInExportBudget()
    {
        constexpr long double mebibyte = 1024.0L * 1024.0L;
        Document shallow = Document::createDefault(QSize(4096, 4096));
        shallow.animationFrames = 2;
        const RenderExportMemoryEstimate shallowStatic =
            RenderExportPolicy::staticImage(shallow);
        const RenderExportMemoryEstimate shallowAnimation =
            RenderExportPolicy::animatedGif(shallow);
        QVERIFY(shallowStatic.valid);
        QVERIFY(shallowAnimation.valid);
        QCOMPARE(shallowStatic.hierarchyTransientBytes, 128.0L * mebibyte);
        QCOMPARE(shallowStatic.workingBytes, 320.0L * mebibyte);
        QCOMPARE(shallowAnimation.workingBytes, 384.0L * mebibyte);
        QVERIFY(RenderExportPolicy::staticImageFitsMemoryBudget(shallow));
        QVERIFY(RenderExportPolicy::animatedGifFitsMemoryBudget(shallow));
        QVERIFY(AnimationExportPolicy::fitsMemoryBudget(shallow));

        const Document deep = deepClippedExportDocument(QSize(4096, 4096));
        QVERIFY(!DocumentSerializer::toJson(deep).isEmpty());
        const RenderExportMemoryEstimate deepStatic =
            RenderExportPolicy::staticImage(deep);
        const RenderExportMemoryEstimate deepAnimation =
            RenderExportPolicy::animatedGif(deep);
        QVERIFY(deepStatic.valid);
        QVERIFY(deepAnimation.valid);
        QCOMPARE(deepStatic.hierarchyTransientBytes, 704.0L * mebibyte);
        QCOMPARE(deepStatic.workingBytes, 896.0L * mebibyte);
        QCOMPARE(deepAnimation.workingBytes, 896.0L * mebibyte);
        QVERIFY(!RenderExportPolicy::staticImageFitsMemoryBudget(deep));
        QVERIFY(!RenderExportPolicy::animatedGifFitsMemoryBudget(deep));
        QVERIFY(!AnimationExportPolicy::fitsMemoryBudget(deep));
    }

    void evaluatesScaledExportBudgetAgainstTheDocumentHierarchy()
    {
        constexpr long double mebibyte = 1024.0L * 1024.0L;
        const Document deep = deepClippedExportDocument(QSize(4096, 4096));

        // The size-only estimate accepts the native export that the
        // document-aware one refuses; the export UI has to consult the
        // latter.
        QVERIFY(AnimationExportPolicy::fitsMemoryBudget(
            deep.size, deep.animationFrames));
        QVERIFY(!AnimationExportPolicy::fitsMemoryBudget(deep, deep.size));

        const RenderExportMemoryEstimate native =
            RenderExportPolicy::animatedGifAtSize(deep, deep.size);
        QVERIFY(native.valid);
        QCOMPARE(native.workingBytes,
            RenderExportPolicy::animatedGif(deep).workingBytes);

        const QSize half(2048, 2048);
        const RenderExportMemoryEstimate scaled =
            RenderExportPolicy::animatedGifAtSize(deep, half);
        QVERIFY(scaled.valid);
        QCOMPARE(scaled.workingBytes, 192.0L * mebibyte);
        QVERIFY(AnimationExportPolicy::fitsMemoryBudget(deep, half));

        QVERIFY(!RenderExportPolicy::animatedGifAtSize(deep, QSize()).valid);
    }

    void rejectsScaledExportWithNativeCompositeSectionsOverBudget()
    {
        constexpr long double mebibyte = 1024.0L * 1024.0L;
        Document document = Document::createDefault(QSize(4096, 4096));
        document.animationFrames = 8;
        document.layers.first().strokes.clear();
        for (int index = 0; index < 9; ++index)
        {
            Stroke boundary;
            boundary.mode = StrokeMode::CompositeBoundary;
            document.layers.first().strokes.append(boundary);
        }
        QVERIFY(!DocumentSerializer::toJson(document).isEmpty());

        const QSize outputSize(1024, 1024);
        const RenderExportMemoryEstimate estimate =
            RenderExportPolicy::animatedGifAtSize(document, outputSize);
        QVERIFY(estimate.valid);
        QCOMPARE(estimate.hierarchyTransientBytes, 40.0L * mebibyte);
        QCOMPARE(estimate.workingBytes, 772.0L * mebibyte);
        QVERIFY(!RenderExportPolicy::animatedGifFitsMemoryBudget(
            document, outputSize));
    }

    void accountsForLargerReframeEpochsAndSectionOverlap()
    {
        constexpr long double mebibyte = 1024.0L * 1024.0L;
        Document document = Document::createDefault(QSize(1024, 1024));
        document.animationFrames = 8;
        Layer &layer = document.layers.first();
        layer.initialCanvasSize = QSize(4096, 4096);
        layer.strokes.clear();

        Stroke firstBoundary;
        firstBoundary.mode = StrokeMode::CompositeBoundary;
        layer.strokes.append(firstBoundary);
        const auto appendImageReframe =
            [&](const QSize &source, const QSize &target)
        {
            Stroke reframe;
            reframe.mode = StrokeMode::Reframe;
            reframe.reframeOp = ReframeOp{
                ReframeMode::Image, SamplingMode::Nearest, source, target, {}};
            layer.strokes.append(reframe);
        };
        appendImageReframe(QSize(4096, 4096), QSize(2048, 2048));
        appendImageReframe(QSize(2048, 2048), QSize(4096, 4096));
        for (int index = 1; index < 9; ++index)
        {
            Stroke boundary;
            boundary.mode = StrokeMode::CompositeBoundary;
            layer.strokes.append(boundary);
        }
        appendImageReframe(QSize(4096, 4096), document.size);
        QVERIFY(!DocumentSerializer::toJson(document).isEmpty());

        const RenderExportMemoryEstimate staticEstimate =
            RenderExportPolicy::staticImage(document);
        const RenderExportMemoryEstimate nativeAnimation =
            RenderExportPolicy::animatedGif(document);
        const RenderExportMemoryEstimate scaledAnimation =
            RenderExportPolicy::animatedGifAtSize(document, QSize(256, 256));
        QVERIFY(staticEstimate.valid);
        QVERIFY(nativeAnimation.valid);
        QVERIFY(scaledAnimation.valid);
        QCOMPARE(staticEstimate.workingBytes, 712.0L * mebibyte);
        QCOMPARE(nativeAnimation.workingBytes, 736.0L * mebibyte);
        QCOMPARE(scaledAnimation.workingBytes, 708.25L * mebibyte);
        QVERIFY(!RenderExportPolicy::staticImageFitsMemoryBudget(document));
        QVERIFY(!RenderExportPolicy::animatedGifFitsMemoryBudget(document));
        QVERIFY(!RenderExportPolicy::animatedGifFitsMemoryBudget(
            document, QSize(256, 256)));
    }

    void preselectsAnExportScaleTheDocumentBudgetAccepts()
    {
        const Document deep = deepClippedExportDocument(QSize(4096, 4096));
        GifExportDialog dialog(deep, QStringLiteral("Export"));
        const QSize chosen = dialog.currentResult().outputSize;
        QVERIFY(chosen.isValid());
        QVERIFY2(AnimationExportPolicy::fitsMemoryBudget(deep, chosen),
            qPrintable(QStringLiteral("the dialog preselected %1x%2, which the "
                                      "document-aware budget rejects")
                    .arg(chosen.width())
                    .arg(chosen.height())));
    }

    void rejectsOverflowingExportMemoryGeometry()
    {
        const int maximum = std::numeric_limits<int>::max();
        Document document = Document::createDefault(QSize(maximum, maximum));
        document.animationFrames = 2;
        QVERIFY(!RenderExportPolicy::staticImage(document).valid);
        QVERIFY(!RenderExportPolicy::animatedGif(document).valid);
        QVERIFY(!RenderExportPolicy::staticImageFitsMemoryBudget(document));
        QVERIFY(!RenderExportPolicy::animatedGifFitsMemoryBudget(document));
    }

    void writesAnimatedGif()
    {
        QTemporaryDir directory;
        QVERIFY(directory.isValid());

        QVector<QImage> frames;
        const QVector<QRgb> colors{
            qRgb(239, 62, 54), qRgb(72, 187, 120), qRgb(66, 153, 225)};

        for (QRgb color : colors)
        {
            QImage frame(48, 32, QImage::Format_ARGB32);
            frame.fill(color);
            frames.append(frame);
        }

        const QString path =
            directory.filePath(QStringLiteral("animation.gif"));
        QString error;
        QVERIFY2(GifWriter::write(path, frames, 7, &error), qPrintable(error));

        QFile file(path);
        QVERIFY(file.open(QIODevice::ReadOnly));
        const QByteArray contents = file.readAll();
        QVERIFY(contents.startsWith("GIF89a"));
        QVERIFY(contents.endsWith(QByteArray(1, static_cast<char>(0x3b))));
        QVERIFY(contents.contains("NETSCAPE2.0"));

        QImageReader reader(path, QByteArrayLiteral("gif"));
        QVERIFY2(reader.canRead(), qPrintable(reader.errorString()));
        QCOMPARE(reader.size(), QSize(48, 32));

        if (reader.supportsAnimation())
        {
            QCOMPARE(reader.imageCount(), frames.size());
        }
    }

    void writesPerFrameDelays()
    {
        QTemporaryDir directory;
        QVERIFY(directory.isValid());

        QVector<QImage> frames;
        for (int value : {20, 100, 220})
        {
            QImage frame(12, 8, QImage::Format_ARGB32);
            frame.fill(QColor(value, 40, 80));
            frames.append(frame);
        }

        const QVector<int> delays{3, 7, 11};
        const QString path =
            directory.filePath(QStringLiteral("variable-delay.gif"));
        QString error;
        QVERIFY2(
            GifWriter::write(path, frames, delays, &error), qPrintable(error));

        QFile file(path);
        QVERIFY(file.open(QIODevice::ReadOnly));
        const QByteArray contents = file.readAll();
        QVector<int> encodedDelays;
        const QByteArray marker("\x21\xf9\x04", 3);
        qsizetype offset = 0;
        while ((offset = contents.indexOf(marker, offset)) >= 0)
        {
            QVERIFY(offset + 6 < contents.size());
            const int low = static_cast<quint8>(contents.at(offset + 4));
            const int high = static_cast<quint8>(contents.at(offset + 5));
            encodedDelays.append(low | (high << 8));
            offset += marker.size();
        }
        QCOMPARE(encodedDelays, delays);
    }

    void rejectsInvalidPerFrameDelays()
    {
        QTemporaryDir directory;
        QVERIFY(directory.isValid());
        QImage frame(8, 8, QImage::Format_ARGB32);
        frame.fill(Qt::black);
        const QVector<QImage> frames{frame, frame};
        QString error;

        QVERIFY(!GifWriter::write(
            directory.filePath(QStringLiteral("missing-delay.gif")),
            frames,
            QVector<int>{5},
            &error));
        QVERIFY(!error.isEmpty());

        QVERIFY(!GifWriter::write(
            directory.filePath(QStringLiteral("negative-delay.gif")),
            frames,
            QVector<int>{5, -1},
            &error));
        QVERIFY(!error.isEmpty());

        QVERIFY(!GifWriter::write(
            directory.filePath(QStringLiteral("uniform-negative-delay.gif")),
            frames,
            -1,
            &error));
        QVERIFY(!error.isEmpty());

        QVERIFY(!GifWriter::write(
            directory.filePath(QStringLiteral("large-delay.gif")),
            frames,
            QVector<int>{5, 65536},
            &error));
        QVERIFY(!error.isEmpty());
    }

    void rejectsMismatchedFrames()
    {
        QTemporaryDir directory;
        QVERIFY(directory.isValid());

        QVector<QImage> frames{QImage(8, 8, QImage::Format_ARGB32),
            QImage(9, 8, QImage::Format_ARGB32)};
        QString error;
        QVERIFY(
            !GifWriter::write(directory.filePath(QStringLiteral("invalid.gif")),
                frames,
                5,
                &error));
        QVERIFY(!error.isEmpty());
    }

    void rejectsExcessiveWorkingSet()
    {
        QTemporaryDir directory;
        QVERIFY(directory.isValid());

        QImage frame(2048, 2048, QImage::Format_ARGB32);
        QVERIFY(!frame.isNull());
        frame.fill(Qt::black);
        const QVector<QImage> frames(17, frame);
        QString error;
        QVERIFY(!GifWriter::write(
            directory.filePath(QStringLiteral("too-large.gif")),
            frames,
            4,
            &error));
        QVERIFY(!error.isEmpty());
    }

    void preservesBinaryTransparency()
    {
        QTemporaryDir directory;
        QVERIFY(directory.isValid());
        QImage frame(32, 24, QImage::Format_ARGB32);
        frame.fill(Qt::transparent);
        for (int y = 6; y < 18; ++y)
        {
            for (int x = 8; x < 24; ++x)
            {
                frame.setPixelColor(x, y, QColor(220, 40, 70));
            }
        }

        const QString path =
            directory.filePath(QStringLiteral("transparent.gif"));
        QString error;
        QVERIFY2(GifWriter::write(path, {frame, frame}, 5, &error),
            qPrintable(error));

        QImageReader reader(path, QByteArrayLiteral("gif"));
        const QImage decoded = reader.read();
        QVERIFY2(!decoded.isNull(), qPrintable(reader.errorString()));
        QCOMPARE(decoded.pixelColor(0, 0).alpha(), 0);
        QVERIFY(decoded.pixelColor(16, 12).alpha() > 0);
        QVERIFY(decoded.pixelColor(16, 12).red() > 180);
    }

    void cancelsBeforeCommittingOutput()
    {
        QTemporaryDir directory;
        QVERIFY(directory.isValid());
        QImage frame(256, 256, QImage::Format_ARGB32);
        frame.fill(QColor(40, 90, 210));
        const QString path = directory.filePath(QStringLiteral("canceled.gif"));
        int checks = 0;
        QString error;
        QVERIFY(!GifWriter::write(path,
            QVector<QImage>(12, frame),
            5,
            &error,
            [&checks]()
            {
                return ++checks >= 4;
            }));
        QVERIFY(!QFileInfo::exists(path));
    }

    void writesRenderedWobbleAnimation()
    {
        Document document = Document::createDefault(QSize(320, 200));
        document.animationFrames = 24;
        document.framesPerSecond = 24.0;
        document.wobbleAmount = 1.6;

        Stroke stroke;
        stroke.seed = 0x9f2b31c4a5678de0ULL;
        stroke.color = QColor(28, 30, 34);
        stroke.width = 7.0;
        for (int x = 24; x <= 296; x += 8)
        {
            const qreal y =
                100.0 + std::sin(static_cast<qreal>(x) * 0.045) * 48.0;
            stroke.points.append({QPointF(x, y), 1.0});
        }
        document.layers.first().strokes.append(stroke);

        QVector<QImage> frames;
        frames.reserve(document.animationFrames);
        for (int frame = 0; frame < document.animationFrames; ++frame)
        {
            frames.append(RenderEngine::render(document, frame));
        }

        QTemporaryDir directory;
        QVERIFY(directory.isValid());
        const QString temporaryPath =
            directory.filePath(QStringLiteral("rendered.gif"));
        QString error;
        QVERIFY2(GifWriter::write(temporaryPath, frames, 4, &error),
            qPrintable(error));

        QImageReader reader(temporaryPath, QByteArrayLiteral("gif"));
        QVERIFY2(reader.canRead(), qPrintable(reader.errorString()));
        QCOMPARE(reader.size(), document.size);
        QCOMPARE(reader.imageCount(), document.animationFrames);

        const QString outputPath = qEnvironmentVariable("UGURUGU_TEST_GIF");
        if (!outputPath.isEmpty())
        {
            QVERIFY2(GifWriter::write(outputPath, frames, 4, &error),
                qPrintable(error));
            QVERIFY(QFileInfo(outputPath).size() > 0);
        }
    }
};

int runGifWriterTests(int argc, char **argv)
{
    GifWriterTests tests;
    return QTest::qExec(&tests, argc, argv);
}

}

#include "GifWriterTests.moc"

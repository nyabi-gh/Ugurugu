// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "io/WebPWriter.hpp"

#include <QFile>
#include <QFileInfo>
#include <QTemporaryDir>
#include <QtTest>

#include <memory>
#include <webp/demux.h>

namespace ugurugu
{

namespace
{

struct DecoderDeleter
{
    void operator()(WebPAnimDecoder *decoder) const
    {
        WebPAnimDecoderDelete(decoder);
    }
};

using Decoder = std::unique_ptr<WebPAnimDecoder, DecoderDeleter>;

}

class WebPWriterTests final : public QObject
{
    Q_OBJECT

private slots:
    void writesLoopingAnimationWithAlphaAndFrameTiming()
    {
        QTemporaryDir directory;
        QVERIFY(directory.isValid());
        QVector<QImage> frames;
        const QVector<QColor> colors{QColor(240, 40, 70, 64),
            QColor(30, 190, 110, 160),
            QColor(60, 100, 230, 255)};
        for (const QColor &color : colors)
        {
            QImage frame(12, 8, QImage::Format_RGBA8888);
            frame.fill(color);
            frames.append(frame);
        }
        const QVector<int> durations{34, 33, 34};
        const QString path =
            directory.filePath(QStringLiteral("animation.webp"));
        QString error;
        QVERIFY2(WebPWriter::write(path, frames, durations, &error),
            qPrintable(error));

        QFile file(path);
        QVERIFY(file.open(QIODevice::ReadOnly));
        const QByteArray bytes = file.readAll();
        QVERIFY(bytes.startsWith("RIFF"));
        QCOMPARE(bytes.mid(8, 4), QByteArrayLiteral("WEBP"));

        const WebPData data{
            reinterpret_cast<const uint8_t *>(bytes.constData()),
            static_cast<size_t>(bytes.size())};
        WebPAnimDecoderOptions options;
        QVERIFY(WebPAnimDecoderOptionsInit(&options));
        options.color_mode = MODE_RGBA;
        Decoder decoder(WebPAnimDecoderNew(&data, &options));
        QVERIFY(decoder);
        WebPAnimInfo info;
        QVERIFY(WebPAnimDecoderGetInfo(decoder.get(), &info));
        QCOMPARE(info.canvas_width, 12U);
        QCOMPARE(info.canvas_height, 8U);
        QCOMPARE(info.frame_count, 3U);
        QCOMPARE(info.loop_count, 0U);

        QVector<int> timestamps;
        uint8_t *pixels = nullptr;
        int timestamp = 0;
        int frameIndex = 0;
        while (WebPAnimDecoderGetNext(decoder.get(), &pixels, &timestamp))
        {
            QVERIFY(pixels);
            timestamps.append(timestamp);
            const QColor decoded(pixels[0], pixels[1], pixels[2], pixels[3]);
            QCOMPARE(decoded, colors[frameIndex]);
            ++frameIndex;
        }
        QCOMPARE(frameIndex, frames.size());
        QCOMPARE(timestamps, QVector<int>({34, 67, 101}));
    }

    void rejectsInvalidFramesAndTimings()
    {
        QTemporaryDir directory;
        QVERIFY(directory.isValid());
        QImage frame(8, 8, QImage::Format_RGBA8888);
        frame.fill(Qt::black);
        QString error;
        QVERIFY(!WebPWriter::write(
            directory.filePath(QStringLiteral("missing.webp")),
            {frame, frame},
            {20},
            &error));
        QVERIFY(!error.isEmpty());
        QVERIFY(!WebPWriter::write(
            directory.filePath(QStringLiteral("mismatch.webp")),
            {frame, QImage(9, 8, QImage::Format_RGBA8888)},
            {20, 20},
            &error));
        QVERIFY(!error.isEmpty());
        QVERIFY(
            !WebPWriter::write(directory.filePath(QStringLiteral("zero.webp")),
                {frame},
                {0},
                &error));
        QVERIFY(!error.isEmpty());
    }

    void cancelsBeforeCommittingOutput()
    {
        QTemporaryDir directory;
        QVERIFY(directory.isValid());
        QImage frame(128, 128, QImage::Format_RGBA8888);
        frame.fill(QColor(40, 90, 210, 120));
        const QString path =
            directory.filePath(QStringLiteral("canceled.webp"));
        int checks = 0;
        QString error;
        QVERIFY(!WebPWriter::write(path,
            QVector<QImage>(10, frame),
            QVector<int>(10, 40),
            &error,
            [&checks]()
            {
                return ++checks >= 4;
            }));
        QVERIFY(!QFileInfo::exists(path));
    }
};

int runWebPWriterTests(int argc, char **argv)
{
    WebPWriterTests tests;
    return QTest::qExec(&tests, argc, argv);
}

}

#include "WebPWriterTests.moc"

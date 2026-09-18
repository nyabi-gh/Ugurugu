// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "io/serializer/BoundedCompression.hpp"
#include "io/serializer/RasterAssetTable.hpp"
#include "support/DocumentTestSuites.hpp"

#include <QColorSpace>
#include <QtEndian>
#include <QtTest>

#include <limits>

namespace ugurugu
{

using namespace serializer_detail;

class RasterAssetTableTests final : public QObject
{
    Q_OBJECT

private slots:
    void boundsDecodedOutputAndRequiresTheWholeStream()
    {
        const QByteArray original("bounded payload");
        const QByteArray compressed = qCompress(original, 6);
        const std::optional<QByteArray> restored =
            uncompressQtPayload(compressed, original.size());
        QVERIFY(restored.has_value());
        QCOMPARE(*restored, original);
        QVERIFY(!uncompressQtPayload(compressed, original.size() + 1).has_value());

        QByteArray oversized = qCompress(QByteArray(4096, 'x'), 6);
        qToBigEndian<quint32>(16, reinterpret_cast<uchar *>(oversized.data()));
        QVERIFY(!uncompressQtPayload(oversized, 16).has_value());

        QByteArray trailing = compressed;
        trailing.append("trailing");
        QVERIFY(!uncompressQtPayload(trailing, original.size()).has_value());

        QByteArray truncated = compressed;
        truncated.chop(1);
        QVERIFY(!uncompressQtPayload(truncated, original.size()).has_value());
    }

    void canonicalizesDeduplicatesAndDecodes()
    {
        QImage source(QSize(8, 6), QImage::Format_ARGB32);
        source.setColorSpace(QColorSpace::SRgb);
        for (int y = 0; y < source.height(); ++y)
        {
            for (int x = 0; x < source.width(); ++x)
            {
                source.setPixelColor(
                    x, y, QColor(20 + x * 9, 30 + y * 11, 80 + x + y, 255));
            }
        }

        RasterAssetTable table;
        const RasterAssetRegistrationResult registered =
            table.registerImage(source);
        QCOMPARE(registered.status, RasterAssetRegistrationStatus::Registered);
        QVERIFY(!registered.id.isEmpty());
        QCOMPARE(table.entries().size(), 1);
        QCOMPARE(table.decodedBytes(), 8ULL * 6ULL * 4ULL);
        QVERIFY(table.payloadBytes() > 0);

        const RasterAssetRegistrationResult reused =
            table.registerImage(source.copy());
        QCOMPARE(reused.status, RasterAssetRegistrationStatus::Reused);
        QCOMPARE(reused.id, registered.id);
        QCOMPARE(table.entries().size(), 1);

        const std::optional<QImage> decoded = table.decode(registered.id);
        QVERIFY(decoded.has_value());
        QCOMPARE(decoded->format(), QImage::Format_RGBA8888);
        QCOMPARE(decoded->colorSpace(), QColorSpace(QColorSpace::SRgb));
        QCOMPARE(*decoded, canonicalRasterImage(source));
    }

    void reloadsVerifiedPayloads()
    {
        QImage source(QSize(5, 7), QImage::Format_RGBA8888);
        source.fill(QColor(17, 93, 211, 127));
        RasterAssetTable original;
        const RasterAssetRegistrationResult registered =
            original.registerImage(source);
        QCOMPARE(registered.status, RasterAssetRegistrationStatus::Registered);
        const RasterAssetEntry entry = original.entries().first();

        RasterAssetTable loaded;
        const RasterAssetRegistrationResult loadedResult =
            loaded.registerPayload(entry.id, entry.size, entry.compressedRgba);
        QCOMPARE(
            loadedResult.status, RasterAssetRegistrationStatus::Registered);
        QCOMPARE(loaded.decode(entry.id), original.decode(entry.id));

        QByteArray corrupted = entry.compressedRgba;
        corrupted[3] = static_cast<char>(corrupted[3] ^ 0x01);
        RasterAssetTable rejected;
        QCOMPARE(
            rejected.registerPayload(entry.id, entry.size, corrupted).status,
            RasterAssetRegistrationStatus::Invalid);
        QVERIFY(rejected.entries().isEmpty());
    }

    void rejectsBudgetsWithoutMutatingTheTable()
    {
        QImage source(QSize(8, 8), QImage::Format_RGBA8888);
        source.fill(Qt::red);

        RasterAssetTable decodedLimited(8ULL * 8ULL * 4ULL - 1ULL,
            DocumentLimits::maximumDistinctRasterPayloadBytes);
        QCOMPARE(decodedLimited.registerImage(source).status,
            RasterAssetRegistrationStatus::DecodedBudget);
        QVERIFY(decodedLimited.entries().isEmpty());

        RasterAssetTable payloadLimited(
            DocumentLimits::maximumDistinctRasterDecodedBytes, 1);
        QCOMPARE(payloadLimited.registerImage(source).status,
            RasterAssetRegistrationStatus::PayloadBudget);
        QVERIFY(payloadLimited.entries().isEmpty());
    }

    void rejectsInvalidAndOversizedDimensions()
    {
        QVERIFY(!rasterDecodedByteCount(QSize()).has_value());
        QVERIFY(!rasterDecodedByteCount(QSize(std::numeric_limits<int>::max(),
                                            std::numeric_limits<int>::max()))
                .has_value());
        QCOMPARE(rasterDecodedByteCount(QSize(4096, 4096)),
            std::optional<quint64>(64ULL * 1024ULL * 1024ULL));
        QVERIFY(!rasterDecodedByteCount(QSize(4097, 4096)).has_value());
    }
};

int runRasterAssetTableTests(int argc, char **argv)
{
    RasterAssetTableTests tests;
    return QTest::qExec(&tests, argc, argv);
}

}

#include "RasterAssetTableTests.moc"

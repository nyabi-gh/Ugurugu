// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "io/DocumentSerializer.hpp"
#include "render/RenderEngine.hpp"
#include "support/RenderTestSuites.hpp"

#include <QByteArrayView>
#include <QCryptographicHash>
#include <QFile>
#include <QJsonDocument>
#include <QJsonObject>
#include <QtTest>

namespace ugurugu
{
namespace
{

QString fixturePath(const QString &fileName)
{
    return QStringLiteral(UGURUGU_SOURCE_DIR "/tests/fixtures/legacy-render/")
           + fileName;
}

QByteArray renderHash(const QImage &source)
{
    const QImage image = source.convertToFormat(QImage::Format_RGBA8888);
    if (image.isNull())
    {
        return {};
    }
    QCryptographicHash hash(QCryptographicHash::Sha256);
    // The golden wire format is version tag, decimal size, then tightly packed
    // top-to-bottom RGBA8888 rows so platform row padding and endianness do not
    // affect the compatibility digest.
    hash.addData(QByteArrayLiteral("WWP_LEGACY_RENDER_RGBA8_V1\n"));
    hash.addData(QByteArray::number(image.width()));
    hash.addData(QByteArrayLiteral("x"));
    hash.addData(QByteArray::number(image.height()));
    hash.addData(QByteArrayLiteral("\n"));
    const qsizetype rowBytes = static_cast<qsizetype>(image.width()) * 4;
    for (int y = 0; y < image.height(); ++y)
    {
        hash.addData(QByteArrayView(
            reinterpret_cast<const char *>(image.constScanLine(y)), rowBytes));
    }
    return hash.result().toHex();
}

}

class LegacyRenderGoldenTests final : public QObject
{
    Q_OBJECT

private slots:
    void fixturesAreV100SchemaNine()
    {
        const QStringList fileNames = {
            QStringLiteral("animated-paint-erase.wagle"),
            QStringLiteral("fill-hierarchy.wagle"),
            QStringLiteral("ordered-operations.wagle")};
        for (const QString &fileName : fileNames)
        {
            QFile file(fixturePath(fileName));
            QVERIFY2(file.open(QIODevice::ReadOnly), qPrintable(fileName));
            QJsonParseError parseError;
            const QJsonObject root =
                QJsonDocument::fromJson(file.readAll(), &parseError).object();
            QCOMPARE(parseError.error, QJsonParseError::NoError);
            QCOMPARE(root.value(QStringLiteral("schemaVersion")).toInt(), 9);
            QCOMPARE(root.value(QStringLiteral("algorithmVersion")).toInt(), 2);
        }
    }

    void matchesV100Pixels_data()
    {
        // These fixtures and hashes pin v1.0.0 commit 9117cc4 with Qt 6.11.1.
        // Classic motion used the platform math library, so animated pixels
        // differ between the shipped macOS and Windows builds. Keep each
        // platform pinned to its own v1 output; static fixtures are shared.
        QTest::addColumn<QString>("fileName");
        QTest::addColumn<int>("frame");
        QTest::addColumn<QByteArray>("expectedHash");

#ifdef Q_OS_WIN
        const QByteArray animatedFrame0 = QByteArrayLiteral(
            "a880bb01875276e0b373b34d104f6dd24480c34d98505e23e5afb87d404167ab");
        const QByteArray animatedFrame1 = QByteArrayLiteral(
            "19aeda40525e865fd7b3d2eb8656c23117cb779d5edf513356b0c7ac5e3f46c3");
        const QByteArray animatedFrame6 = QByteArrayLiteral(
            "6d8b84ef5ad1d2953c5259e0b7751fc2aa3eba3a5f8a46fbdf69ba0bfb280aed");
        const QByteArray animatedFrame11 = QByteArrayLiteral(
            "fdb8a8ba30fa8ba7a7e4ff302e1accdd5460bfba09785ec9c09504b167947466");
#else
        const QByteArray animatedFrame0 = QByteArrayLiteral(
            "6a089d1cc7bf17863051b39db06df5ab2c1eb6c2ee5a29a6de923046e3080065");
        const QByteArray animatedFrame1 = QByteArrayLiteral(
            "508cb433b5517a0f3cfedf1446a6761fa842efbfa7505a00d7dff95ddfc48c15");
        const QByteArray animatedFrame6 = QByteArrayLiteral(
            "fa53aece48211e3e85b5dfb39b85fae8df214888a2d84ed2c7373a3ed066bf59");
        const QByteArray animatedFrame11 = QByteArrayLiteral(
            "f1524157872b85482731cc57e72537769e80ef8f06799ce10b5e5d0a2a237175");
#endif
        QTest::newRow("animated-frame-0")
            << QStringLiteral("animated-paint-erase.wagle") << 0
            << animatedFrame0;
        QTest::newRow("animated-frame-1")
            << QStringLiteral("animated-paint-erase.wagle") << 1
            << animatedFrame1;
        QTest::newRow("animated-frame-6")
            << QStringLiteral("animated-paint-erase.wagle") << 6
            << animatedFrame6;
        QTest::newRow("animated-frame-11")
            << QStringLiteral("animated-paint-erase.wagle") << 11
            << animatedFrame11;
        QTest::newRow("fill-hierarchy-frame-0")
            << QStringLiteral("fill-hierarchy.wagle") << 0
            << QByteArrayLiteral("04cb57d78a702ae774db20c02cab5dff3a0a2aa1db866"
                                 "3e2495d6b9012ef1097");
        QTest::newRow("fill-hierarchy-frame-4")
            << QStringLiteral("fill-hierarchy.wagle") << 4
            << QByteArrayLiteral("033c571d57cc2f1cc1d0a09adc539aac598d006667e4b"
                                 "25f08ef9f645af0c8fe");
        QTest::newRow("fill-hierarchy-frame-9")
            << QStringLiteral("fill-hierarchy.wagle") << 9
            << QByteArrayLiteral("2073f05d32324079361b67adcc210eb6761d30a65dbf1"
                                 "1f3dea744532b3fceef");
        QTest::newRow("ordered-operations-frame-0")
            << QStringLiteral("ordered-operations.wagle") << 0
            << QByteArrayLiteral("7f9f8fd0620525a79947c2d4385e84baa937266ce005c"
                                 "74c3d014395609d24c4");
        QTest::newRow("ordered-operations-frame-3")
            << QStringLiteral("ordered-operations.wagle") << 3
            << QByteArrayLiteral("01d156ea9416ba45a3458dda489b98d1a325e8192c9d4"
                                 "c1c1beb7748efcdb555");
        QTest::newRow("ordered-operations-frame-7")
            << QStringLiteral("ordered-operations.wagle") << 7
            << QByteArrayLiteral("6adfa164fadabf4fd1ac5b9ddf40ccc1acf63335b161d"
                                 "1c44b22bd59cbc44748");
    }

    void matchesV100Pixels()
    {
        QFETCH(QString, fileName);
        QFETCH(int, frame);
        QFETCH(QByteArray, expectedHash);

        QString error;
        const std::optional<Document> document =
            DocumentSerializer::load(fixturePath(fileName), &error);
        QVERIFY2(document.has_value(), qPrintable(error));
        const QImage rendered = RenderEngine::render(*document, frame);
        QVERIFY(!rendered.isNull());
        QCOMPARE(renderHash(rendered), expectedHash);
    }
};

int runLegacyRenderGoldenTests(int argc, char **argv)
{
    LegacyRenderGoldenTests tests;
    return QTest::qExec(&tests, argc, argv);
}

}

#include "LegacyRenderGoldenTests.moc"

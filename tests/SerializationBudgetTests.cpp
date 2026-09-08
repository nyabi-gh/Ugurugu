// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "support/DocumentTestHelpers.hpp"
#include "support/DocumentTestSuites.hpp"

#include <QBuffer>

namespace ugurugu
{

class DocumentSerializerTestAccess final
{
public:
    static std::optional<Document> load(
        QIODevice &device, qint64 maximumBytes, QString *error = nullptr)
    {
        return DocumentSerializer::loadFromDevice(device, maximumBytes, error);
    }
};

class StaleSizeBuffer final : public QBuffer
{
public:
    qint64 size() const override
    {
        return 0;
    }
};

class SerializationBudgetTests final : public QObject
{
    Q_OBJECT

private slots:
    void loadsBundledExample()
    {
        QString error;
        const QString path = QStringLiteral(UGURUGU_SOURCE_DIR)
                             + QStringLiteral("/examples/Wave.ugu");
        const std::optional<Document> document =
            DocumentSerializer::load(path, &error);
        QVERIFY2(document.has_value(), qPrintable(error));
        QCOMPARE(document->size, QSize(640, 400));
        QCOMPARE(document->animationFrames, 30);
        QCOMPARE(document->framesPerSecond, 25.0);
        QCOMPARE(document->layers.size(), 1);
        QCOMPARE(document->layers.first().strokes.size(), 2);
    }

    void savesAndLoadsProjectFile()
    {
        QTemporaryDir directory;
        QVERIFY(directory.isValid());
        const QString path = directory.filePath(QStringLiteral("project.ugu"));
        Document source = Document::createDefault(QSize(222, 111));
        source.wobbleAmount = 3.4;

        QString error;
        QVERIFY2(
            DocumentSerializer::save(path, source, &error), qPrintable(error));
        const std::optional<Document> loaded =
            DocumentSerializer::load(path, &error);
        QVERIFY2(loaded.has_value(), qPrintable(error));
        QCOMPARE(loaded->size, source.size);
        QCOMPARE(loaded->wobbleAmount, source.wobbleAmount);
        QCOMPARE(loaded->activeLayerId, source.activeLayerId);
    }

    void preparesExactSerializationAtTheByteLimit()
    {
        Document source = Document::createDefault(QSize(65, 17));
        source.layers.first().name = QStringLiteral("레이어 \"A\"\n日本語");

        QImage clipMask(source.size, QImage::Format_Grayscale8);
        clipMask.fill(0);
        for (int y = 2; y < 15; ++y)
        {
            std::fill_n(clipMask.scanLine(y) + 4, 41, static_cast<uchar>(255));
        }
        Stroke paint;
        paint.points = {
            {QPointF(4.25, 3.5), 0.5}, {QPointF(44.75, 13.25), 1.0}};
        paint.clipMask = clipMask;
        source.layers.first().strokes.append(paint);

        QImage selection(source.size, QImage::Format_Grayscale8);
        selection.fill(0);
        for (int y = 5; y < 12; ++y)
        {
            std::fill_n(
                selection.scanLine(y) + 10, 23, static_cast<uchar>(255));
        }
        const std::optional<PixelSelectionOp> operation = makePixelSelectionOp(
            selection, QTransform::fromTranslate(1.5, -0.5), true, true);
        QVERIFY(operation.has_value());
        Stroke selectionStroke;
        selectionStroke.mode = StrokeMode::PixelSelection;
        selectionStroke.points.clear();
        selectionStroke.pixelSelectionOp = *operation;
        source.layers.first().strokes.append(selectionStroke);

        DocumentSerializer::SerializationCache cache;
        QString error;
        const auto prepared =
            DocumentSerializer::prepare(source, cache, &error);
        QVERIFY2(prepared.has_value(), qPrintable(error));
        const QByteArray preparedJson =
            DocumentSerializer::toJson(*prepared, cache);
        QVERIFY(!preparedJson.isEmpty());
        QCOMPARE(
            prepared->compactSize(), static_cast<qint64>(preparedJson.size()));
        QCOMPARE(DocumentSerializer::toJson(source), preparedJson);

        DocumentSerializer::SerializationCache exactCache;
        const auto exact = DocumentSerializer::prepare(
            source, exactCache, nullptr, preparedJson.size(), &error);
        QVERIFY2(exact.has_value(), qPrintable(error));
        QCOMPARE(exact->compactSize(), prepared->compactSize());

        DocumentSerializer::SerializationCache smallCache;
        error.clear();
        const auto tooSmall = DocumentSerializer::prepare(
            source, smallCache, nullptr, preparedJson.size() - 1, &error);
        QVERIFY(!tooSmall.has_value());
        QVERIFY(!error.isEmpty());

        error.clear();
        const auto aboveHardLimit = DocumentSerializer::prepare(source,
            smallCache,
            nullptr,
            DocumentLimits::maximumProjectBytes + 1,
            &error);
        QVERIFY(!aboveHardLimit.has_value());
        QVERIFY(!error.isEmpty());
    }

    void preservesExactSerializationWithAdditionalRootFields()
    {
        Document source = Document::createDefault(QSize(79, 53));
        source.layers.first().name = QStringLiteral("복구 レイヤー");

        DocumentSerializer::SerializationCache cache;
        const auto prepared = DocumentSerializer::prepare(source, cache);
        QVERIFY(prepared.has_value());
        const QByteArray base = DocumentSerializer::toJson(*prepared, cache);
        QVERIFY(!base.isEmpty());

        QJsonObject metadata;
        metadata.insert(QStringLiteral("formatVersion"), 1);
        metadata.insert(
            QStringLiteral("revision"), QStringLiteral("9007199254740993"));
        QJsonObject additionalRootFields;
        additionalRootFields.insert(
            QStringLiteral("uguruguRecovery"), metadata);

        QJsonObject expectedRoot = QJsonDocument::fromJson(base).object();
        expectedRoot.insert(QStringLiteral("uguruguRecovery"), metadata);
        const QByteArray expected =
            QJsonDocument(expectedRoot).toJson(QJsonDocument::Compact);
        const QByteArray extended =
            DocumentSerializer::toJson(*prepared, cache, additionalRootFields);
        QCOMPARE(extended, expected);

        QJsonObject collidingRootFields;
        collidingRootFields.insert(QStringLiteral("layers"), QJsonArray());
        QVERIFY(
            DocumentSerializer::toJson(*prepared, cache, collidingRootFields)
                .isEmpty());
    }

    void freezesPreparedDocumentsAgainstRawCowAliases()
    {
        Document source = Document::createDefault(QSize(8, 8));
        source.layers.first().name = QStringLiteral("Layer");

        Stroke paint;
        paint.width = 5.0;
        paint.points = {{QPointF(1.0, 1.0), 0.5}, {QPointF(6.0, 6.0), 1.0}};
        paint.clipMask = QImage(source.size, QImage::Format_Grayscale8);
        paint.clipMask.fill(0);
        paint.clipMask.scanLine(0)[0] = 0xff;

        Stroke selection;
        selection.mode = StrokeMode::PixelSelection;
        selection.points.clear();
        PixelSelectionOp operation;
        operation.canvasSize = source.size;
        operation.sourceBounds = QRect(0, 0, 8, 1);
        operation.packedMask = QByteArray(1, static_cast<char>(0x80));
        operation.sampling = SamplingMode::Nearest;
        selection.pixelSelectionOp = operation;
        source.layers.first().strokes = {paint, selection};

        QChar *rawName = source.layers.first().name.data();
        Stroke *rawStrokes = source.layers.first().strokes.data();
        StrokePoint *rawPoints = rawStrokes[0].points.data();
        uchar *rawImage = rawStrokes[0].clipMask.bits();
        char *rawPacked = rawStrokes[1].pixelSelectionOp->packedMask.data();

        DocumentSerializer::SerializationCache cache;
        const auto prepared = DocumentSerializer::prepare(source, cache);
        QVERIFY(prepared.has_value());
        const QByteArray originalJson =
            DocumentSerializer::toJson(*prepared, cache);
        QVERIFY(!originalJson.isEmpty());

        rawName[0] = QChar(u'X');
        rawStrokes[0].width = 7.0;
        rawPoints[0].pressure = 0.75;
        rawImage[0] = 0;
        rawPacked[0] = static_cast<char>(0x40);

        const Layer &frozenLayer = prepared->document().layers.first();
        QCOMPARE(frozenLayer.name, QStringLiteral("Layer"));
        QCOMPARE(frozenLayer.strokes[0].width, 5.0);
        QCOMPARE(frozenLayer.strokes[0].points[0].pressure, 0.5);
        QCOMPARE(frozenLayer.strokes[0].clipMask.constScanLine(0)[0],
            static_cast<uchar>(0xff));
        QCOMPARE(frozenLayer.strokes[1].pixelSelectionOp->packedMask[0],
            static_cast<char>(0x80));

        DocumentSerializer::SerializationCache freshCache;
        const QByteArray frozenJson =
            DocumentSerializer::toJson(*prepared, freshCache);
        QCOMPARE(frozenJson, originalJson);
        QString error;
        QVERIFY2(DocumentSerializer::fromJson(frozenJson, &error).has_value(),
            qPrintable(error));
    }

    void reusesPreparedMetadataTopologyExactly()
    {
        Document source = Document::createDefault(QSize(48, 32));
        Stroke firstStroke;
        firstStroke.points = {{QPointF(4.0, 5.0), 1.0}};
        source.layers.first().strokes.append(firstStroke);

        Layer secondLayer;
        secondLayer.name = QStringLiteral("Second");
        secondLayer.initialCanvasSize = source.size;
        Stroke secondStroke;
        secondStroke.points = {{QPointF(20.0, 12.0), 0.75}};
        secondLayer.strokes.append(secondStroke);
        source.layers.append(secondLayer);

        DocumentSerializer::SerializationCache cache;
        const auto base = DocumentSerializer::prepare(source, cache);
        QVERIFY(base.has_value());

        Document changed = base->document();
        changed.layers.swapItemsAt(0, 1);
        changed.layers.first().name = QStringLiteral("이름 \"변경\"\n日本語");
        changed.layers.first().visible = false;
        changed.layers.first().opacity = 0.625;
        changed.layers.first().blendMode = LayerBlendMode::Overlay;
        changed.background = QColor(QStringLiteral("#123456"));
        changed.animationFrames = 22;
        changed.framesPerSecond = 18.5;
        changed.wobbleAmount = 2.25;
        changed.activeLayerId = changed.layers.first().id;
        const QString expectedName = changed.layers.first().name;
        QChar *rawChangedName = changed.layers.first().name.data();

        cache.resetStats();
        QString error;
        const auto prepared = DocumentSerializer::prepare(changed,
            cache,
            &*base,
            DocumentLimits::maximumProjectBytes,
            &error);
        QVERIFY2(prepared.has_value(), qPrintable(error));
        const auto stats = cache.stats();
        QCOMPARE(stats.clipMaskContentHashes, 0ULL);
        QCOMPARE(stats.clipMaskCompressions, 0ULL);
        QCOMPARE(stats.binaryMaskContentHashes, 0ULL);
        QCOMPARE(stats.binaryMaskCompressions, 0ULL);
        QCOMPARE(stats.strokeSerializations, 0ULL);
        QCOMPARE(prepared->document().layers.first().id, secondLayer.id);
        QCOMPARE(prepared->document().layers.first().name, expectedName);
        QCOMPARE(prepared->document().activeLayerId, secondLayer.id);
        for (const Layer &baseLayer : base->document().layers)
        {
            const Layer *preparedLayer =
                prepared->document().layer(baseLayer.id);
            QVERIFY(preparedLayer);
            QVERIFY(preparedLayer->strokes.constData()
                    == baseLayer.strokes.constData());
        }

        const QByteArray json = DocumentSerializer::toJson(*prepared, cache);
        QVERIFY(!json.isEmpty());
        QCOMPARE(prepared->compactSize(), static_cast<qint64>(json.size()));
        const std::optional<Document> decoded =
            DocumentSerializer::fromJson(json, &error);
        QVERIFY2(decoded.has_value(), qPrintable(error));
        QCOMPARE(decoded->layers.first().name, expectedName);
        QCOMPARE(decoded->layers.first().blendMode, LayerBlendMode::Overlay);
        QCOMPARE(decoded->background, changed.background);
        QCOMPARE(decoded->animationFrames, 22);
        QCOMPARE(decoded->framesPerSecond, 18.5);
        QCOMPARE(decoded->wobbleAmount, 2.25);

        rawChangedName[0] = QChar(u'X');
        QCOMPARE(prepared->document().layers.first().name, expectedName);

        const auto exact = DocumentSerializer::prepare(prepared->document(),
            cache,
            &*base,
            prepared->compactSize(),
            &error);
        QVERIFY2(exact.has_value(), qPrintable(error));
        const auto tooSmall = DocumentSerializer::prepare(prepared->document(),
            cache,
            &*base,
            prepared->compactSize() - 1,
            &error);
        QVERIFY(!tooSmall.has_value());

        Document invalid = base->document();
        invalid.layers.first().opacity =
            std::numeric_limits<qreal>::quiet_NaN();
        QVERIFY(!DocumentSerializer::prepare(std::move(invalid),
            cache,
            &*base,
            DocumentLimits::maximumProjectBytes)
                .has_value());

        invalid = base->document();
        invalid.layers.first().blendMode = static_cast<LayerBlendMode>(100);
        QVERIFY(!DocumentSerializer::prepare(std::move(invalid),
            cache,
            &*base,
            DocumentLimits::maximumProjectBytes)
                .has_value());
    }

    void doesNotReuseDetachedWritableStrokeBacking()
    {
        Document source = Document::createDefault(QSize(32, 32));
        Stroke stroke;
        stroke.width = 5.0;
        stroke.points = {{QPointF(8.0, 9.0), 0.5}};
        source.layers.first().strokes.append(stroke);

        DocumentSerializer::SerializationCache cache;
        const auto base = DocumentSerializer::prepare(source, cache);
        QVERIFY(base.has_value());
        const Stroke *baseStrokes =
            base->document().layers.first().strokes.constData();

        Document changed = base->document();
        Stroke *writableStrokes = changed.layers.first().strokes.data();
        QVERIFY(writableStrokes != baseStrokes);
        writableStrokes[0].width = 8.0;

        cache.resetStats();
        const auto prepared = DocumentSerializer::prepare(
            changed, cache, &*base, DocumentLimits::maximumProjectBytes);
        QVERIFY(prepared.has_value());
        QCOMPARE(cache.stats().strokeSerializations, 1ULL);
        QCOMPARE(base->document().layers.first().strokes.first().width, 5.0);
        QCOMPARE(
            prepared->document().layers.first().strokes.first().width, 8.0);
        writableStrokes[0].width = 11.0;
        QCOMPARE(
            prepared->document().layers.first().strokes.first().width, 8.0);

        const QByteArray json = DocumentSerializer::toJson(*prepared, cache);
        QVERIFY(!json.isEmpty());
        QCOMPARE(prepared->compactSize(), static_cast<qint64>(json.size()));
    }

    void reusesPreparedMetadataWithoutLayers()
    {
        Document source;
        source.size = QSize(64, 48);
        source.layers.clear();
        source.activeLayerId = QUuid();

        DocumentSerializer::SerializationCache cache;
        const auto base = DocumentSerializer::prepare(source, cache);
        QVERIFY(base.has_value());
        Document changed = base->document();
        changed.background = Qt::transparent;
        changed.wobbleAmount = 3.5;

        cache.resetStats();
        const auto prepared = DocumentSerializer::prepare(std::move(changed),
            cache,
            &*base,
            DocumentLimits::maximumProjectBytes);
        QVERIFY(prepared.has_value());
        QVERIFY(prepared->document().layers.isEmpty());
        QVERIFY(prepared->document().activeLayerId.isNull());
        QCOMPARE(cache.stats().strokeSerializations, 0ULL);
        const QByteArray json = DocumentSerializer::toJson(*prepared, cache);
        QVERIFY(!json.isEmpty());
        QCOMPARE(prepared->compactSize(), static_cast<qint64>(json.size()));
    }

    void appendsPreparedStrokeIncrementallyExactly()
    {
        Document source = documentWithStrokeCount(512);
        const QUuid layerId = source.activeLayerId;
        DocumentSerializer::SerializationCache cache;
        const auto base = DocumentSerializer::prepare(source, cache);
        QVERIFY(base.has_value());
        const QByteArray baseJson = DocumentSerializer::toJson(*base, cache);
        QVERIFY(!baseJson.isEmpty());
        const Stroke *baseStrokeBacking =
            base->document().layers.first().strokes.constData();
        const StrokePoint *basePointBacking =
            base->document().layers.first().strokes.first().points.constData();

        Stroke stroke;
        stroke.points = {
            {QPointF(8.0, 9.0), 0.25}, {QPointF(48.0, 40.0), 0.75}};
        stroke.clipMask = QImage(source.size, QImage::Format_Grayscale8);
        stroke.clipMask.fill(0);
        for (int y = 8; y < 40; ++y)
        {
            std::fill_n(stroke.clipMask.scanLine(y) + 8, 32, 255);
        }
        const Stroke expectedStroke = stroke;
        StrokePoint *rawPoints = stroke.points.data();
        uchar *rawMask = stroke.clipMask.bits();

        cache.resetStats();
        DocumentSerializer::AppendStrokeResult appended =
            DocumentSerializer::appendStroke(*base,
                layerId,
                stroke,
                cache,
                DocumentLimits::maximumProjectBytes);
        QCOMPARE(
            appended.status, DocumentSerializer::AppendStrokeStatus::Appended);
        QVERIFY(appended.prepared.isValid());
        const auto stats = cache.stats();
        QCOMPARE(stats.incrementalStrokeAppends, 1ULL);
        QCOMPARE(stats.fullDocumentPreparations, 0ULL);
        QCOMPARE(stats.strokeSerializations, 1ULL);
        QCOMPARE(stats.clipMaskContentHashes, 1ULL);
        QCOMPARE(stats.clipMaskCompressions, 1ULL);
        QCOMPARE(stats.binaryMaskContentHashes, 0ULL);
        QCOMPARE(stats.binaryMaskCompressions, 0ULL);

        const Layer &baseLayer = base->document().layers.first();
        const Layer &appendedLayer =
            appended.prepared.document().layers.first();
        QCOMPARE(appendedLayer.strokes.size(), 513);
        QCOMPARE(appendedLayer.strokes.first().points.constData(),
            baseLayer.strokes.first().points.constData());
        const Stroke &stored = appendedLayer.strokes.constLast();
        QVERIFY(stored.points.constData() != stroke.points.constData());
        QVERIFY(stored.clipMask.cacheKey() != stroke.clipMask.cacheKey());
        QVERIFY(DocumentSerializer::retainImmutableBackings(
            appended.prepared, {stored})
                .isValid());

        rawPoints[0].pressure = 1.0;
        rawMask[0] = 255;
        QCOMPARE(stored.points.first().pressure, 0.25);
        QCOMPARE(stored.clipMask.constScanLine(0)[0], static_cast<uchar>(0));

        const QByteArray appendedJson =
            DocumentSerializer::toJson(appended.prepared, cache);
        QVERIFY(!appendedJson.isEmpty());
        QCOMPARE(appended.prepared.compactSize(),
            static_cast<qint64>(appendedJson.size()));
        QString error;
        const std::optional<Document> decoded =
            DocumentSerializer::fromJson(appendedJson, &error);
        QVERIFY2(decoded.has_value(), qPrintable(error));
        QCOMPARE(decoded->layers.first().strokes.size(), 513);

        Document expected = base->document();
        expected.layers.first().strokes.append(expectedStroke);
        QCOMPARE(appendedJson, DocumentSerializer::toJson(expected));

        const DocumentSerializer::AppendStrokeResult exact =
            DocumentSerializer::appendStroke(*base,
                layerId,
                expectedStroke,
                cache,
                appended.prepared.compactSize());
        QCOMPARE(
            exact.status, DocumentSerializer::AppendStrokeStatus::Appended);
        QCOMPARE(exact.prepared.compactSize(), appended.prepared.compactSize());
        const DocumentSerializer::AppendStrokeResult oneByteShort =
            DocumentSerializer::appendStroke(*base,
                layerId,
                expectedStroke,
                cache,
                appended.prepared.compactSize() - 1);
        QCOMPARE(oneByteShort.status,
            DocumentSerializer::AppendStrokeStatus::TooLarge);
        QCOMPARE(DocumentSerializer::toJson(*base, cache), baseJson);
        QCOMPARE(base->document().layers.first().strokes.constData(),
            baseStrokeBacking);
        QCOMPARE(
            base->document().layers.first().strokes.first().points.constData(),
            basePointBacking);

        Stroke equalMask = expectedStroke;
        equalMask.id = QUuid::createUuid();
        equalMask.clipMask = expectedStroke.clipMask.copy();
        cache.resetStats();
        DocumentSerializer::AppendStrokeResult reused =
            DocumentSerializer::appendStroke(appended.prepared,
                layerId,
                equalMask,
                cache,
                DocumentLimits::maximumProjectBytes);
        QCOMPARE(
            reused.status, DocumentSerializer::AppendStrokeStatus::Appended);
        QCOMPARE(cache.stats().incrementalStrokeAppends, 1ULL);
        QCOMPARE(cache.stats().fullDocumentPreparations, 0ULL);
        QCOMPARE(cache.stats().clipMaskContentHashes, 1ULL);
        QCOMPARE(cache.stats().clipMaskCompressions, 0ULL);
        const QVector<Stroke> &reusedStrokes =
            reused.prepared.document().layers.first().strokes;
        QCOMPARE(reusedStrokes.at(reusedStrokes.size() - 2).clipMask.cacheKey(),
            reusedStrokes.constLast().clipMask.cacheKey());
        const QByteArray reusedJson =
            DocumentSerializer::toJson(reused.prepared, cache);
        QCOMPARE(reused.prepared.compactSize(),
            static_cast<qint64>(reusedJson.size()));

        cache.resetStats();
        const DocumentSerializer::AppendStrokeResult tooLarge =
            DocumentSerializer::appendStroke(
                *base, layerId, expectedStroke, cache, base->compactSize());
        QCOMPARE(
            tooLarge.status, DocumentSerializer::AppendStrokeStatus::TooLarge);
        QCOMPARE(cache.stats().incrementalStrokeAppends, 0ULL);
        QCOMPARE(cache.stats().fullDocumentPreparations, 0ULL);
        QCOMPARE(DocumentSerializer::toJson(*base, cache), baseJson);
        QCOMPARE(base->document().layers.first().strokes.constData(),
            baseStrokeBacking);
        QCOMPARE(
            base->document().layers.first().strokes.first().points.constData(),
            basePointBacking);

        Stroke duplicate = expectedStroke;
        duplicate.id = baseLayer.strokes.first().id;
        const DocumentSerializer::AppendStrokeResult invalid =
            DocumentSerializer::appendStroke(*base,
                layerId,
                duplicate,
                cache,
                DocumentLimits::maximumProjectBytes);
        QCOMPARE(
            invalid.status, DocumentSerializer::AppendStrokeStatus::Invalid);
        QCOMPARE(DocumentSerializer::toJson(*base, cache), baseJson);
        QCOMPARE(base->document().layers.first().strokes.constData(),
            baseStrokeBacking);
        QCOMPARE(
            base->document().layers.first().strokes.first().points.constData(),
            basePointBacking);
    }

    void appendsDistinctPreparedFillMasksExactly()
    {
        Document source = Document::createDefault(QSize(48, 32));
        const QUuid layerId = source.activeLayerId;
        DocumentSerializer::SerializationCache cache;
        const auto base = DocumentSerializer::prepare(source, cache);
        QVERIFY(base.has_value());

        Stroke fill;
        fill.mode = StrokeMode::Fill;
        fill.color = QColor(20, 120, 230, 190);
        fill.points = {{QPointF(12.0, 14.0), 1.0}};
        fill.clipMask = QImage(source.size, QImage::Format_Grayscale8);
        fill.fillMask = QImage(source.size, QImage::Format_Grayscale8);
        fill.clipMask.fill(0);
        fill.fillMask.fill(0);
        for (int y = 3; y < 25; ++y)
        {
            std::fill_n(fill.clipMask.scanLine(y) + 4, 18, 255);
        }
        for (int y = 8; y < 29; ++y)
        {
            std::fill_n(fill.fillMask.scanLine(y) + 24, 20, 255);
        }
        const Stroke expectedFill = fill;

        cache.resetStats();
        const DocumentSerializer::AppendStrokeResult appended =
            DocumentSerializer::appendStroke(*base,
                layerId,
                fill,
                cache,
                DocumentLimits::maximumProjectBytes);
        QCOMPARE(
            appended.status, DocumentSerializer::AppendStrokeStatus::Appended);
        QVERIFY(appended.prepared.isValid());
        const auto stats = cache.stats();
        QCOMPARE(stats.incrementalStrokeAppends, 1ULL);
        QCOMPARE(stats.fullDocumentPreparations, 0ULL);
        QCOMPARE(stats.strokeSerializations, 1ULL);
        QCOMPARE(stats.clipMaskContentHashes, 2ULL);
        QCOMPARE(stats.clipMaskCompressions, 2ULL);

        const QByteArray appendedJson =
            DocumentSerializer::toJson(appended.prepared, cache);
        QVERIFY(!appendedJson.isEmpty());
        QCOMPARE(appended.prepared.compactSize(),
            static_cast<qint64>(appendedJson.size()));
        Document expected = base->document();
        expected.layers.first().strokes.append(expectedFill);
        QCOMPARE(appendedJson, DocumentSerializer::toJson(expected));

        const QJsonObject root = QJsonDocument::fromJson(appendedJson).object();
        QCOMPARE(root.value(QStringLiteral("clipMasks")).toArray().size(), 2);
        const QJsonObject strokeObject = root.value(QStringLiteral("layers"))
                                             .toArray()
                                             .first()
                                             .toObject()
                                             .value(QStringLiteral("strokes"))
                                             .toArray()
                                             .first()
                                             .toObject();
        const QString clipMaskId =
            strokeObject.value(QStringLiteral("clipMaskId")).toString();
        const QString fillMaskId =
            strokeObject.value(QStringLiteral("fillMaskId")).toString();
        QVERIFY(!clipMaskId.isEmpty());
        QVERIFY(!fillMaskId.isEmpty());
        QVERIFY(clipMaskId != fillMaskId);
    }

    void enforcesIncrementalDistinctMaskBudgetExactly()
    {
        QImage appendedMask(QSize(1, 1), QImage::Format_Grayscale8);
        QVERIFY(!appendedMask.isNull());
        appendedMask.fill(1);
        const quint64 appendedMaskBytes = appendedMask.sizeInBytes();
        QVERIFY(appendedMaskBytes > 0);
        const quint64 baseMaskBytes =
            DocumentLimits::maximumDistinctClipMaskBytes - appendedMaskBytes;
        QVERIFY(baseMaskBytes
                <= static_cast<quint64>(std::numeric_limits<qsizetype>::max()));

        std::unique_ptr<uchar[]> baseMaskBacking(
            new (std::nothrow) uchar[static_cast<std::size_t>(baseMaskBytes)]);
        QVERIFY(baseMaskBacking);
        baseMaskBacking[0] = 0;
        QImage baseMask(baseMaskBacking.get(),
            1,
            1,
            static_cast<qsizetype>(baseMaskBytes),
            QImage::Format_Grayscale8);
        QVERIFY(!baseMask.isNull());
        QCOMPARE(static_cast<quint64>(baseMask.sizeInBytes()), baseMaskBytes);

        Document source = Document::createDefault(QSize(1, 1));
        const QUuid layerId = source.activeLayerId;
        Stroke existing;
        existing.points = {{QPointF(0.0, 0.0), 1.0}};
        existing.clipMask = baseMask;
        source.layers.first().strokes.append(std::move(existing));

        DocumentSerializer::SerializationCache cache;
        const auto base = DocumentSerializer::prepare(std::move(source), cache);
        QVERIFY(base.has_value());
        baseMask = {};
        baseMaskBacking.reset();
        const QByteArray baseJson = DocumentSerializer::toJson(*base, cache);
        QVERIFY(!baseJson.isEmpty());
        const qint64 baseMaskKey =
            base->document().layers.first().strokes.first().clipMask.cacheKey();
        const StrokePoint *basePoints =
            base->document().layers.first().strokes.first().points.constData();

        Stroke exact;
        exact.points = {{QPointF(0.0, 0.0), 0.5}};
        exact.clipMask = appendedMask;
        StrokePoint *submittedPoints = exact.points.data();
        uchar *submittedMask = exact.clipMask.bits();
        cache.resetStats();
        DocumentSerializer::AppendStrokeResult appended =
            DocumentSerializer::appendStroke(*base,
                layerId,
                exact,
                cache,
                DocumentLimits::maximumProjectBytes);
        QCOMPARE(
            appended.status, DocumentSerializer::AppendStrokeStatus::Appended);
        QVERIFY(appended.prepared.isValid());
        QCOMPARE(cache.stats().incrementalStrokeAppends, 1ULL);
        const QByteArray appendedJson =
            DocumentSerializer::toJson(appended.prepared, cache);
        QVERIFY(!appendedJson.isEmpty());
        QCOMPARE(appended.prepared.compactSize(),
            static_cast<qint64>(appendedJson.size()));

        const Stroke &stored =
            appended.prepared.document().layers.first().strokes.constLast();
        submittedPoints[0].pressure = 1.0;
        submittedMask[0] = 3;
        QCOMPARE(stored.points.first().pressure, 0.5);
        QCOMPARE(stored.clipMask.constScanLine(0)[0], static_cast<uchar>(1));
        QCOMPARE(DocumentSerializer::toJson(*base, cache), baseJson);
        QCOMPARE(
            base->document().layers.first().strokes.first().clipMask.cacheKey(),
            baseMaskKey);
        QCOMPARE(
            base->document().layers.first().strokes.first().points.constData(),
            basePoints);

        Stroke over;
        over.points = {{QPointF(0.0, 0.0), 1.0}};
        over.clipMask = QImage(QSize(1, 1), QImage::Format_Grayscale8);
        QVERIFY(!over.clipMask.isNull());
        over.clipMask.fill(2);
        const qint64 appendedMaskKey = stored.clipMask.cacheKey();
        const StrokePoint *appendedPoints = stored.points.constData();
        const DocumentSerializer::AppendStrokeResult rejected =
            DocumentSerializer::appendStroke(appended.prepared,
                layerId,
                over,
                cache,
                DocumentLimits::maximumProjectBytes);
        QCOMPARE(
            rejected.status, DocumentSerializer::AppendStrokeStatus::MaskLimit);
        QCOMPARE(cache.stats().incrementalStrokeAppends, 1ULL);
        QCOMPARE(
            DocumentSerializer::toJson(appended.prepared, cache), appendedJson);
        QCOMPARE(appended.prepared.document()
                     .layers.first()
                     .strokes.constLast()
                     .clipMask.cacheKey(),
            appendedMaskKey);
        QCOMPARE(appended.prepared.document()
                     .layers.first()
                     .strokes.constLast()
                     .points.constData(),
            appendedPoints);
        QCOMPARE(DocumentSerializer::toJson(*base, cache), baseJson);
    }

    void reusesPreparedClipMaskAndStrokeMetadata()
    {
        Document source = Document::createDefault(QSize(257, 129));
        QImage mask(source.size, QImage::Format_Grayscale8);
        quint32 random = 0x9e3779b9U;
        for (int y = 0; y < mask.height(); ++y)
        {
            uchar *line = mask.scanLine(y);
            for (int x = 0; x < mask.width(); ++x)
            {
                random ^= random << 13U;
                random ^= random >> 17U;
                random ^= random << 5U;
                line[x] = static_cast<uchar>(random & 0xffU);
            }
        }
        Stroke masked;
        masked.points = {
            {QPointF(5.0, 5.0), 0.5}, {QPointF(200.0, 100.0), 1.0}};
        masked.clipMask = mask;
        source.layers.first().strokes.append(masked);

        DocumentSerializer::SerializationCache cache;
        const auto first = DocumentSerializer::prepare(source, cache);
        QVERIFY(first.has_value());
        const QByteArray firstJson = DocumentSerializer::toJson(*first, cache);
        QVERIFY(!firstJson.isEmpty());

        Document appended = first->document();
        Stroke plain;
        plain.points = {
            {QPointF(10.0, 20.0), 1.0}, {QPointF(40.0, 60.0), 0.75}};
        appended.layers.first().strokes.append(plain);
        cache.resetStats();
        const auto second = DocumentSerializer::prepare(
            appended, cache, &*first, DocumentLimits::maximumProjectBytes);
        QVERIFY(second.has_value());
        const auto appendStats = cache.stats();
        QCOMPARE(appendStats.clipMaskContentHashes, 0ULL);
        QCOMPARE(appendStats.clipMaskCompressions, 0ULL);
        QCOMPARE(appendStats.strokeSerializations, 1ULL);

        Document duplicated = first->document();
        Stroke duplicate = duplicated.layers.first().strokes.first();
        duplicate.id = QUuid::createUuid();
        duplicated.layers.first().strokes.append(std::move(duplicate));
        cache.resetStats();
        const auto duplicatedPrepared =
            DocumentSerializer::prepare(std::move(duplicated),
                cache,
                &*first,
                DocumentLimits::maximumProjectBytes);
        QVERIFY(duplicatedPrepared.has_value());
        const auto duplicateStats = cache.stats();
        QCOMPARE(duplicateStats.clipMaskContentHashes, 0ULL);
        QCOMPARE(duplicateStats.clipMaskCompressions, 0ULL);
        QCOMPARE(duplicateStats.strokeSerializations, 1ULL);

        Document equalContent = first->document();
        const qint64 originalKey =
            equalContent.layers.first().strokes.first().clipMask.cacheKey();
        equalContent.layers.first().strokes.first().clipMask =
            equalContent.layers.first().strokes.first().clipMask.copy();
        QVERIFY(equalContent.layers.first().strokes.first().clipMask.cacheKey()
                != originalKey);
        cache.resetStats();
        const auto equalPrepared = DocumentSerializer::prepare(
            equalContent, cache, &*first, DocumentLimits::maximumProjectBytes);
        QVERIFY(equalPrepared.has_value());
        const auto equalStats = cache.stats();
        QCOMPARE(equalStats.clipMaskContentHashes, 1ULL);
        QCOMPARE(equalStats.clipMaskCompressions, 0ULL);
        QCOMPARE(equalStats.strokeSerializations, 1ULL);
        QCOMPARE(DocumentSerializer::toJson(*equalPrepared, cache), firstJson);

        Document changed = first->document();
        QImage &changedMask = changed.layers.first().strokes.first().clipMask;
        const uchar previous = changedMask.constScanLine(0)[0];
        changedMask.scanLine(0)[0] = static_cast<uchar>(previous ^ 0xffU);
        cache.resetStats();
        const auto changedPrepared = DocumentSerializer::prepare(
            changed, cache, &*first, DocumentLimits::maximumProjectBytes);
        QVERIFY(changedPrepared.has_value());
        const auto changedStats = cache.stats();
        QCOMPARE(changedStats.clipMaskContentHashes, 1ULL);
        QCOMPARE(changedStats.clipMaskCompressions, 1ULL);
        QCOMPARE(changedStats.strokeSerializations, 1ULL);
        QCOMPARE(DocumentSerializer::toJson(*first, cache), firstJson);
        QVERIFY(
            DocumentSerializer::toJson(*changedPrepared, cache) != firstJson);
    }

    void distinguishesSharedBinaryMaskGeometryAndCowChanges()
    {
        Document source = Document::createDefault(QSize(16, 16));
        QByteArray packed(2, '\0');
        packed[0] = static_cast<char>(0x80);
        packed[1] = static_cast<char>(0x40);

        PixelSelectionOp firstOperation;
        firstOperation.canvasSize = source.size;
        firstOperation.sourceBounds = QRect(0, 0, 8, 2);
        firstOperation.packedMask = packed;
        firstOperation.sampling = SamplingMode::Nearest;
        PixelSelectionOp secondOperation = firstOperation;
        secondOperation.sourceBounds = QRect(0, 4, 16, 1);
        secondOperation.packedMask = firstOperation.packedMask;
        QVERIFY(firstOperation.packedMask.constData()
                == secondOperation.packedMask.constData());

        Stroke firstStroke;
        firstStroke.mode = StrokeMode::PixelSelection;
        firstStroke.points.clear();
        firstStroke.pixelSelectionOp = firstOperation;
        Stroke secondStroke;
        secondStroke.mode = StrokeMode::PixelSelection;
        secondStroke.points.clear();
        secondStroke.pixelSelectionOp = secondOperation;
        source.layers.first().strokes = {firstStroke, secondStroke};

        DocumentSerializer::SerializationCache cache;
        const auto prepared = DocumentSerializer::prepare(source, cache);
        QVERIFY(prepared.has_value());
        const auto initialStats = cache.stats();
        QCOMPARE(initialStats.binaryMaskContentHashes, 2ULL);
        QCOMPARE(initialStats.binaryMaskCompressions, 2ULL);
        const QByteArray originalJson =
            DocumentSerializer::toJson(*prepared, cache);
        const QJsonArray masks = QJsonDocument::fromJson(originalJson)
                                     .object()
                                     .value(QStringLiteral("binaryMasks"))
                                     .toArray();
        QCOMPARE(masks.size(), 2);

        Document changed = prepared->document();
        QByteArray &changedBytes =
            changed.layers.first().strokes.first().pixelSelectionOp->packedMask;
        changedBytes[0] = static_cast<char>(0x20);
        cache.resetStats();
        const auto changedPrepared = DocumentSerializer::prepare(
            changed, cache, &*prepared, DocumentLimits::maximumProjectBytes);
        QVERIFY(changedPrepared.has_value());
        const auto changedStats = cache.stats();
        QCOMPARE(changedStats.binaryMaskContentHashes, 1ULL);
        QCOMPARE(changedStats.binaryMaskCompressions, 1ULL);
        QCOMPARE(changedStats.strokeSerializations, 1ULL);
        QCOMPARE(DocumentSerializer::toJson(*prepared, cache), originalJson);
        QVERIFY(DocumentSerializer::toJson(*changedPrepared, cache)
                != originalJson);

        DocumentSerializer::SerializationCache tinyCache(1);
        const auto tinyPrepared =
            DocumentSerializer::prepare(source, tinyCache);
        QVERIFY(tinyPrepared.has_value());
        QCOMPARE(tinyCache.payloadBytes(), 0);
        tinyCache.resetStats();
        const QByteArray tinyJson =
            DocumentSerializer::toJson(*tinyPrepared, tinyCache);
        QCOMPARE(tinyJson, originalJson);
        QCOMPARE(tinyCache.stats().binaryMaskCompressions, 2ULL);
        QCOMPARE(tinyCache.payloadBytes(), 0);

        const qint64 onePayloadBytes = qCompress(packed, 6).size();
        DocumentSerializer::SerializationCache lruCache(onePayloadBytes);
        const auto lruPrepared = DocumentSerializer::prepare(source, lruCache);
        QVERIFY(lruPrepared.has_value());
        QVERIFY(lruCache.payloadBytes() > 0);
        QVERIFY(lruCache.payloadBytes() <= lruCache.payloadCapacityBytes());
        lruCache.resetStats();
        QCOMPARE(
            DocumentSerializer::toJson(*lruPrepared, lruCache), originalJson);
        QCOMPARE(lruCache.stats().binaryMaskCompressions, 2ULL);
        QVERIFY(lruCache.payloadBytes() <= lruCache.payloadCapacityBytes());

        DocumentSerializer::SerializationCache cappedCache(
            DocumentSerializer::SerializationCache::maximumPayloadBytes * 2);
        QCOMPARE(cappedCache.payloadCapacityBytes(),
            DocumentSerializer::SerializationCache::maximumPayloadBytes);
    }

    void cachesFourKMaskAcrossAccumulatedStrokes()
    {
        constexpr int edge = 4096;
        Document source = Document::createDefault(QSize(edge, edge));
        PixelSelectionOp operation;
        operation.canvasSize = source.size;
        operation.sourceBounds = QRect(QPoint(), source.size);
        operation.packedMask =
            QByteArray(static_cast<qsizetype>(edge) * edge / 8, '\0');
        quint32 random = 0x6d2b79f5U;
        for (char &byte : operation.packedMask)
        {
            random ^= random << 13U;
            random ^= random >> 17U;
            random ^= random << 5U;
            byte = static_cast<char>(random & 0xffU);
        }
        operation.sampling = SamplingMode::Nearest;
        Stroke selectionStroke;
        selectionStroke.mode = StrokeMode::PixelSelection;
        selectionStroke.points.clear();
        selectionStroke.pixelSelectionOp = std::move(operation);
        source.layers.first().strokes.append(std::move(selectionStroke));

        DocumentSerializer::SerializationCache cache;
        auto current = DocumentSerializer::prepare(source, cache);
        QVERIFY(current.has_value());
        QCOMPARE(cache.stats().binaryMaskCompressions, 1ULL);

        constexpr int appendedStrokeCount = 64;
        QElapsedTimer timer;
        timer.start();
        for (int index = 0; index < appendedStrokeCount; ++index)
        {
            Document candidate = current->document();
            Stroke stroke;
            stroke.points = {{QPointF(10.0 + index, 20.0 + index), 1.0}};
            candidate.layers.first().strokes.append(std::move(stroke));
            cache.resetStats();
            auto next = DocumentSerializer::prepare(std::move(candidate),
                cache,
                &*current,
                DocumentLimits::maximumProjectBytes);
            QVERIFY(next.has_value());
            const auto stats = cache.stats();
            QCOMPARE(stats.binaryMaskContentHashes, 0ULL);
            QCOMPARE(stats.binaryMaskCompressions, 0ULL);
            QCOMPARE(stats.strokeSerializations, 1ULL);
            current = std::move(next);
        }
        const qint64 elapsedMilliseconds = timer.elapsed();
        const QByteArray json = DocumentSerializer::toJson(*current, cache);
        QVERIFY(!json.isEmpty());
        QCOMPARE(current->compactSize(), static_cast<qint64>(json.size()));
        qInfo().nospace() << "4K prepared-cache benchmark: "
                          << appendedStrokeCount << " accumulated strokes in "
                          << elapsedMilliseconds << " ms";
    }

    void rebindsPreparedActiveLayerWithoutRebuildingContent()
    {
        Document source = Document::createDefault(QSize(32, 32));
        Layer second;
        second.name = QStringLiteral("Layer 2");
        second.initialCanvasSize = source.size;
        source.layers.append(second);

        DocumentSerializer::SerializationCache cache;
        const auto prepared = DocumentSerializer::prepare(source, cache);
        QVERIFY(prepared.has_value());
        cache.resetStats();
        const auto rebound =
            DocumentSerializer::rebindActiveLayer(*prepared, second.id);
        QVERIFY(rebound.has_value());
        QCOMPARE(rebound->document().activeLayerId, second.id);
        QCOMPARE(rebound->compactSize(), prepared->compactSize());
        const QJsonObject root =
            QJsonDocument::fromJson(DocumentSerializer::toJson(*rebound, cache))
                .object();
        QCOMPARE(root.value(QStringLiteral("activeLayerId")).toString(),
            second.id.toString(QUuid::WithoutBraces));
        QCOMPARE(cache.stats().strokeSerializations, 0ULL);
        QVERIFY(!DocumentSerializer::rebindActiveLayer(
            *prepared, QUuid::createUuid())
                .has_value());
    }

    void savesMaximumPointBudget()
    {
        QTemporaryDir directory;
        QVERIFY(directory.isValid());
        const QString path =
            directory.filePath(QStringLiteral("maximum-points.ugu"));
        Document source = Document::createDefault(QSize(4096, 4096));
        const StrokePoint point{
            QPointF(1234.56789012345, 3987.65432109876), 0.543210987654321};

        Stroke first;
        first.points.fill(point, DocumentLimits::maximumPointsPerStroke);
        Stroke second;
        second.points.fill(point,
            DocumentLimits::maximumTotalPoints
                - DocumentLimits::maximumPointsPerStroke);
        source.layers.first().strokes.append(std::move(first));
        source.layers.first().strokes.append(std::move(second));

        QString error;
        QVERIFY2(
            DocumentSerializer::save(path, source, &error), qPrintable(error));
        QVERIFY(QFileInfo(path).size() <= DocumentLimits::maximumProjectBytes);

        const std::optional<Document> loaded =
            DocumentSerializer::load(path, &error);
        QVERIFY2(loaded.has_value(), qPrintable(error));
        QCOMPARE(loaded->layers.first().strokes.size(), 2);
        QCOMPARE(loaded->layers.first().strokes.first().points.size()
                     + loaded->layers.first().strokes.last().points.size(),
            DocumentLimits::maximumTotalPoints);
    }

    void rejectsUnsafeDocumentsBeforeSaving()
    {
        QTemporaryDir directory;
        QVERIFY(directory.isValid());
        const QString path = directory.filePath(QStringLiteral("unsafe.ugu"));

        Document document = Document::createDefault(QSize(100, 100));
        Stroke stroke;
        stroke.points = {
            {QPointF(
                 DocumentLimits::maximumStoredCoordinateMagnitude + 1.0, 50.0),
                0.5}};
        document.layers.first().strokes.append(stroke);

        QString error;
        QVERIFY(!DocumentSerializer::save(path, document, &error));
        QVERIFY(!error.isEmpty());
        QVERIFY(!QFile::exists(path));

        document.layers.first().strokes.first().points.first().position =
            QPointF(50.0, 50.0);
        document.framesPerSecond = std::numeric_limits<qreal>::quiet_NaN();
        error.clear();
        QVERIFY(!DocumentSerializer::save(path, document, &error));
        QVERIFY(!error.isEmpty());
        QVERIFY(!QFile::exists(path));
    }

    void rejectsOversizedProjectFile()
    {
        QTemporaryDir directory;
        QVERIFY(directory.isValid());
        const QString path =
            directory.filePath(QStringLiteral("oversized.ugu"));
        QFile file(path);
        QVERIFY(file.open(QIODevice::WriteOnly));
        QVERIFY(file.resize(DocumentLimits::maximumProjectBytes + 1));
        file.close();

        QString error;
        const std::optional<Document> document =
            DocumentSerializer::load(path, &error);
        QVERIFY(!document.has_value());
        QVERIFY(!error.isEmpty());
    }

    void boundsTheReadWhenTheReportedFileSizeIsStale()
    {
        const QByteArray data =
            DocumentSerializer::toJson(Document::createDefault(QSize(40, 30)));
        QVERIFY(data.size() > 1);
        StaleSizeBuffer device;
        device.setData(data);
        QVERIFY(device.open(QIODevice::ReadOnly));

        QString error;
        QVERIFY(!DocumentSerializerTestAccess::load(
            device, data.size() - 1, &error));
        QVERIFY(error.contains(QStringLiteral("too large")));

        QVERIFY(device.seek(0));
        error.clear();
        const std::optional<Document> loaded =
            DocumentSerializerTestAccess::load(device, data.size(), &error);
        QVERIFY2(loaded.has_value(), qPrintable(error));
        QCOMPARE(loaded->size, QSize(40, 30));
    }

    void rejectsExcessivePointCollection()
    {
        const QByteArray json =
            QByteArrayLiteral(
                R"({"schemaVersion":1,"algorithmVersion":1,"canvas":{"width":100,"height":100,"background":"#ffffffff"},"animation":{"frames":30,"fps":25,"wobble":1.6},"activeLayerId":"11111111-1111-1111-1111-111111111111","layers":[{"id":"11111111-1111-1111-1111-111111111111","name":"Layer","visible":true,"opacity":1,"strokes":[{"id":"22222222-2222-2222-2222-222222222222","seed":"1","mode":"paint","color":"#ff000000","width":6,"points":)")
            + pointArray(DocumentLimits::maximumPointsPerStroke)
            + QByteArrayLiteral(
                R"(},{"id":"33333333-3333-3333-3333-333333333333","seed":"2","mode":"paint","color":"#ff000000","width":6,"points":)")
            + pointArray(DocumentLimits::maximumTotalPoints
                         - DocumentLimits::maximumPointsPerStroke + 1)
            + QByteArrayLiteral(R"(}]}]})");

        QString error;
        const std::optional<Document> document =
            DocumentSerializer::fromJson(json, &error);
        QVERIFY(!document.has_value());
        QVERIFY(!error.isEmpty());
    }

    void enforcesEditingLimits()
    {
        DocumentController controller;
        controller.newDocument(
            QSize(0, DocumentLimits::maximumCanvasEdge + 100));
        QCOMPARE(controller.document().size,
            QSize(DocumentLimits::minimumCanvasEdge,
                DocumentLimits::maximumCanvasEdge));

        controller.newDocument(QSize(100, 100));
        const QUuid layerId = controller.document().activeLayerId;
        Stroke stroke;
        stroke.points.fill({QPointF(50.0, 50.0), 0.5},
            DocumentLimits::maximumPointsPerStroke + 1);
        const DocumentController::AddStrokeResult strokeResult =
            controller.addStroke(layerId, std::move(stroke));
        QCOMPARE(strokeResult,
            DocumentController::AddStrokeResult::AddedWithResampledPoints);
        QCOMPARE(
            controller.document().layer(layerId)->strokes.first().points.size(),
            DocumentLimits::maximumPointsPerStroke);

        Stroke invalid;
        invalid.points = {{QPointF(150.0, 50.0), 0.5}};
        const DocumentController::AddStrokeResult invalidResult =
            controller.addStroke(layerId, std::move(invalid));
        QCOMPARE(invalidResult,
            DocumentController::AddStrokeResult::RejectedInvalidStroke);
        QCOMPARE(controller.document().layer(layerId)->strokes.size(), 1);

        const int undoCount = controller.undoStack()->count();
        controller.setFramesPerSecond(std::numeric_limits<qreal>::quiet_NaN());
        QCOMPARE(controller.undoStack()->count(), undoCount);

        Document full = Document::createDefault(QSize(100, 100));
        while (full.layers.size() < DocumentLimits::maximumLayers)
        {
            Layer layer;
            layer.name = QStringLiteral("Layer %1").arg(full.layers.size() + 1);
            full.layers.append(layer);
        }
        full.activeLayerId = full.layers.constLast().id;
        controller.loadDocument(std::move(full));
        const int layerCount =
            static_cast<int>(controller.document().layers.size());
        controller.addLayer();
        QCOMPARE(controller.document().layers.size(), layerCount);
        controller.duplicateLayer(controller.document().activeLayerId);
        QCOMPARE(controller.document().layers.size(), layerCount);
        QCOMPARE(controller.undoStack()->count(), 0);
    }

    void preservesStrokeEndpointsWhenPointBudgetIsLimited()
    {
        Document nearLimit = Document::createDefault(QSize(100, 100));
        const StrokePoint repeated{QPointF(50.0, 50.0), 0.5};
        Stroke first;
        first.points.fill(repeated, DocumentLimits::maximumPointsPerStroke);
        Stroke second;
        second.points.fill(repeated,
            DocumentLimits::maximumTotalPoints
                - DocumentLimits::maximumPointsPerStroke - 3);
        nearLimit.layers.first().strokes.append(std::move(first));
        nearLimit.layers.first().strokes.append(std::move(second));

        DocumentController controller;
        controller.loadDocument(std::move(nearLimit));
        const QUuid layerId = controller.document().activeLayerId;
        Stroke stroke;
        stroke.points = {{QPointF(10.0, 10.0), 0.1},
            {QPointF(20.0, 20.0), 0.2},
            {QPointF(30.0, 30.0), 0.3},
            {QPointF(40.0, 40.0), 0.4},
            {QPointF(50.0, 50.0), 0.5}};
        const QVector<StrokePoint> submittedPoints = stroke.points;

        const DocumentController::AddStrokeResult result =
            controller.addStroke(layerId, std::move(stroke));
        QCOMPARE(result,
            DocumentController::AddStrokeResult::AddedWithResampledPoints);
        const QVector<StrokePoint> &storedPoints =
            controller.document().layer(layerId)->strokes.constLast().points;
        QCOMPARE(storedPoints.size(), 3);
        QCOMPARE(storedPoints.first(), submittedPoints.first());
        QCOMPARE(storedPoints.at(1), submittedPoints.at(2));
        QCOMPARE(storedPoints.last(), submittedPoints.last());

        Stroke beyondLimit;
        beyondLimit.points = {{QPointF(60.0, 60.0), 1.0}};
        QCOMPARE(controller.addStroke(layerId, std::move(beyondLimit)),
            DocumentController::AddStrokeResult::RejectedPointLimit);
        QCOMPARE(controller.document().layer(layerId)->strokes.size(), 3);
    }

    void reportsLayerRenameOutcomes()
    {
        DocumentController controller;
        controller.newDocument(QSize(100, 100));
        const QUuid layerId = controller.document().activeLayerId;

        QCOMPARE(controller.renameLayer(layerId, QStringLiteral("Renamed")),
            DocumentController::RenameLayerResult::Renamed);
        QCOMPARE(controller.renameLayer(layerId, QStringLiteral(" Renamed ")),
            DocumentController::RenameLayerResult::Unchanged);
        QCOMPARE(controller.renameLayer(layerId, QString()),
            DocumentController::RenameLayerResult::RejectedEmptyName);
        QCOMPARE(controller.renameLayer(layerId,
                     QString(DocumentLimits::maximumLayerNameLength + 1,
                         QLatin1Char('x'))),
            DocumentController::RenameLayerResult::RejectedNameTooLong);
        QCOMPARE(controller.renameLayer(
                     QUuid::createUuid(), QStringLiteral("Missing")),
            DocumentController::RenameLayerResult::RejectedInvalidLayer);
        QCOMPARE(controller.document().layer(layerId)->name,
            QStringLiteral("Renamed"));
    }

    void rejectsInvalidJson_data()
    {
        QTest::addColumn<QByteArray>("json");

        QTest::newRow("malformed") << QByteArrayLiteral("{");
        QTest::newRow("unsupported-version") << QByteArrayLiteral(
            R"({"schemaVersion":10,"algorithmVersion":2,"canvas":{"width":10,"height":10},"layers":[]})");
        QTest::newRow("unsupported-algorithm") << QByteArrayLiteral(
            R"({"schemaVersion":2,"algorithmVersion":3,"canvas":{"width":10,"height":10},"layers":[{}]})");
        QTest::newRow("invalid-canvas") << QByteArrayLiteral(
            R"({"schemaVersion":1,"canvas":{"width":0,"height":10},"layers":[{}]})");
        QTest::newRow("missing-layers") << QByteArrayLiteral(
            R"({"schemaVersion":1,"canvas":{"width":10,"height":10},"layers":[]})");
        QTest::newRow("duplicate-layers") << QByteArrayLiteral(
            R"({"schemaVersion":1,"canvas":{"width":10,"height":10},"layers":[{"id":"11111111-1111-1111-1111-111111111111","strokes":[]},{"id":"11111111-1111-1111-1111-111111111111","strokes":[]}]})");
        QTest::newRow("empty-stroke") << QByteArrayLiteral(
            R"({"schemaVersion":1,"canvas":{"width":10,"height":10},"layers":[{"name":"Layer","strokes":[{"points":[]}]}]})");
        QTest::newRow("invalid-point") << QByteArrayLiteral(
            R"({"schemaVersion":1,"canvas":{"width":10,"height":10},"layers":[{"name":"Layer","strokes":[{"points":["bad"]}]}]})");
        QTest::newRow("fractional-canvas") << QByteArrayLiteral(
            R"({"schemaVersion":1,"algorithmVersion":1,"canvas":{"width":10.5,"height":10,"background":"#ffffffff"},"animation":{"frames":2,"fps":25,"wobble":1},"activeLayerId":"11111111-1111-1111-1111-111111111111","layers":[{"id":"11111111-1111-1111-1111-111111111111","name":"Layer","visible":true,"opacity":1,"strokes":[]}]})");
        QTest::newRow("invalid-active-layer") << QByteArrayLiteral(
            R"({"schemaVersion":1,"algorithmVersion":1,"canvas":{"width":10,"height":10,"background":"#ffffffff"},"animation":{"frames":2,"fps":25,"wobble":1},"activeLayerId":"22222222-2222-2222-2222-222222222222","layers":[{"id":"11111111-1111-1111-1111-111111111111","name":"Layer","visible":true,"opacity":1,"strokes":[]}]})");
        QTest::newRow("active-layer-without-layers") << QByteArrayLiteral(
            R"({"schemaVersion":4,"algorithmVersion":2,"canvas":{"width":10,"height":10,"background":"#ffffffff"},"animation":{"frames":2,"fps":25,"wobble":1},"activeLayerId":"11111111-1111-1111-1111-111111111111","layers":[],"clipMasks":[]})");
        QTest::newRow("malformed-active-layer-without-layers") << QByteArrayLiteral(
            R"({"schemaVersion":4,"algorithmVersion":2,"canvas":{"width":10,"height":10,"background":"#ffffffff"},"animation":{"frames":2,"fps":25,"wobble":1},"activeLayerId":"garbage","layers":[],"clipMasks":[]})");
        QTest::newRow("outside-point") << QByteArrayLiteral(
            R"({"schemaVersion":1,"algorithmVersion":1,"canvas":{"width":10,"height":10,"background":"#ffffffff"},"animation":{"frames":2,"fps":25,"wobble":1},"activeLayerId":"11111111-1111-1111-1111-111111111111","layers":[{"id":"11111111-1111-1111-1111-111111111111","name":"Layer","visible":true,"opacity":1,"strokes":[{"id":"22222222-2222-2222-2222-222222222222","seed":"1","mode":"paint","color":"#ff000000","width":1,"points":[[11,5,1]]}]}]})");
        QTest::newRow("invalid-pressure") << QByteArrayLiteral(
            R"({"schemaVersion":1,"algorithmVersion":1,"canvas":{"width":10,"height":10,"background":"#ffffffff"},"animation":{"frames":2,"fps":25,"wobble":1},"activeLayerId":"11111111-1111-1111-1111-111111111111","layers":[{"id":"11111111-1111-1111-1111-111111111111","name":"Layer","visible":true,"opacity":1,"strokes":[{"id":"22222222-2222-2222-2222-222222222222","seed":"1","mode":"paint","color":"#ff000000","width":1,"points":[[5,5,1.1]]}]}]})");
        QTest::newRow("missing-brush") << QByteArrayLiteral(
            R"({"schemaVersion":2,"algorithmVersion":2,"canvas":{"width":10,"height":10,"background":"#ffffffff"},"animation":{"frames":2,"fps":25,"wobble":1},"activeLayerId":"11111111-1111-1111-1111-111111111111","layers":[{"id":"11111111-1111-1111-1111-111111111111","name":"Layer","visible":true,"opacity":1,"strokes":[{"id":"22222222-2222-2222-2222-222222222222","seed":"1","mode":"paint","color":"#ff000000","width":1,"points":[[5,5,1]]}]}]})");
        QTest::newRow("wrong-field-type") << QByteArrayLiteral(
            R"({"schemaVersion":1,"algorithmVersion":1,"canvas":{"width":10,"height":10,"background":"#ffffffff"},"animation":{"frames":2,"fps":25,"wobble":1},"activeLayerId":"11111111-1111-1111-1111-111111111111","layers":[{"id":"11111111-1111-1111-1111-111111111111","name":"Layer","visible":1,"opacity":1,"strokes":[]}]})");
        QTest::newRow("fps-too-high") << QByteArrayLiteral(
            R"({"schemaVersion":1,"algorithmVersion":1,"canvas":{"width":10,"height":10,"background":"#ffffffff"},"animation":{"frames":2,"fps":51,"wobble":1},"activeLayerId":"11111111-1111-1111-1111-111111111111","layers":[{"id":"11111111-1111-1111-1111-111111111111","name":"Layer","visible":true,"opacity":1,"strokes":[]}]})");
    }

    void rejectsInvalidJson()
    {
        QFETCH(QByteArray, json);

        QString error;
        const std::optional<Document> document =
            DocumentSerializer::fromJson(json, &error);
        QVERIFY(!document.has_value());
        QVERIFY(!error.isEmpty());
    }
};

int runSerializationBudgetTests(int argc, char **argv)
{
    SerializationBudgetTests tests;
    return QTest::qExec(&tests, argc, argv);
}

}

#include "SerializationBudgetTests.moc"

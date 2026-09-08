// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "io/DocumentSerializer.hpp"

#include "document/DocumentLimits.hpp"
#include "document/DocumentOperations.hpp"
#include "document/LayerHierarchy.hpp"
#include "document/SelectionOperation.hpp"
#include "io/serializer/DocumentJsonCodec.hpp"
#include "io/serializer/DocumentValidation.hpp"
#include "io/serializer/MaskAssetTable.hpp"
#include "io/serializer/PreparedPlanBuilder.hpp"
#include "io/serializer/RasterAssetTable.hpp"
#include "io/serializer/SerializerSchema.hpp"

#include <QCryptographicHash>
#include <QFile>
#include <QHash>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QMap>
#include <QSaveFile>
#include <QSet>

#include <algorithm>
#include <cmath>
#include <cstring>
#include <limits>
#include <list>
#include <memory>
#include <utility>

namespace ugurugu
{

using namespace serializer_detail;

struct DocumentSerializer::SerializationCache::Impl
{
    struct Payload
    {
        QByteArray compressed;
        std::list<QString>::iterator lruPosition;
    };

    explicit Impl(qint64 requestedCapacity)
        : capacity(std::clamp(requestedCapacity,
              0LL,
              DocumentSerializer::SerializationCache::maximumPayloadBytes))
    {
    }

    QByteArray payload(const QString &key)
    {
        const auto found = payloads.find(key);
        if (found == payloads.end())
        {
            ++statistics.payloadCacheMisses;
            return {};
        }
        lru.splice(lru.begin(), lru, found->lruPosition);
        ++statistics.payloadCacheHits;
        return found->compressed;
    }

    void storePayload(const QString &key, const QByteArray &compressed)
    {
        if (compressed.isEmpty() || compressed.size() > capacity)
        {
            return;
        }
        auto existing = payloads.find(key);
        if (existing != payloads.end())
        {
            residentBytes -= existing->compressed.size();
            lru.erase(existing->lruPosition);
            payloads.erase(existing);
        }
        while (!lru.empty() && residentBytes > capacity - compressed.size())
        {
            const QString evictedKey = lru.back();
            const auto evicted = payloads.find(evictedKey);
            if (evicted != payloads.end())
            {
                residentBytes -= evicted->compressed.size();
                payloads.erase(evicted);
            }
            lru.pop_back();
        }
        lru.push_front(key);
        Payload payload{compressed, lru.begin()};
        residentBytes += compressed.size();
        payloads.insert(key, std::move(payload));
    }

    void clear()
    {
        payloads.clear();
        lru.clear();
        residentBytes = 0;
    }

    qint64 capacity = 0;
    qint64 residentBytes = 0;
    QHash<QString, Payload> payloads;
    std::list<QString> lru;
    DocumentSerializer::SerializationCache::Stats statistics;
};

struct DocumentSerializer::PreparedDocument::Impl
{
    Document document;
    serializer_detail::PreparedPlan plan;
};

struct DocumentSerializer::ImmutableBackingLease::Impl
{
    serializer_detail::ImmutableBackings backings;
};

DocumentSerializer::SerializationCache::SerializationCache(
    qint64 payloadCapacityBytes)
    : m_impl(std::make_unique<Impl>(payloadCapacityBytes))
{
}

DocumentSerializer::SerializationCache::~SerializationCache() = default;

DocumentSerializer::SerializationCache::SerializationCache(
    SerializationCache &&) noexcept = default;

DocumentSerializer::SerializationCache &
DocumentSerializer::SerializationCache::operator=(
    SerializationCache &&) noexcept = default;

void DocumentSerializer::SerializationCache::clear()
{
    if (m_impl)
    {
        m_impl->clear();
    }
}

void DocumentSerializer::SerializationCache::resetStats()
{
    if (m_impl)
    {
        m_impl->statistics = {};
    }
}

DocumentSerializer::SerializationCache::Stats
DocumentSerializer::SerializationCache::stats() const
{
    return m_impl ? m_impl->statistics : Stats{};
}

qint64 DocumentSerializer::SerializationCache::payloadBytes() const
{
    return m_impl ? m_impl->residentBytes : 0;
}

qint64 DocumentSerializer::SerializationCache::payloadCapacityBytes() const
{
    return m_impl ? m_impl->capacity : 0;
}

DocumentSerializer::PreparedDocument::PreparedDocument() = default;
DocumentSerializer::PreparedDocument::~PreparedDocument() = default;
DocumentSerializer::PreparedDocument::PreparedDocument(
    const PreparedDocument &) = default;
DocumentSerializer::PreparedDocument &
DocumentSerializer::PreparedDocument::operator=(
    const PreparedDocument &) = default;
DocumentSerializer::PreparedDocument::PreparedDocument(
    PreparedDocument &&) noexcept = default;
DocumentSerializer::PreparedDocument &
DocumentSerializer::PreparedDocument::operator=(
    PreparedDocument &&) noexcept = default;

DocumentSerializer::PreparedDocument::PreparedDocument(
    std::shared_ptr<const Impl> impl)
    : m_impl(std::move(impl))
{
}

bool DocumentSerializer::PreparedDocument::isValid() const
{
    return static_cast<bool>(m_impl);
}

const Document &DocumentSerializer::PreparedDocument::document() const
{
    static const Document empty;
    return m_impl ? m_impl->document : empty;
}

qint64 DocumentSerializer::PreparedDocument::compactSize() const
{
    return m_impl ? m_impl->plan.compactSize : 0;
}

qsizetype DocumentSerializer::PreparedDocument::totalStrokeCount() const
{
    return m_impl ? m_impl->plan.totalStrokeCount : 0;
}

qsizetype DocumentSerializer::PreparedDocument::totalPointCount() const
{
    return m_impl ? m_impl->plan.totalPointCount : 0;
}

DocumentSerializer::ImmutableBackingLease::ImmutableBackingLease() = default;
DocumentSerializer::ImmutableBackingLease::~ImmutableBackingLease() = default;
DocumentSerializer::ImmutableBackingLease::ImmutableBackingLease(
    const ImmutableBackingLease &) = default;
DocumentSerializer::ImmutableBackingLease &
DocumentSerializer::ImmutableBackingLease::operator=(
    const ImmutableBackingLease &) = default;
DocumentSerializer::ImmutableBackingLease::ImmutableBackingLease(
    ImmutableBackingLease &&) noexcept = default;
DocumentSerializer::ImmutableBackingLease &
DocumentSerializer::ImmutableBackingLease::operator=(
    ImmutableBackingLease &&) noexcept = default;

DocumentSerializer::ImmutableBackingLease::ImmutableBackingLease(
    std::shared_ptr<const Impl> impl)
    : m_impl(std::move(impl))
{
}

bool DocumentSerializer::ImmutableBackingLease::isValid() const
{
    return static_cast<bool>(m_impl);
}

std::optional<DocumentSerializer::PreparedDocument> DocumentSerializer::prepare(
    Document document, SerializationCache &cache, QString *error)
{
    return prepare(std::move(document),
        cache,
        nullptr,
        DocumentLimits::maximumProjectBytes,
        error);
}

std::optional<DocumentSerializer::PreparedDocument> DocumentSerializer::prepare(
    Document document,
    SerializationCache &cache,
    const PreparedDocument *base,
    qint64 maximumBytes,
    QString *error)
{
    return prepare(
        std::move(document), cache, base, nullptr, maximumBytes, error);
}

DocumentSerializer::ImmutableBackingLease
DocumentSerializer::retainImmutableBackings(
    const PreparedDocument &source, const QVector<Stroke> &strokes)
{
    if (!source.m_impl)
    {
        return {};
    }
    auto impl = std::make_shared<ImmutableBackingLease::Impl>();
    for (const Stroke &stroke : strokes)
    {
        const auto trusted = source.m_impl->plan.strokes.constFind(stroke.id);
        if (trusted == source.m_impl->plan.strokes.cend()
            || !sameStrokeIdentity(stroke, trusted->snapshot))
        {
            return {};
        }
        rememberImmutablePoints(impl->backings, trusted->snapshot.points);
        rememberImmutableImage(impl->backings, trusted->snapshot.clipMask);
        rememberImmutableImage(impl->backings, trusted->snapshot.fillMask);
        if (trusted->snapshot.fillCoverage)
        {
            rememberImmutableBytes(
                impl->backings, trusted->snapshot.fillCoverage->packedMask);
        }
        if (trusted->snapshot.pixelSelectionOp)
        {
            rememberImmutableBytes(
                impl->backings, trusted->snapshot.pixelSelectionOp->packedMask);
        }
    }
    return ImmutableBackingLease(std::move(impl));
}

std::optional<DocumentSerializer::PreparedDocument> DocumentSerializer::prepare(
    Document document,
    SerializationCache &cache,
    const PreparedDocument *base,
    const ImmutableBackingLease *trusted,
    qint64 maximumBytes,
    QString *error)
{
    if (!cache.m_impl || maximumBytes < 0
        || maximumBytes > DocumentLimits::maximumProjectBytes
        || (base && !base->m_impl) || (trusted && !trusted->m_impl))
    {
        setError(error,
            DocumentSerializer::tr("The project contains invalid "
                                   "operations or too much mask data."));
        return std::nullopt;
    }

    if (base)
    {
        MetadataReuseResult reused =
            reusePreparedContentForMetadataEdit(document,
                base->m_impl->document,
                base->m_impl->plan,
                maximumBytes,
                error);
        if (reused.status == MetadataReuseStatus::Success)
        {
            auto impl = std::make_shared<PreparedDocument::Impl>();
            impl->document = std::move(reused.document);
            impl->plan = std::move(reused.plan);
            return PreparedDocument(std::move(impl));
        }
        if (reused.status != MetadataReuseStatus::NotApplicable)
        {
            return std::nullopt;
        }
    }

    ++cache.m_impl->statistics.fullDocumentPreparations;
    normalizeLayerInitialCanvasSizes(document);
    if (!DocumentOperations::normalizeAndValidate(document))
    {
        setError(error,
            DocumentSerializer::tr("The project contains invalid "
                                   "operations or too much mask data."));
        return std::nullopt;
    }
    DocumentValidationStats validationStats;
    if (!validateDocument(document, schemaVersion, error, &validationStats))
    {
        return std::nullopt;
    }

    const PreparedPlan *basePlan = base ? &base->m_impl->plan : nullptr;
    const ImmutableBackings *trustedBackings =
        trusted ? &trusted->m_impl->backings : nullptr;
    document = DocumentFreezer(basePlan, trustedBackings).freeze(document);
    const ClipMaskTable clipMasks = buildClipMaskTable(
        document, basePlan, *cache.m_impl, maximumBytes, false);
    const BinaryMaskTable binaryMasks = buildBinaryMaskTable(
        document, basePlan, *cache.m_impl, maximumBytes, false);
    PreparedPlan plan;
    if (clipMasks.invalid || binaryMasks.invalid)
    {
        setError(error,
            DocumentSerializer::tr("The project contains invalid "
                                   "operations or too much mask data."));
        return std::nullopt;
    }
    if (clipMasks.tooLarge || binaryMasks.tooLarge
        || !serializedSizeWithinLimit(document,
            clipMasks,
            binaryMasks,
            basePlan,
            plan,
            *cache.m_impl,
            maximumBytes))
    {
        setError(
            error, DocumentSerializer::tr("The project is too large to save."));
        return std::nullopt;
    }
    plan.totalStrokeCount = validationStats.totalStrokeCount;
    plan.totalPointCount = validationStats.totalPointCount;
    plan.distinctMaskBytes = validationStats.distinctMaskBytes;

    auto impl = std::make_shared<PreparedDocument::Impl>();
    impl->document = std::move(document);
    impl->plan = std::move(plan);
    return PreparedDocument(std::move(impl));
}

DocumentSerializer::AppendStrokeResult DocumentSerializer::appendStroke(
    const PreparedDocument &base,
    const QUuid &layerId,
    const Stroke &stroke,
    SerializationCache &cache,
    qint64 maximumBytes)
{
    AppendStrokeResult result;
    if (!base.m_impl || !cache.m_impl || maximumBytes < 0
        || maximumBytes > DocumentLimits::maximumProjectBytes)
    {
        result.status = AppendStrokeStatus::Invalid;
        return result;
    }
    if (stroke.mode == StrokeMode::PixelSelection
        || stroke.mode == StrokeMode::Reframe
        || stroke.mode == StrokeMode::CompositeBoundary || stroke.fillCoverage)
    {
        return result;
    }

    const Document &baseDocument = base.m_impl->document;
    const PreparedPlan &basePlan = base.m_impl->plan;
    const Layer *baseLayer = baseDocument.layer(layerId);
    if (!baseLayer || baseLayer->kind != LayerKind::Paint
        || basePlan.totalStrokeCount != basePlan.strokes.size())
    {
        result.status = AppendStrokeStatus::Invalid;
        return result;
    }
    if (baseLayer->strokes.size() >= DocumentLimits::maximumStrokesPerLayer
        || basePlan.totalStrokeCount >= DocumentLimits::maximumTotalStrokes)
    {
        result.status = AppendStrokeStatus::StrokeLimit;
        return result;
    }
    if (stroke.points.size() > DocumentLimits::maximumPointsPerStroke
        || stroke.points.size()
               > DocumentLimits::maximumTotalPoints - basePlan.totalPointCount)
    {
        result.status = AppendStrokeStatus::PointLimit;
        return result;
    }
    if (basePlan.strokes.contains(stroke.id)
        || !isValidIncrementalStroke(stroke, baseDocument.size))
    {
        result.status = AppendStrokeStatus::Invalid;
        return result;
    }

    PreparedPlan plan = basePlan;
    Stroke frozen = freezeIncrementalStroke(stroke);
    qint64 serializedGrowth = 0;
    AppendStrokeStatus status = AppendStrokeStatus::Appended;
    if (!freezeIncrementalClipMask(stroke.clipMask,
            plan,
            *cache.m_impl,
            maximumBytes,
            serializedGrowth,
            frozen.clipMask,
            status)
        || !freezeIncrementalClipMask(stroke.fillMask,
            plan,
            *cache.m_impl,
            maximumBytes,
            serializedGrowth,
            frozen.fillMask,
            status))
    {
        result.status = status;
        return result;
    }

    ClipMaskTable clipMasks;
    if (!frozen.clipMask.isNull())
    {
        clipMasks.idByCacheKey.insert(frozen.clipMask.cacheKey(),
            plan.clipIdsByIdentity.value(frozen.clipMask.cacheKey()));
    }
    if (!frozen.fillMask.isNull())
    {
        clipMasks.idByCacheKey.insert(frozen.fillMask.cacheKey(),
            plan.clipIdsByIdentity.value(frozen.fillMask.cacheKey()));
    }
    const BinaryMaskTable binaryMasks;
    ++cache.m_impl->statistics.strokeSerializations;
    const qint64 strokeBytes =
        QJsonDocument(strokeToJson(frozen, clipMasks, binaryMasks))
            .toJson(QJsonDocument::Compact)
            .size();
    if ((!baseLayer->strokes.isEmpty()
            && !addSerializedBytes(serializedGrowth, 1, maximumBytes))
        || !addSerializedBytes(serializedGrowth, strokeBytes, maximumBytes))
    {
        result.status = AppendStrokeStatus::TooLarge;
        return result;
    }
    qint64 compactSize = basePlan.compactSize;
    if (!addSerializedBytes(compactSize, serializedGrowth, maximumBytes))
    {
        result.status = AppendStrokeStatus::TooLarge;
        return result;
    }

    const QString resolvedClipId =
        frozen.clipMask.isNull()
            ? QString()
            : plan.clipIdsByIdentity.value(frozen.clipMask.cacheKey());
    const QString resolvedFillId =
        frozen.fillMask.isNull()
            ? QString()
            : plan.clipIdsByIdentity.value(frozen.fillMask.cacheKey());
    plan.strokes.insert(frozen.id,
        StrokeMeta{frozen, resolvedClipId, resolvedFillId, {}, strokeBytes});
    plan.compactSize = compactSize;
    ++plan.totalStrokeCount;
    plan.totalPointCount += frozen.points.size();

    Document document = baseDocument;
    Layer *target = document.layer(layerId);
    if (!target)
    {
        result.status = AppendStrokeStatus::Invalid;
        return result;
    }
    target->strokes.append(frozen);

    auto impl = std::make_shared<PreparedDocument::Impl>();
    impl->document = std::move(document);
    impl->plan = std::move(plan);
    result.prepared = PreparedDocument(std::move(impl));
    result.status = AppendStrokeStatus::Appended;
    ++cache.m_impl->statistics.incrementalStrokeAppends;
    return result;
}

std::optional<DocumentSerializer::PreparedDocument>
DocumentSerializer::rebindActiveLayer(
    const PreparedDocument &prepared, const QUuid &activeLayerId)
{
    if (!prepared.m_impl)
    {
        return std::nullopt;
    }
    const Document &source = prepared.m_impl->document;
    const Layer *active = source.layer(activeLayerId);
    const bool hasPaintLayer = std::any_of(source.layers.cbegin(),
        source.layers.cend(),
        [](const Layer &layer)
        {
            return layer.kind == LayerKind::Paint;
        });
    const bool valid = hasPaintLayer
                           ? active && active->kind == LayerKind::Paint
                           : activeLayerId.isNull();
    if (!valid)
    {
        return std::nullopt;
    }
    if (source.activeLayerId == activeLayerId)
    {
        return prepared;
    }

    auto rebound = std::make_shared<PreparedDocument::Impl>(*prepared.m_impl);
    const qint64 previousRootBytes =
        QJsonDocument(rootToJson(rebound->document, {}, {}, {}))
            .toJson(QJsonDocument::Compact)
            .size();
    rebound->document.activeLayerId = activeLayerId;
    const qint64 currentRootBytes =
        QJsonDocument(rootToJson(rebound->document, {}, {}, {}))
            .toJson(QJsonDocument::Compact)
            .size();
    rebound->plan.compactSize += currentRootBytes - previousRootBytes;
    return PreparedDocument(std::move(rebound));
}

bool DocumentSerializer::save(
    const QString &filePath, const Document &document, QString *error)
{
    SerializationCache cache;
    const std::optional<PreparedDocument> prepared =
        prepare(document, cache, error);
    return prepared && save(filePath, *prepared, cache, error);
}

bool DocumentSerializer::save(const QString &filePath,
    const PreparedDocument &document,
    SerializationCache &cache,
    QString *error)
{
    if (!document.m_impl || !cache.m_impl)
    {
        setError(error,
            DocumentSerializer::tr("The project contains invalid "
                                   "operations or too much mask data."));
        return false;
    }
    if (document.m_impl->plan.compactSize <= 0
        || document.m_impl->plan.compactSize
               > DocumentLimits::maximumProjectBytes)
    {
        setError(
            error, DocumentSerializer::tr("The project is too large to save."));
        return false;
    }
    const QByteArray data = toJson(document, cache);
    if (data.isEmpty())
    {
        setError(error,
            DocumentSerializer::tr("The project contains invalid "
                                   "operations or too much mask data."));
        return false;
    }
    QSaveFile file(filePath);
    if (!file.open(QIODevice::WriteOnly))
    {
        setError(error, file.errorString());
        return false;
    }
    if (file.write(data) != data.size())
    {
        setError(error, file.errorString());
        file.cancelWriting();
        return false;
    }
    if (!file.commit())
    {
        setError(error, file.errorString());
        return false;
    }
    return true;
}

std::optional<Document> DocumentSerializer::load(
    const QString &filePath, QString *error)
{
    QFile file(filePath);
    if (!file.open(QIODevice::ReadOnly))
    {
        setError(error, file.errorString());
        return std::nullopt;
    }
    return loadFromDevice(file, DocumentLimits::maximumProjectBytes, error);
}

std::optional<Document> DocumentSerializer::loadFromDevice(
    QIODevice &device, qint64 maximumBytes, QString *error)
{
    if (maximumBytes < 0 || maximumBytes > DocumentLimits::maximumProjectBytes
        || device.size() < 0 || device.size() > maximumBytes)
    {
        setError(
            error, DocumentSerializer::tr("The project file is too large."));
        return std::nullopt;
    }
    const QByteArray data = device.read(maximumBytes + 1);
    if (data.size() > maximumBytes || !device.atEnd())
    {
        setError(
            error, DocumentSerializer::tr("The project file is too large."));
        return std::nullopt;
    }
    return fromJson(data, error);
}

QByteArray DocumentSerializer::toJson(const Document &document)
{
    return toJson(document, {});
}

QByteArray DocumentSerializer::toJson(
    const Document &document, const QJsonObject &additionalRootFields)
{
    SerializationCache cache;
    const std::optional<PreparedDocument> prepared = prepare(document, cache);
    return prepared ? toJson(*prepared, cache, additionalRootFields)
                    : QByteArray();
}

QByteArray DocumentSerializer::toJson(
    const PreparedDocument &document, SerializationCache &cache)
{
    return toJson(document, cache, {});
}

QByteArray DocumentSerializer::toJson(const PreparedDocument &document,
    SerializationCache &cache,
    const QJsonObject &additionalRootFields)
{
    if (!document.m_impl || !cache.m_impl
        || document.m_impl->plan.compactSize <= 0
        || document.m_impl->plan.compactSize
               > DocumentLimits::maximumProjectBytes)
    {
        return {};
    }
    const QJsonObject rootSkeleton =
        rootToJson(document.m_impl->document, {}, {}, {});
    for (auto field = additionalRootFields.constBegin();
        field != additionalRootFields.constEnd();
        ++field)
    {
        if (rootSkeleton.contains(field.key()))
        {
            return {};
        }
    }
    const std::optional<QByteArray> data =
        serializePreparedDocument(document.m_impl->document,
            document.m_impl->plan,
            *cache.m_impl,
            additionalRootFields);
    return data.value_or(QByteArray());
}

std::optional<Document> DocumentSerializer::fromJson(
    const QByteArray &data, QString *error)
{
    return fromJson(data, nullptr, error);
}

std::optional<Document> DocumentSerializer::fromJson(
    const QByteArray &data, QJsonObject *parsedRoot, QString *error)
{
    if (data.size() > DocumentLimits::maximumProjectBytes)
    {
        setError(
            error, DocumentSerializer::tr("The project data is too large."));
        return std::nullopt;
    }
    QJsonParseError parseError;
    const QJsonDocument json = QJsonDocument::fromJson(data, &parseError);
    if (json.isNull() || !json.isObject())
    {
        setError(error, parseError.errorString());
        return std::nullopt;
    }

    const QJsonObject root = json.object();
    const std::optional<int> fileSchemaVersion =
        integerFromJson(root.value(QStringLiteral("schemaVersion")));
    if (!fileSchemaVersion || *fileSchemaVersion < 1
        || *fileSchemaVersion > schemaVersion)
    {
        setError(error,
            DocumentSerializer::tr("This project version is not supported."));
        return std::nullopt;
    }
    const std::optional<int> fileAlgorithmVersion =
        integerFromJson(root.value(QStringLiteral("algorithmVersion")));
    if (!fileAlgorithmVersion || *fileAlgorithmVersion < 1
        || *fileAlgorithmVersion > algorithmVersion)
    {
        setError(error,
            DocumentSerializer::tr(
                "This rendering algorithm version is not supported."));
        return std::nullopt;
    }

    const QJsonValue activeLayerIdValue =
        root.value(QStringLiteral("activeLayerId"));
    if (!root.value(QStringLiteral("canvas")).isObject()
        || !root.value(QStringLiteral("animation")).isObject()
        || (!activeLayerIdValue.isString() && !activeLayerIdValue.isNull())
        || !root.value(QStringLiteral("layers")).isArray()
        || (*fileSchemaVersion >= 4
            && !root.value(QStringLiteral("clipMasks")).isArray())
        || (*fileSchemaVersion >= 6
            && !root.value(QStringLiteral("binaryMasks")).isArray())
        || (*fileSchemaVersion >= 11
            && !root.value(QStringLiteral("rasterAssets")).isArray()))
    {
        setError(error,
            DocumentSerializer::tr("The project contains invalid fields."));
        return std::nullopt;
    }
    const QJsonObject canvas = root.value(QStringLiteral("canvas")).toObject();
    const std::optional<int> width =
        integerFromJson(canvas.value(QStringLiteral("width")));
    const std::optional<int> height =
        integerFromJson(canvas.value(QStringLiteral("height")));
    if (!width || !height
        || !canvas.value(QStringLiteral("background")).isString()
        || *width < DocumentLimits::minimumCanvasEdge
        || *height < DocumentLimits::minimumCanvasEdge
        || *width > DocumentLimits::maximumCanvasEdge
        || *height > DocumentLimits::maximumCanvasEdge)
    {
        setError(error, DocumentSerializer::tr("The canvas size is invalid."));
        return std::nullopt;
    }
    const QColor background(
        canvas.value(QStringLiteral("background")).toString());
    if (!background.isValid())
    {
        setError(
            error, DocumentSerializer::tr("The canvas background is invalid."));
        return std::nullopt;
    }
    const QJsonArray layers = root.value(QStringLiteral("layers")).toArray();
    if (layers.size() > DocumentLimits::maximumLayers)
    {
        setError(error, DocumentSerializer::tr("The layer count is invalid."));
        return std::nullopt;
    }
    if (!validateCollectionBudgets(layers, error))
    {
        return std::nullopt;
    }

    quint64 remainingAssetBytes = DocumentLimits::maximumDistinctClipMaskBytes;
    QHash<QString, QImage> referencedMasks;
    if (*fileSchemaVersion >= 4)
    {
        const std::optional<QHash<QString, QImage>> parsedMasks =
            clipMaskTableFromJson(root.value(QStringLiteral("clipMasks")),
                QSize(*width, *height),
                *fileSchemaVersion <= 5,
                remainingAssetBytes,
                error);
        if (!parsedMasks)
        {
            return std::nullopt;
        }
        referencedMasks = *parsedMasks;
    }
    QHash<QString, PackedMaskRegion> referencedBinaryMasks;
    if (*fileSchemaVersion >= 6)
    {
        const std::optional<QHash<QString, PackedMaskRegion>> parsedMasks =
            binaryMaskTableFromJson(root.value(QStringLiteral("binaryMasks")),
                remainingAssetBytes,
                error);
        if (!parsedMasks)
        {
            return std::nullopt;
        }
        referencedBinaryMasks = *parsedMasks;
    }
    QMap<QString, RasterAsset> rasterAssets;
    if (*fileSchemaVersion >= 11)
    {
        RasterAssetTable table;
        const QJsonArray entries =
            root.value(QStringLiteral("rasterAssets")).toArray();
        for (const auto &entryValue : entries)
        {
            if (!entryValue.isObject())
            {
                setError(error,
                    DocumentSerializer::tr(
                        "The project contains an invalid raster asset table."));
                return std::nullopt;
            }
            const QJsonObject entry = entryValue.toObject();
            const QString id = entry.value(QStringLiteral("id")).toString();
            const std::optional<QSize> size =
                sizeFromJsonArray(entry.value(QStringLiteral("size")));
            if (!entry.value(QStringLiteral("id")).isString() || !size
                || !entry.value(QStringLiteral("data")).isString()
                || rasterAssets.contains(id))
            {
                setError(error,
                    DocumentSerializer::tr(
                        "The project contains an invalid raster asset table."));
                return std::nullopt;
            }
            const QByteArray compressed = QByteArray::fromBase64(
                entry.value(QStringLiteral("data")).toString().toLatin1());
            const RasterAssetRegistrationResult result =
                table.registerPayload(id, *size, compressed);
            if (result.status != RasterAssetRegistrationStatus::Registered)
            {
                setError(error,
                    DocumentSerializer::tr(
                        "The project contains an invalid raster asset."));
                return std::nullopt;
            }
            rasterAssets.insert(id, RasterAsset{id, *size, compressed});
        }
    }

    const QJsonObject animation =
        root.value(QStringLiteral("animation")).toObject();
    const std::optional<int> frames =
        integerFromJson(animation.value(QStringLiteral("frames")));
    if (!frames || !animation.value(QStringLiteral("fps")).isDouble()
        || !animation.value(QStringLiteral("wobble")).isDouble())
    {
        setError(error,
            DocumentSerializer::tr("The animation settings are invalid."));
        return std::nullopt;
    }
    const qreal framesPerSecond =
        animation.value(QStringLiteral("fps")).toDouble();
    const qreal wobbleAmount =
        animation.value(QStringLiteral("wobble")).toDouble();
    MotionSettings motion;
    if (*fileSchemaVersion >= 10)
    {
        const std::optional<MotionSettings> parsedMotion =
            motionSettingsFromJson(
                animation.value(QStringLiteral("motion")), error);
        if (!parsedMotion)
        {
            return std::nullopt;
        }
        motion = *parsedMotion;
    }
    else
    {
        motion.poseCount = std::min(motion.poseCount, *frames);
    }
    if (*frames < DocumentLimits::minimumAnimationFrames
        || *frames > DocumentLimits::maximumAnimationFrames
        || !std::isfinite(framesPerSecond)
        || framesPerSecond < DocumentLimits::minimumFramesPerSecond
        || framesPerSecond > DocumentLimits::maximumFramesPerSecond
        || !std::isfinite(wobbleAmount)
        || wobbleAmount < DocumentLimits::minimumWobbleAmount
        || wobbleAmount > DocumentLimits::maximumWobbleAmount
        || !isValidMotionSettings(motion, *frames))
    {
        setError(error,
            DocumentSerializer::tr("The animation settings are invalid."));
        return std::nullopt;
    }

    Document document;
    document.size = QSize(*width, *height);
    document.background = background;
    document.animationFrames = *frames;
    document.framesPerSecond = framesPerSecond;
    document.wobbleAmount = wobbleAmount;
    document.motion = motion;
    document.rasterAssets = std::move(rasterAssets);
    document.layers.reserve(layers.size());
    QSet<QUuid> layerIds;
    QSet<QUuid> strokeIds;
    qsizetype totalPoints = 0;
    QHash<QByteArray, QImage> maskCache;
    quint64 distinctMaskBytes = 0;

    for (const auto &layerValue : layers)
    {
        const std::optional<Layer> layer = layerFromJson(layerValue,
            *fileSchemaVersion,
            QSize(*width, *height),
            maskCache,
            referencedMasks,
            referencedBinaryMasks,
            distinctMaskBytes,
            error);
        if (!layer)
        {
            return std::nullopt;
        }
        if (layerIds.contains(layer->id))
        {
            setError(error,
                DocumentSerializer::tr(
                    "The project contains duplicate layer IDs."));
            return std::nullopt;
        }
        layerIds.insert(layer->id);
        for (const Stroke &stroke : layer->strokes)
        {
            if (stroke.points.size()
                > DocumentLimits::maximumTotalPoints - totalPoints)
            {
                setError(error,
                    DocumentSerializer::tr(
                        "The project contains too many points."));
                return std::nullopt;
            }
            totalPoints += stroke.points.size();
            if (strokeIds.contains(stroke.id))
            {
                setError(error,
                    DocumentSerializer::tr(
                        "The project contains duplicate stroke IDs."));
                return std::nullopt;
            }
            strokeIds.insert(stroke.id);
        }
        document.layers.append(*layer);
    }

    document.activeLayerId = activeLayerIdValue.isNull()
                                 ? QUuid()
                                 : QUuid(activeLayerIdValue.toString());
    if (activeLayerIdValue.isString() && document.activeLayerId.isNull())
    {
        setError(
            error, DocumentSerializer::tr("The active layer ID is invalid."));
        return std::nullopt;
    }
    if (!validateDocument(document, *fileSchemaVersion, error))
    {
        return std::nullopt;
    }
    if (!DocumentOperations::normalizeAndValidate(document))
    {
        setError(error,
            DocumentSerializer::tr("The project contains invalid "
                                   "operations or too much mask data."));
        return std::nullopt;
    }
    if (!validateDocument(document, schemaVersion, error))
    {
        return std::nullopt;
    }
    if (parsedRoot)
    {
        *parsedRoot = root;
    }
    return document;
}

}

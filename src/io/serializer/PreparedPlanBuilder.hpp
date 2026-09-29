// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#pragma once

#include "document/Document.hpp"
#include "document/DocumentLimits.hpp"
#include "io/DocumentSerializer.hpp"
#include "io/serializer/DocumentJsonCodec.hpp"
#include "io/serializer/MaskAssetTable.hpp"
#include "io/serializer/SerializerSchema.hpp"

#include <QHash>
#include <QJsonObject>
#include <QString>
#include <QVector>

#include <optional>

namespace ugurugu
{
namespace serializer_detail
{

// Turns a caller-owned document into the immutable content plus exact byte
// plan that a PreparedDocument holds.
//
// Two invariants run through everything here. Prepared content must not alias
// any buffer the caller can still write to, which is what DocumentFreezer
// enforces; and the recorded compactSize must equal the byte length the
// writer will actually produce, which is what the size planning below
// enforces. Breaking either one is not a size error — it lets history replay
// or a save observe a document nobody committed.

bool addSerializedBytes(qint64 &total, qint64 amount, qint64 maximumBytes);

// Identity comparisons below are backing-address equality, not content
// equality: they answer "is this still the same allocation the prepared base
// owns", which is the only question that makes reuse safe.
bool sameImageIdentity(const QImage &left, const QImage &right);
bool samePackedIdentity(const QByteArray &left, const QByteArray &right);
bool samePixelSelectionIdentity(const std::optional<PixelSelectionOp> &left,
    const std::optional<PixelSelectionOp> &right);
bool sameStrokeIdentity(const Stroke &left, const Stroke &right);

QString backingIdentity(const void *data, qsizetype size);

void rememberImmutableImage(ImmutableBackings &backings, const QImage &image);
void rememberImmutableBytes(
    ImmutableBackings &backings, const QByteArray &bytes);
void rememberImmutablePoints(
    ImmutableBackings &backings, const QVector<StrokePoint> &points);

ImmutableBackings immutableBackingsFromPlan(const PreparedPlan *base);

bool sameStrokeVectorBacking(
    const QVector<Stroke> &left, const QVector<Stroke> &right);

QString owningStringCopy(const QString &source);

bool isValidIncrementalStroke(const Stroke &stroke, const QSize &canvasSize);
QVector<StrokePoint> freezeIncrementalPoints(
    const QVector<StrokePoint> &source);
Stroke freezeIncrementalStroke(const Stroke &source);

QString clipMaskId(const QImage &mask, const ClipMaskTable &table);
QString binaryMaskId(const Stroke &stroke, const BinaryMaskTable &table);

class DocumentFreezer final
{
public:
    explicit DocumentFreezer(
        const PreparedPlan *base, const ImmutableBackings *trusted = nullptr)
        : m_reusable(immutableBackingsFromPlan(base))
    {
        if (!trusted)
        {
            return;
        }
        for (auto entry = trusted->images.cbegin();
            entry != trusted->images.cend();
            ++entry)
        {
            m_reusable.images.insert(entry.key(), entry.value());
        }
        for (auto entry = trusted->byteArrays.cbegin();
            entry != trusted->byteArrays.cend();
            ++entry)
        {
            m_reusable.byteArrays.insert(entry.key(), entry.value());
        }
        for (auto entry = trusted->pointVectors.cbegin();
            entry != trusted->pointVectors.cend();
            ++entry)
        {
            m_reusable.pointVectors.insert(entry.key(), entry.value());
        }
    }

    Document freeze(const Document &source)
    {
        Document frozen;
        frozen.size = source.size;
        frozen.background = source.background;
        frozen.animationFrames = source.animationFrames;
        frozen.framesPerSecond = source.framesPerSecond;
        frozen.wobbleAmount = source.wobbleAmount;
        frozen.motion = source.motion;
        for (auto asset = source.rasterAssets.cbegin();
            asset != source.rasterAssets.cend();
            ++asset)
        {
            RasterAsset frozenAsset;
            frozenAsset.id = owningCopy(asset->id);
            frozenAsset.size = asset->size;
            frozenAsset.compressedRgba = freezeBytes(asset->compressedRgba);
            frozen.rasterAssets.insert(owningCopy(asset.key()), frozenAsset);
        }
        frozen.activeLayerId = source.activeLayerId;
        frozen.layers.reserve(source.layers.size());
        for (const Layer &layer : source.layers)
        {
            frozen.layers.append(freezeLayer(layer));
        }
        return frozen;
    }

private:
    static QString owningCopy(const QString &source)
    {
        if (source.isNull())
        {
            return {};
        }
        return QString(source.constData(), source.size());
    }

    QImage freezeImage(const QImage &source)
    {
        if (source.isNull())
        {
            return {};
        }
        const qint64 identity = source.cacheKey();
        const auto reusable = m_reusable.images.constFind(identity);
        if (reusable != m_reusable.images.cend()
            && sameImageIdentity(source, reusable.value()))
        {
            return reusable.value();
        }
        const auto alreadyFrozen = m_frozen.images.constFind(identity);
        if (alreadyFrozen != m_frozen.images.cend()
            && alreadyFrozen->size() == source.size()
            && alreadyFrozen->format() == source.format())
        {
            return alreadyFrozen.value();
        }
        QImage frozen = source.copy();
        m_frozen.images.insert(identity, frozen);
        return frozen;
    }

    QByteArray freezeBytes(const QByteArray &source)
    {
        if (source.isEmpty())
        {
            return source;
        }
        const QString identity =
            backingIdentity(source.constData(), source.size());
        const auto reusable = m_reusable.byteArrays.constFind(identity);
        if (reusable != m_reusable.byteArrays.cend()
            && samePackedIdentity(source, reusable.value()))
        {
            return reusable.value();
        }
        const auto alreadyFrozen = m_frozen.byteArrays.constFind(identity);
        if (alreadyFrozen != m_frozen.byteArrays.cend())
        {
            return alreadyFrozen.value();
        }
        const QByteArray frozen(source.constData(), source.size());
        m_frozen.byteArrays.insert(identity, frozen);
        return frozen;
    }

    QVector<StrokePoint> freezePoints(const QVector<StrokePoint> &source)
    {
        if (source.isEmpty())
        {
            return {};
        }
        const QString identity =
            backingIdentity(source.constData(), source.size());
        const auto reusable = m_reusable.pointVectors.constFind(identity);
        if (reusable != m_reusable.pointVectors.cend()
            && reusable->size() == source.size())
        {
            return reusable.value();
        }
        const auto alreadyFrozen = m_frozen.pointVectors.constFind(identity);
        if (alreadyFrozen != m_frozen.pointVectors.cend())
        {
            return alreadyFrozen.value();
        }
        QVector<StrokePoint> frozen;
        frozen.reserve(source.size());
        for (const StrokePoint &point : source)
        {
            frozen.append(point);
        }
        m_frozen.pointVectors.insert(identity, frozen);
        return frozen;
    }

    Stroke freezeStroke(const Stroke &source)
    {
        Stroke frozen;
        frozen.id = source.id;
        frozen.seed = source.seed;
        frozen.mode = source.mode;
        frozen.color = source.color;
        frozen.width = source.width;
        frozen.brush = source.brush;
        frozen.points = freezePoints(source.points);
        frozen.visibilityClip = source.visibilityClip;
        frozen.clipMask = freezeImage(source.clipMask);
        frozen.fillMask = freezeImage(source.fillMask);
        if (source.fillCoverage)
        {
            frozen.fillCoverage = *source.fillCoverage;
            frozen.fillCoverage->packedMask =
                freezeBytes(source.fillCoverage->packedMask);
        }
        else
        {
            frozen.fillCoverage.reset();
        }
        if (source.pixelSelectionOp)
        {
            PixelSelectionOp operation = *source.pixelSelectionOp;
            operation.packedMask = freezeBytes(operation.packedMask);
            frozen.pixelSelectionOp = std::move(operation);
        }
        else
        {
            frozen.pixelSelectionOp.reset();
        }
        frozen.reframeOp = source.reframeOp;
        if (source.imageOp)
        {
            frozen.imageOp = *source.imageOp;
            frozen.imageOp->assetId = owningCopy(source.imageOp->assetId);
        }
        return frozen;
    }

    Layer freezeLayer(const Layer &source)
    {
        Layer frozen;
        frozen.id = source.id;
        frozen.name = owningCopy(source.name);
        frozen.kind = source.kind;
        frozen.parentGroupId = source.parentGroupId;
        frozen.clipToLayerBelow = source.clipToLayerBelow;
        frozen.visible = source.visible;
        frozen.reference = source.reference;
        frozen.opacity = source.opacity;
        frozen.blendMode = source.blendMode;
        frozen.wobbleAmount = source.wobbleAmount;
        frozen.motion = source.motion;
        frozen.initialCanvasSize = source.initialCanvasSize;
        frozen.strokes.reserve(source.strokes.size());
        for (const Stroke &stroke : source.strokes)
        {
            frozen.strokes.append(freezeStroke(stroke));
        }
        return frozen;
    }

    ImmutableBackings m_reusable;
    ImmutableBackings m_frozen;
};

enum class MetadataReuseStatus
{
    NotApplicable,
    Invalid,
    TooLarge,
    Success
};

struct MetadataReuseResult
{
    MetadataReuseStatus status = MetadataReuseStatus::NotApplicable;
    Document document;
    PreparedPlan plan;
};

// Fast path for edits that change only layer metadata: reuses the base plan's
// frozen content instead of re-freezing every stroke. Returns NotApplicable
// when anything but metadata differs, and the caller must then take the full
// freeze path.
MetadataReuseResult reusePreparedContentForMetadataEdit(const Document &source,
    const Document &baseDocument,
    const PreparedPlan &basePlan,
    qint64 maximumBytes,
    QString *error);

template <typename Cache>
bool freezeIncrementalClipMask(const QImage &source,
    PreparedPlan &plan,
    Cache &cache,
    qint64 maximumBytes,
    qint64 &serializedGrowth,
    QImage &frozen,
    DocumentSerializer::AppendStrokeStatus &status)
{
    if (source.isNull())
    {
        frozen = {};
        return true;
    }
    const auto knownId = plan.clipIdsByIdentity.constFind(source.cacheKey());
    if (knownId != plan.clipIdsByIdentity.cend())
    {
        const auto asset = plan.clipAssets.constFind(knownId.value());
        if (asset == plan.clipAssets.cend()
            || asset->image.size() != source.size()
            || asset->image.format() != source.format())
        {
            status = DocumentSerializer::AppendStrokeStatus::Invalid;
            return false;
        }
        frozen = asset->image;
        plan.clipIdsByIdentity.insert(frozen.cacheKey(), knownId.value());
        return true;
    }

    const QByteArray bytes = canonicalMaskBytes(source);
    if (bytes.isEmpty())
    {
        status = DocumentSerializer::AppendStrokeStatus::Invalid;
        return false;
    }
    ++cache.statistics.clipMaskContentHashes;
    const QString id = maskContentId(source.width(), source.height(), bytes);
    const auto existing = plan.clipAssets.constFind(id);
    if (existing != plan.clipAssets.cend())
    {
        if (existing->image.size() != source.size()
            || existing->image != source)
        {
            status = DocumentSerializer::AppendStrokeStatus::Invalid;
            return false;
        }
        frozen = existing->image;
        plan.clipIdsByIdentity.insert(frozen.cacheKey(), id);
        return true;
    }

    const quint64 maskBytes = source.sizeInBytes();
    if (maskBytes
        > DocumentLimits::maximumDistinctClipMaskBytes - plan.distinctMaskBytes)
    {
        status = DocumentSerializer::AppendStrokeStatus::MaskLimit;
        return false;
    }
    const QByteArray compressed = compressedPayload(cache, false, id, bytes);
    frozen = source.copy();
    if (compressed.isEmpty() || frozen.isNull())
    {
        status = DocumentSerializer::AppendStrokeStatus::Invalid;
        return false;
    }
    const qint64 entryBytes =
        serializedClipMaskSize(id, frozen, compressed.size());
    if (entryBytes < 0)
    {
        status = DocumentSerializer::AppendStrokeStatus::TooLarge;
        return false;
    }
    qint64 assetGrowth = entryBytes;
    if (!plan.clipAssets.isEmpty())
    {
        ++assetGrowth;
    }
    if (!addSerializedBytes(serializedGrowth, assetGrowth, maximumBytes))
    {
        status = DocumentSerializer::AppendStrokeStatus::TooLarge;
        return false;
    }
    plan.clipAssets.insert(
        id, ClipAssetMeta{id, frozen, entryBytes, compressed.size()});
    plan.clipIdsByIdentity.insert(frozen.cacheKey(), id);
    plan.distinctMaskBytes += maskBytes;
    return true;
}

template <typename Cache>
bool serializedSizeWithinLimit(const Document &document,
    const ClipMaskTable &clipMasks,
    const BinaryMaskTable &binaryMasks,
    const PreparedPlan *base,
    PreparedPlan &plan,
    Cache &cache,
    qint64 maximumBytes)
{
    for (auto entry = clipMasks.entries.cbegin();
        entry != clipMasks.entries.cend();
        ++entry)
    {
        plan.clipAssets.insert(entry.key(),
            ClipAssetMeta{entry->id,
                entry->image,
                entry->serializedBytes,
                entry->compressedBytes});
    }
    plan.clipIdsByIdentity = clipMasks.idByCacheKey;
    for (auto entry = binaryMasks.entries.cbegin();
        entry != binaryMasks.entries.cend();
        ++entry)
    {
        plan.binaryAssets.insert(entry.key(),
            BinaryAssetMeta{entry->id,
                entry->region,
                entry->serializedBytes,
                entry->compressedBytes});
    }
    plan.binaryIdsByIdentity = binaryMasks.idByIdentity;
    plan.rasterAssets = document.rasterAssets;

    qint64 serializedLayerBytes = 0;
    for (const Layer &layer : document.layers)
    {
        qint64 serializedStrokeBytes = 0;
        for (const Stroke &stroke : layer.strokes)
        {
            const QString resolvedClipId =
                clipMaskId(stroke.clipMask, clipMasks);
            const QString resolvedFillId =
                clipMaskId(stroke.fillMask, clipMasks);
            const QString resolvedBinaryId = binaryMaskId(stroke, binaryMasks);
            qint64 bytes = -1;
            if (base)
            {
                const auto cached = base->strokes.constFind(stroke.id);
                if (cached != base->strokes.cend()
                    && cached->clipMaskId == resolvedClipId
                    && cached->fillMaskId == resolvedFillId
                    && cached->binaryMaskId == resolvedBinaryId
                    && sameStrokeIdentity(stroke, cached->snapshot))
                {
                    bytes = cached->serializedBytes;
                }
            }
            if (bytes < 0)
            {
                ++cache.statistics.strokeSerializations;
                bytes =
                    QJsonDocument(strokeToJson(stroke, clipMasks, binaryMasks))
                        .toJson(QJsonDocument::Compact)
                        .size();
            }
            plan.strokes.insert(stroke.id,
                StrokeMeta{stroke,
                    resolvedClipId,
                    resolvedFillId,
                    resolvedBinaryId,
                    bytes});
            if (!addSerializedBytes(serializedStrokeBytes, bytes, maximumBytes))
            {
                return false;
            }
        }
        const qint64 strokeArrayBytes =
            2 + serializedStrokeBytes
            + std::max<qint64>(0, layer.strokes.size() - 1);
        const qint64 skeletonBytes = QJsonDocument(layerSkeletonToJson(layer))
                                         .toJson(QJsonDocument::Compact)
                                         .size();
        const qint64 layerBytes = skeletonBytes - 2 + strokeArrayBytes;
        if (!addSerializedBytes(serializedLayerBytes, layerBytes, maximumBytes))
        {
            return false;
        }
    }

    const qint64 layerArrayBytes =
        2 + serializedLayerBytes
        + std::max<qint64>(0, document.layers.size() - 1);
    const qint64 maskArrayBytes =
        2 + clipMasks.serializedEntryBytes
        + std::max<qint64>(0, clipMasks.entries.size() - 1);
    const qint64 binaryMaskArrayBytes =
        2 + binaryMasks.serializedEntryBytes
        + std::max<qint64>(0, binaryMasks.entries.size() - 1);
    const qint64 rootSkeletonBytes =
        QJsonDocument(rootToJson(document, {}, {}, {}))
            .toJson(QJsonDocument::Compact)
            .size();
    qint64 total = rootSkeletonBytes - 6;
    const bool withinLimit =
        addSerializedBytes(total, layerArrayBytes, maximumBytes)
        && addSerializedBytes(total, maskArrayBytes, maximumBytes)
        && addSerializedBytes(total, binaryMaskArrayBytes, maximumBytes);
    if (withinLimit)
    {
        plan.compactSize = total;
    }
    return withinLimit;
}

template <typename Cache>
bool tablesMatchPlan(const ClipMaskTable &clipMasks,
    const BinaryMaskTable &binaryMasks,
    const PreparedPlan &plan,
    Cache &)
{
    if (clipMasks.entries.size() != plan.clipAssets.size()
        || binaryMasks.entries.size() != plan.binaryAssets.size()
        || clipMasks.idByCacheKey != plan.clipIdsByIdentity
        || binaryMasks.idByIdentity != plan.binaryIdsByIdentity)
    {
        return false;
    }
    for (auto entry = clipMasks.entries.cbegin();
        entry != clipMasks.entries.cend();
        ++entry)
    {
        const auto expected = plan.clipAssets.constFind(entry.key());
        if (expected == plan.clipAssets.cend() || entry->id != expected->id
            || !sameImageIdentity(entry->image, expected->image)
            || entry->serializedBytes != expected->serializedEntryBytes
            || entry->compressedBytes != expected->compressedBytes
            || entry->compressedData.isEmpty())
        {
            return false;
        }
    }
    for (auto entry = binaryMasks.entries.cbegin();
        entry != binaryMasks.entries.cend();
        ++entry)
    {
        const auto expected = plan.binaryAssets.constFind(entry.key());
        if (expected == plan.binaryAssets.cend() || entry->id != expected->id
            || entry->region != expected->region
            || entry->serializedBytes != expected->serializedEntryBytes
            || entry->compressedBytes != expected->compressedBytes
            || entry->compressedData.isEmpty())
        {
            return false;
        }
    }
    return true;
}

template <typename Cache>
std::optional<QByteArray> serializePreparedDocument(const Document &document,
    const PreparedPlan &plan,
    Cache &cache,
    const QJsonObject &additionalRootFields = {})
{
    if (plan.compactSize <= 0
        || plan.compactSize > DocumentLimits::maximumProjectBytes)
    {
        return std::nullopt;
    }
    qint64 expectedSize = plan.compactSize;
    if (!additionalRootFields.isEmpty())
    {
        const qint64 baseRootSize =
            QJsonDocument(rootToJson(document, {}, {}, {}))
                .toJson(QJsonDocument::Compact)
                .size();
        const qint64 extendedRootSize = QJsonDocument(
            rootToJson(document, {}, {}, {}, additionalRootFields))
                                            .toJson(QJsonDocument::Compact)
                                            .size();
        const qint64 additionalBytes = extendedRootSize - baseRootSize;
        if (additionalBytes < 0
            || expectedSize
                   > DocumentLimits::maximumProjectBytes - additionalBytes)
        {
            return std::nullopt;
        }
        expectedSize += additionalBytes;
    }
    const ClipMaskTable clipMasks =
        buildClipMaskTable(document, &plan, cache, plan.compactSize, true);
    const BinaryMaskTable binaryMasks =
        buildBinaryMaskTable(document, &plan, cache, plan.compactSize, true);
    if (clipMasks.invalid || clipMasks.tooLarge || binaryMasks.invalid
        || binaryMasks.tooLarge
        || !tablesMatchPlan(clipMasks, binaryMasks, plan, cache))
    {
        return std::nullopt;
    }
    QJsonArray layers;
    for (const Layer &layer : document.layers)
    {
        layers.append(layerToJson(layer, clipMasks, binaryMasks));
    }
    QJsonArray masks;
    for (const SerializedClipMask &entry : clipMasks.entries)
    {
        masks.append(serializedClipMaskToJson(entry));
    }
    QJsonArray packedMasks;
    for (const SerializedBinaryMask &entry : binaryMasks.entries)
    {
        packedMasks.append(serializedBinaryMaskToJson(entry));
    }
    QByteArray data = QJsonDocument(
        rootToJson(document, layers, masks, packedMasks, additionalRootFields))
                          .toJson(QJsonDocument::Compact);
    if (data.size() != expectedSize)
    {
        return std::nullopt;
    }
    if (data.size() > DocumentLimits::maximumProjectBytes)
    {
        return std::nullopt;
    }
    return data;
}

}

}

// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "io/serializer/DocumentJsonCodec.hpp"

// Messages keep using DocumentSerializer::tr so their translation context
// stays ugurugu::DocumentSerializer, matching the existing i18n catalogues.
#include "document/DocumentLimits.hpp"
#include "document/SelectionOperation.hpp"
#include "io/DocumentSerializer.hpp"
#include "io/serializer/BoundedCompression.hpp"

#include <QCryptographicHash>

#include <algorithm>
#include <cmath>
#include <limits>

namespace ugurugu
{
namespace serializer_detail
{

QJsonArray pointToJson(const StrokePoint &point)
{
    return {point.position.x(), point.position.y(), point.pressure};
}

std::optional<StrokePoint> pointFromJson(const QJsonValue &value)
{
    if (!value.isArray())
    {
        return std::nullopt;
    }
    const QJsonArray array = value.toArray();
    if ((array.size() != 2 && array.size() != 3) || !array[0].isDouble()
        || !array[1].isDouble() || (array.size() == 3 && !array[2].isDouble()))
    {
        return std::nullopt;
    }
    StrokePoint point;
    point.position = QPointF(array[0].toDouble(), array[1].toDouble());
    point.pressure = array.size() == 3 ? array[2].toDouble() : 1.0;
    if (!std::isfinite(point.position.x()) || !std::isfinite(point.position.y())
        || !std::isfinite(point.pressure) || point.pressure < 0.0
        || point.pressure > 1.0)
    {
        return std::nullopt;
    }
    return point;
}

QJsonObject brushToJson(const BrushSettings &brush)
{
    QString engineName = QStringLiteral("line");
    if (brush.engine == BrushEngine::Airbrush)
    {
        engineName = QStringLiteral("airbrush");
    }
    else if (brush.engine == BrushEngine::Spray)
    {
        engineName = QStringLiteral("spray");
    }
    const QString tipName = brush.tipShape == BrushTipShape::Square
                                ? QStringLiteral("square")
                                : QStringLiteral("round");

    QJsonObject object;
    object.insert(QStringLiteral("engine"), engineName);
    object.insert(QStringLiteral("tip"), tipName);
    object.insert(QStringLiteral("opacity"), brush.opacity);
    object.insert(QStringLiteral("flow"), brush.flow);
    object.insert(QStringLiteral("hardness"), brush.hardness);
    object.insert(QStringLiteral("spacing"), brush.spacing);
    object.insert(QStringLiteral("scatter"), brush.scatter);
    object.insert(QStringLiteral("particleSize"), brush.particleSize);
    object.insert(QStringLiteral("density"), brush.density);
    object.insert(QStringLiteral("sizeDynamics"), brush.sizeDynamics);
    object.insert(QStringLiteral("opacityDynamics"), brush.opacityDynamics);
    object.insert(QStringLiteral("sizeJitter"), brush.sizeJitter);
    object.insert(QStringLiteral("animatedJitter"), brush.animatedJitter);
    object.insert(QStringLiteral("wobbleScale"), brush.wobbleScale);
    object.insert(QStringLiteral("antialiasing"), brush.antialiasing);
    return object;
}

std::optional<BrushSettings> brushFromJson(
    const QJsonValue &value, QString *error)
{
    if (!value.isObject())
    {
        setError(error,
            DocumentSerializer::tr("A stroke has invalid brush settings."));
        return std::nullopt;
    }
    const QJsonObject object = value.toObject();
    if (!object.value(QStringLiteral("engine")).isString()
        || !object.value(QStringLiteral("tip")).isString()
        || !object.value(QStringLiteral("opacity")).isDouble()
        || !object.value(QStringLiteral("flow")).isDouble()
        || !object.value(QStringLiteral("hardness")).isDouble()
        || !object.value(QStringLiteral("spacing")).isDouble()
        || !object.value(QStringLiteral("scatter")).isDouble()
        || !object.value(QStringLiteral("particleSize")).isDouble()
        || !object.value(QStringLiteral("density")).isDouble()
        || !object.value(QStringLiteral("sizeDynamics")).isDouble()
        || !object.value(QStringLiteral("opacityDynamics")).isDouble()
        || !object.value(QStringLiteral("sizeJitter")).isDouble()
        || !object.value(QStringLiteral("animatedJitter")).isBool())
    {
        setError(error,
            DocumentSerializer::tr("A stroke has invalid brush settings."));
        return std::nullopt;
    }
    const QJsonValue wobbleScaleValue =
        object.value(QStringLiteral("wobbleScale"));
    if (!wobbleScaleValue.isUndefined() && !wobbleScaleValue.isDouble())
    {
        setError(error,
            DocumentSerializer::tr("A stroke has invalid brush settings."));
        return std::nullopt;
    }
    const QJsonValue antialiasingValue =
        object.value(QStringLiteral("antialiasing"));
    if (!antialiasingValue.isUndefined() && !antialiasingValue.isBool())
    {
        setError(error,
            DocumentSerializer::tr("A stroke has invalid brush settings."));
        return std::nullopt;
    }

    BrushSettings brush;
    const QString engine = object.value(QStringLiteral("engine")).toString();
    if (engine == QStringLiteral("airbrush"))
    {
        brush.engine = BrushEngine::Airbrush;
    }
    else if (engine == QStringLiteral("spray"))
    {
        brush.engine = BrushEngine::Spray;
    }
    else if (engine != QStringLiteral("line"))
    {
        setError(error,
            DocumentSerializer::tr("A stroke has an invalid brush engine."));
        return std::nullopt;
    }
    const QString tip = object.value(QStringLiteral("tip")).toString();
    if (tip == QStringLiteral("square"))
    {
        brush.tipShape = BrushTipShape::Square;
    }
    else if (tip != QStringLiteral("round"))
    {
        setError(error,
            DocumentSerializer::tr("A stroke has an invalid brush tip."));
        return std::nullopt;
    }
    brush.opacity = object.value(QStringLiteral("opacity")).toDouble();
    brush.flow = object.value(QStringLiteral("flow")).toDouble();
    brush.hardness = object.value(QStringLiteral("hardness")).toDouble();
    brush.spacing = object.value(QStringLiteral("spacing")).toDouble();
    brush.scatter = object.value(QStringLiteral("scatter")).toDouble();
    brush.particleSize =
        object.value(QStringLiteral("particleSize")).toDouble();
    brush.density = object.value(QStringLiteral("density")).toDouble();
    brush.sizeDynamics =
        object.value(QStringLiteral("sizeDynamics")).toDouble();
    brush.opacityDynamics =
        object.value(QStringLiteral("opacityDynamics")).toDouble();
    brush.sizeJitter = object.value(QStringLiteral("sizeJitter")).toDouble();
    brush.animatedJitter =
        object.value(QStringLiteral("animatedJitter")).toBool();
    brush.wobbleScale =
        wobbleScaleValue.isDouble() ? wobbleScaleValue.toDouble() : 1.0;
    brush.antialiasing = antialiasingValue.toBool();
    if (!isValidBrushSettings(brush))
    {
        setError(error,
            DocumentSerializer::tr("A stroke has invalid brush settings."));
        return std::nullopt;
    }
    return brush;
}

QString samplingModeName(SamplingMode sampling)
{
    return sampling == SamplingMode::Smooth ? QStringLiteral("smooth")
                                            : QStringLiteral("nearest");
}

QString layerBlendModeName(LayerBlendMode mode)
{
    switch (mode)
    {
    case LayerBlendMode::Multiply:
        return QStringLiteral("multiply");
    case LayerBlendMode::Screen:
        return QStringLiteral("screen");
    case LayerBlendMode::Overlay:
        return QStringLiteral("overlay");
    case LayerBlendMode::Normal:
        return QStringLiteral("normal");
    }
    return {};
}

QString layerKindName(LayerKind kind)
{
    return kind == LayerKind::Group ? QStringLiteral("group")
                                    : QStringLiteral("paint");
}

QJsonObject motionSettingsToJson(const MotionSettings &settings)
{
    QString style = QStringLiteral("classic");
    if (settings.style == MotionStyle::Smooth)
    {
        style = QStringLiteral("smooth");
    }
    else if (settings.style == MotionStyle::Stepped)
    {
        style = QStringLiteral("stepped");
    }
    QJsonObject object;
    object.insert(QStringLiteral("style"), style);
    object.insert(QStringLiteral("poseCount"), settings.poseCount);
    object.insert(QStringLiteral("detail"), settings.detail);
    object.insert(QStringLiteral("linked"), settings.linked);
    object.insert(QStringLiteral("randomness"), settings.randomness);
    object.insert(QStringLiteral("brokenLine"), settings.brokenLine);
    object.insert(QStringLiteral("breakAmount"), settings.breakAmount);
    object.insert(QStringLiteral("breakRange"), settings.breakRange);
    return object;
}

std::optional<MotionSettings> motionSettingsFromJson(
    const QJsonValue &value, QString *error)
{
    if (!value.isObject())
    {
        setError(
            error, DocumentSerializer::tr("The motion settings are invalid."));
        return std::nullopt;
    }
    const QJsonObject object = value.toObject();
    const std::optional<int> poseCount =
        integerFromJson(object.value(QStringLiteral("poseCount")));
    const std::optional<int> detail =
        integerFromJson(object.value(QStringLiteral("detail")));
    if (!object.value(QStringLiteral("style")).isString() || !poseCount
        || !detail || !object.value(QStringLiteral("linked")).isDouble()
        || !object.value(QStringLiteral("randomness")).isDouble()
        || !object.value(QStringLiteral("brokenLine")).isBool()
        || !object.value(QStringLiteral("breakAmount")).isDouble()
        || !object.value(QStringLiteral("breakRange")).isDouble())
    {
        setError(
            error, DocumentSerializer::tr("The motion settings are invalid."));
        return std::nullopt;
    }

    MotionSettings settings;
    const QString style = object.value(QStringLiteral("style")).toString();
    if (style == QStringLiteral("smooth"))
    {
        settings.style = MotionStyle::Smooth;
    }
    else if (style == QStringLiteral("stepped"))
    {
        settings.style = MotionStyle::Stepped;
    }
    else if (style != QStringLiteral("classic"))
    {
        setError(
            error, DocumentSerializer::tr("The motion settings are invalid."));
        return std::nullopt;
    }
    settings.poseCount = *poseCount;
    settings.detail = *detail;
    settings.linked = object.value(QStringLiteral("linked")).toDouble();
    settings.randomness = object.value(QStringLiteral("randomness")).toDouble();
    settings.brokenLine = object.value(QStringLiteral("brokenLine")).toBool();
    settings.breakAmount =
        object.value(QStringLiteral("breakAmount")).toDouble();
    settings.breakRange = object.value(QStringLiteral("breakRange")).toDouble();
    return settings;
}

// Schema 12. A layer without these keys follows the document, which is what
// every layer written by an earlier version did.
void insertLayerWobbleOverrides(QJsonObject &object, const Layer &layer)
{
    if (layer.wobbleAmount)
    {
        object.insert(QStringLiteral("wobble"), *layer.wobbleAmount);
    }
    if (layer.motion)
    {
        object.insert(
            QStringLiteral("motion"), motionSettingsToJson(*layer.motion));
    }
}

bool readLayerWobbleOverrides(
    const QJsonObject &object, Layer &layer, QString *error)
{
    const QJsonValue wobble = object.value(QStringLiteral("wobble"));
    if (!wobble.isUndefined() && !wobble.isNull())
    {
        if (!wobble.isDouble())
        {
            setError(error,
                DocumentSerializer::tr("A layer has an invalid wobble."));
            return false;
        }
        const qreal amount = wobble.toDouble();
        if (!std::isfinite(amount)
            || amount < DocumentLimits::minimumWobbleAmount
            || amount > DocumentLimits::maximumWobbleAmount)
        {
            setError(error,
                DocumentSerializer::tr("A layer has an invalid wobble."));
            return false;
        }
        layer.wobbleAmount = amount;
    }
    const QJsonValue motion = object.value(QStringLiteral("motion"));
    if (!motion.isUndefined() && !motion.isNull())
    {
        const std::optional<MotionSettings> settings =
            motionSettingsFromJson(motion, error);
        if (!settings)
        {
            return false;
        }
        layer.motion = *settings;
    }
    if (layer.wobbleAmount.has_value() != layer.motion.has_value())
    {
        setError(error,
            DocumentSerializer::tr(
                "A layer has an incomplete wobble override."));
        return false;
    }
    return true;
}

QJsonArray transformToJson(const QTransform &transform)
{
    return {transform.m11(),
        transform.m12(),
        transform.m13(),
        transform.m21(),
        transform.m22(),
        transform.m23(),
        transform.m31(),
        transform.m32(),
        transform.m33()};
}

QJsonObject strokeToJson(const Stroke &stroke,
    const ClipMaskTable &clipMasks,
    const BinaryMaskTable &binaryMasks)
{
    QJsonArray points;
    for (const StrokePoint &point : stroke.points)
    {
        points.append(pointToJson(point));
    }

    QJsonObject object;
    object.insert(
        QStringLiteral("id"), stroke.id.toString(QUuid::WithoutBraces));
    object.insert(QStringLiteral("seed"), QString::number(stroke.seed));
    QString modeName = QStringLiteral("paint");
    if (stroke.mode == StrokeMode::Erase)
    {
        modeName = QStringLiteral("erase");
    }
    else if (stroke.mode == StrokeMode::Fill)
    {
        modeName = QStringLiteral("fill");
    }
    else if (stroke.mode == StrokeMode::Image)
    {
        modeName = QStringLiteral("image");
    }
    else if (stroke.mode == StrokeMode::PixelSelection)
    {
        modeName = QStringLiteral("pixelSelection");
    }
    else if (stroke.mode == StrokeMode::Reframe)
    {
        modeName = QStringLiteral("reframe");
    }
    else if (stroke.mode == StrokeMode::CompositeBoundary)
    {
        modeName = QStringLiteral("compositeBoundary");
    }
    object.insert(QStringLiteral("mode"), modeName);
    object.insert(QStringLiteral("color"), stroke.color.name(QColor::HexArgb));
    object.insert(QStringLiteral("width"), stroke.width);
    object.insert(QStringLiteral("brush"), brushToJson(stroke.brush));
    object.insert(QStringLiteral("points"), points);
    if (!stroke.clipMask.isNull())
    {
        const auto id =
            clipMasks.idByCacheKey.constFind(stroke.clipMask.cacheKey());
        if (id != clipMasks.idByCacheKey.cend())
        {
            object.insert(QStringLiteral("clipMaskId"), id.value());
        }
    }
    if (stroke.visibilityClip)
    {
        object.insert(QStringLiteral("visibilityClip"),
            QJsonArray{stroke.visibilityClip->x(),
                stroke.visibilityClip->y(),
                stroke.visibilityClip->width(),
                stroke.visibilityClip->height()});
    }
    if (!stroke.fillMask.isNull())
    {
        const auto id =
            clipMasks.idByCacheKey.constFind(stroke.fillMask.cacheKey());
        if (id != clipMasks.idByCacheKey.cend())
        {
            object.insert(QStringLiteral("fillMaskId"), id.value());
        }
    }
    if (stroke.fillCoverage)
    {
        const auto id = binaryMasks.idByIdentity.constFind(
            binaryMaskIdentity(*stroke.fillCoverage));
        if (id != binaryMasks.idByIdentity.cend())
        {
            object.insert(QStringLiteral("fillCoverageId"), id.value());
        }
    }
    if (stroke.pixelSelectionOp)
    {
        const PixelSelectionOp &operation = *stroke.pixelSelectionOp;
        QJsonObject payload;
        const auto maskId = binaryMasks.idByIdentity.constFind(
            binaryMaskIdentity(pixelSelectionMaskRegion(operation)));
        if (maskId != binaryMasks.idByIdentity.cend())
        {
            payload.insert(QStringLiteral("maskId"), maskId.value());
        }
        payload.insert(
            QStringLiteral("transform"), transformToJson(operation.transform));
        payload.insert(
            QStringLiteral("sampling"), samplingModeName(operation.sampling));
        payload.insert(QStringLiteral("clearSource"), operation.clearSource);
        payload.insert(
            QStringLiteral("drawDestination"), operation.drawDestination);
        object.insert(QStringLiteral("pixelSelection"), payload);
    }
    if (stroke.reframeOp)
    {
        const ReframeOp &operation = *stroke.reframeOp;
        QJsonObject payload;
        payload.insert(QStringLiteral("mode"),
            operation.mode == ReframeMode::Image ? QStringLiteral("image")
                                                 : QStringLiteral("canvas"));
        payload.insert(
            QStringLiteral("sampling"), samplingModeName(operation.sampling));
        payload.insert(QStringLiteral("sourceSize"),
            QJsonArray{
                operation.sourceSize.width(), operation.sourceSize.height()});
        payload.insert(QStringLiteral("targetSize"),
            QJsonArray{
                operation.targetSize.width(), operation.targetSize.height()});
        payload.insert(QStringLiteral("contentOffset"),
            QJsonArray{
                operation.contentOffset.x(), operation.contentOffset.y()});
        object.insert(QStringLiteral("reframe"), payload);
    }
    if (stroke.imageOp)
    {
        const ImageOp &operation = *stroke.imageOp;
        QJsonObject payload;
        payload.insert(QStringLiteral("assetId"), operation.assetId);
        payload.insert(
            QStringLiteral("transform"), transformToJson(operation.transform));
        payload.insert(
            QStringLiteral("sampling"), samplingModeName(operation.sampling));
        object.insert(QStringLiteral("image"), payload);
    }
    return object;
}

std::optional<QImage> legacyClipMaskFromJson(const QJsonValue &value,
    QHash<QByteArray, QImage> &maskCache,
    quint64 &distinctMaskBytes,
    QString *error)
{
    if (!value.isObject())
    {
        setError(error,
            DocumentSerializer::tr("A stroke has an invalid clip mask."));
        return std::nullopt;
    }
    const QJsonObject object = value.toObject();
    const std::optional<int> width =
        integerFromJson(object.value(QStringLiteral("width")));
    const std::optional<int> height =
        integerFromJson(object.value(QStringLiteral("height")));
    if (!width || !height || !object.value(QStringLiteral("data")).isString()
        || *width < DocumentLimits::minimumCanvasEdge
        || *height < DocumentLimits::minimumCanvasEdge
        || *width > DocumentLimits::maximumCanvasEdge
        || *height > DocumentLimits::maximumCanvasEdge)
    {
        setError(error,
            DocumentSerializer::tr("A stroke has an invalid clip mask."));
        return std::nullopt;
    }

    const QByteArray compressed = QByteArray::fromBase64(
        object.value(QStringLiteral("data")).toString().toLatin1());
    if (compressed.size() < 4)
    {
        setError(error,
            DocumentSerializer::tr("A stroke has an invalid clip mask."));
        return std::nullopt;
    }
    const auto *header =
        reinterpret_cast<const uchar *>(compressed.constData());
    const quint32 declaredSize = (static_cast<quint32>(header[0]) << 24U)
                                 | (static_cast<quint32>(header[1]) << 16U)
                                 | (static_cast<quint32>(header[2]) << 8U)
                                 | static_cast<quint32>(header[3]);
    const quint64 expectedSize =
        static_cast<quint64>((*width + 3) & ~3) * static_cast<quint64>(*height);
    if (expectedSize > std::numeric_limits<quint32>::max()
        || declaredSize != expectedSize)
    {
        setError(error,
            DocumentSerializer::tr("A stroke has an invalid clip mask."));
        return std::nullopt;
    }
    QByteArray cacheKey = QByteArray::number(*width) + 'x'
                          + QByteArray::number(*height) + ':' + compressed;
    const auto cached = maskCache.constFind(cacheKey);
    if (cached != maskCache.cend())
    {
        return cached.value();
    }
    if (declaredSize
        > DocumentLimits::maximumDistinctClipMaskBytes - distinctMaskBytes)
    {
        setError(error,
            DocumentSerializer::tr(
                "The project contains too much selection data."));
        return std::nullopt;
    }
    QImage mask(QSize(*width, *height), QImage::Format_Grayscale8);
    if (mask.isNull()
        || static_cast<quint64>(mask.sizeInBytes()) != expectedSize)
    {
        setError(
            error, DocumentSerializer::tr("A stroke clip mask is too large."));
        return std::nullopt;
    }
    const std::optional<QByteArray> bytes =
        uncompressQtPayload(compressed, expectedSize);
    if (!bytes || bytes->size() != mask.sizeInBytes())
    {
        setError(error,
            DocumentSerializer::tr("A stroke has an invalid clip mask."));
        return std::nullopt;
    }
    std::memcpy(mask.bits(),
        bytes->constData(),
        static_cast<std::size_t>(bytes->size()));
    for (int y = 0; y < mask.height(); ++y)
    {
        std::fill(mask.scanLine(y) + mask.width(),
            mask.scanLine(y) + mask.bytesPerLine(),
            0);
    }
    distinctMaskBytes += declaredSize;
    maskCache.insert(std::move(cacheKey), mask);
    return mask;
}

bool isValidMaskContentId(const QString &id)
{
    return id.size() == 64
           && std::all_of(id.cbegin(),
               id.cend(),
               [](QChar character)
               {
                   return (character >= QLatin1Char('0')
                              && character <= QLatin1Char('9'))
                          || (character >= QLatin1Char('a')
                              && character <= QLatin1Char('f'));
               });
}

std::optional<QHash<QString, QImage>> clipMaskTableFromJson(
    const QJsonValue &value,
    const QSize &canvasSize,
    bool requireCanvasSize,
    quint64 &remainingAssetBytes,
    QString *error)
{
    if (!value.isArray())
    {
        setError(error,
            DocumentSerializer::tr(
                "The project contains an invalid selection mask table."));
        return std::nullopt;
    }
    const QJsonArray entries = value.toArray();
    if (entries.size() > DocumentLimits::maximumTotalStrokes)
    {
        setError(error,
            DocumentSerializer::tr(
                "The project contains too many selection masks."));
        return std::nullopt;
    }

    QHash<QString, QImage> masks;
    masks.reserve(entries.size());
    for (const auto &entryValue : entries)
    {
        if (!entryValue.isObject())
        {
            setError(error,
                DocumentSerializer::tr(
                    "The project contains an invalid selection mask table."));
            return std::nullopt;
        }
        const QJsonObject entry = entryValue.toObject();
        const QString id = entry.value(QStringLiteral("id")).toString();
        const std::optional<int> width =
            integerFromJson(entry.value(QStringLiteral("width")));
        const std::optional<int> height =
            integerFromJson(entry.value(QStringLiteral("height")));
        if (!entry.value(QStringLiteral("id")).isString()
            || !isValidMaskContentId(id) || !width || !height
            || (requireCanvasSize
                    ? QSize(*width, *height) != canvasSize
                    : *width < DocumentLimits::minimumCanvasEdge
                          || *height < DocumentLimits::minimumCanvasEdge
                          || *width > DocumentLimits::maximumCanvasEdge
                          || *height > DocumentLimits::maximumCanvasEdge)
            || !entry.value(QStringLiteral("data")).isString()
            || masks.contains(id))
        {
            setError(error,
                DocumentSerializer::tr(
                    "The project contains an invalid selection mask table."));
            return std::nullopt;
        }

        const QByteArray compressed = QByteArray::fromBase64(
            entry.value(QStringLiteral("data")).toString().toLatin1());
        if (compressed.size() < 4)
        {
            setError(error,
                DocumentSerializer::tr(
                    "The project contains an invalid selection mask."));
            return std::nullopt;
        }
        const auto *header =
            reinterpret_cast<const uchar *>(compressed.constData());
        const quint32 declaredSize = (static_cast<quint32>(header[0]) << 24U)
                                     | (static_cast<quint32>(header[1]) << 16U)
                                     | (static_cast<quint32>(header[2]) << 8U)
                                     | static_cast<quint32>(header[3]);
        const quint64 canonicalSize =
            static_cast<quint64>(*width) * static_cast<quint64>(*height);
        const quint64 paddedSize = static_cast<quint64>((*width + 3) & ~3)
                                   * static_cast<quint64>(*height);
        if (canonicalSize > std::numeric_limits<quint32>::max()
            || declaredSize != canonicalSize)
        {
            setError(error,
                DocumentSerializer::tr(
                    "The project contains an invalid selection mask."));
            return std::nullopt;
        }

        if (paddedSize > remainingAssetBytes)
        {
            setError(error,
                DocumentSerializer::tr(
                    "The project contains too much selection data."));
            return std::nullopt;
        }
        QImage mask(QSize(*width, *height), QImage::Format_Grayscale8);
        if (mask.isNull()
            || static_cast<quint64>(mask.sizeInBytes()) != paddedSize)
        {
            setError(error,
                DocumentSerializer::tr(
                    "The project contains an invalid selection mask."));
            return std::nullopt;
        }
        const std::optional<QByteArray> bytes =
            uncompressQtPayload(compressed, canonicalSize);
        if (!bytes || static_cast<quint64>(bytes->size()) != canonicalSize
            || maskContentId(*width, *height, *bytes) != id)
        {
            setError(error,
                DocumentSerializer::tr(
                    "The project contains an invalid selection mask."));
            return std::nullopt;
        }
        mask.fill(0);
        for (int y = 0; y < *height; ++y)
        {
            std::memcpy(mask.scanLine(y),
                bytes->constData() + static_cast<qsizetype>(y) * *width,
                static_cast<std::size_t>(*width));
        }
        remainingAssetBytes -= mask.sizeInBytes();
        masks.insert(id, std::move(mask));
    }
    return masks;
}

std::optional<QHash<QString, PackedMaskRegion>> binaryMaskTableFromJson(
    const QJsonValue &value, quint64 &remainingAssetBytes, QString *error)
{
    if (!value.isArray())
    {
        setError(error,
            DocumentSerializer::tr(
                "The project contains an invalid binary mask table."));
        return std::nullopt;
    }
    const QJsonArray entries = value.toArray();
    if (entries.size() > DocumentLimits::maximumTotalStrokes)
    {
        setError(error,
            DocumentSerializer::tr(
                "The project contains too many binary masks."));
        return std::nullopt;
    }
    QHash<QString, PackedMaskRegion> masks;
    masks.reserve(entries.size());
    for (const auto &entryValue : entries)
    {
        if (!entryValue.isObject())
        {
            setError(error,
                DocumentSerializer::tr(
                    "The project contains an invalid binary mask table."));
            return std::nullopt;
        }
        const QJsonObject entry = entryValue.toObject();
        const QString id = entry.value(QStringLiteral("id")).toString();
        const QJsonArray canvas =
            entry.value(QStringLiteral("canvas")).toArray();
        const QJsonArray bounds =
            entry.value(QStringLiteral("bounds")).toArray();
        if (!entry.value(QStringLiteral("id")).isString()
            || !isValidMaskContentId(id)
            || !entry.value(QStringLiteral("canvas")).isArray()
            || canvas.size() != 2
            || !entry.value(QStringLiteral("bounds")).isArray()
            || bounds.size() != 4
            || !entry.value(QStringLiteral("data")).isString()
            || masks.contains(id))
        {
            setError(error,
                DocumentSerializer::tr(
                    "The project contains an invalid binary mask table."));
            return std::nullopt;
        }
        const std::optional<int> canvasWidth = integerFromJson(canvas[0]);
        const std::optional<int> canvasHeight = integerFromJson(canvas[1]);
        const std::optional<int> x = integerFromJson(bounds[0]);
        const std::optional<int> y = integerFromJson(bounds[1]);
        const std::optional<int> width = integerFromJson(bounds[2]);
        const std::optional<int> height = integerFromJson(bounds[3]);
        if (!canvasWidth || !canvasHeight || !x || !y || !width || !height
            || *canvasWidth < DocumentLimits::minimumCanvasEdge
            || *canvasHeight < DocumentLimits::minimumCanvasEdge
            || *canvasWidth > DocumentLimits::maximumCanvasEdge
            || *canvasHeight > DocumentLimits::maximumCanvasEdge || *x < 0
            || *y < 0 || *width <= 0 || *height <= 0 || *width > *canvasWidth
            || *height > *canvasHeight || *x > *canvasWidth - *width
            || *y > *canvasHeight - *height)
        {
            setError(error,
                DocumentSerializer::tr(
                    "The project contains an invalid binary mask table."));
            return std::nullopt;
        }
        PackedMaskRegion region;
        region.canvasSize = QSize(*canvasWidth, *canvasHeight);
        region.bounds = QRect(*x, *y, *width, *height);
        const qsizetype stride = (static_cast<qsizetype>(*width) + 7) / 8;
        if (*width <= 0 || *height <= 0 || stride <= 0
            || stride > std::numeric_limits<qsizetype>::max() / *height)
        {
            setError(error,
                DocumentSerializer::tr(
                    "The project contains an invalid binary mask."));
            return std::nullopt;
        }
        const quint64 expectedBytes =
            static_cast<quint64>(stride) * static_cast<quint64>(*height);
        const QByteArray compressed = QByteArray::fromBase64(
            entry.value(QStringLiteral("data")).toString().toLatin1());
        if (compressed.size() < 4)
        {
            setError(error,
                DocumentSerializer::tr(
                    "The project contains an invalid binary mask."));
            return std::nullopt;
        }
        const auto *header =
            reinterpret_cast<const uchar *>(compressed.constData());
        const quint32 declaredSize = (static_cast<quint32>(header[0]) << 24U)
                                     | (static_cast<quint32>(header[1]) << 16U)
                                     | (static_cast<quint32>(header[2]) << 8U)
                                     | static_cast<quint32>(header[3]);
        if (expectedBytes > std::numeric_limits<quint32>::max()
            || declaredSize != expectedBytes
            || expectedBytes > remainingAssetBytes)
        {
            setError(error,
                DocumentSerializer::tr(
                    "The project contains too much binary mask data."));
            return std::nullopt;
        }
        const std::optional<QByteArray> bytes =
            uncompressQtPayload(compressed, expectedBytes);
        if (!bytes)
        {
            setError(error,
                DocumentSerializer::tr(
                    "The project contains an invalid binary mask."));
            return std::nullopt;
        }
        region.packedMask = *bytes;
        if (static_cast<quint64>(region.packedMask.size()) != expectedBytes
            || !isValidPackedMaskRegion(region)
            || binaryMaskContentId(region) != id)
        {
            setError(error,
                DocumentSerializer::tr(
                    "The project contains an invalid binary mask."));
            return std::nullopt;
        }
        remainingAssetBytes -= expectedBytes;
        masks.insert(id, std::move(region));
    }
    return masks;
}

std::optional<SamplingMode> samplingModeFromJson(const QJsonValue &value)
{
    if (!value.isString())
    {
        return std::nullopt;
    }
    const QString name = value.toString();
    if (name == QStringLiteral("nearest"))
    {
        return SamplingMode::Nearest;
    }
    if (name == QStringLiteral("smooth"))
    {
        return SamplingMode::Smooth;
    }
    return std::nullopt;
}

std::optional<LayerBlendMode> layerBlendModeFromJson(const QJsonValue &value)
{
    if (!value.isString())
    {
        return std::nullopt;
    }
    const QString name = value.toString();
    if (name == QStringLiteral("normal"))
    {
        return LayerBlendMode::Normal;
    }
    if (name == QStringLiteral("multiply"))
    {
        return LayerBlendMode::Multiply;
    }
    if (name == QStringLiteral("screen"))
    {
        return LayerBlendMode::Screen;
    }
    if (name == QStringLiteral("overlay"))
    {
        return LayerBlendMode::Overlay;
    }
    return std::nullopt;
}

std::optional<LayerKind> layerKindFromJson(const QJsonValue &value)
{
    if (!value.isString())
    {
        return std::nullopt;
    }
    const QString name = value.toString();
    if (name == QStringLiteral("paint"))
    {
        return LayerKind::Paint;
    }
    if (name == QStringLiteral("group"))
    {
        return LayerKind::Group;
    }
    return std::nullopt;
}

std::optional<QSize> sizeFromJsonArray(const QJsonValue &value)
{
    if (!value.isArray())
    {
        return std::nullopt;
    }
    const QJsonArray values = value.toArray();
    if (values.size() != 2)
    {
        return std::nullopt;
    }
    const std::optional<int> width = integerFromJson(values[0]);
    const std::optional<int> height = integerFromJson(values[1]);
    if (!width || !height)
    {
        return std::nullopt;
    }
    return QSize(*width, *height);
}

std::optional<QTransform> transformFromJson(const QJsonValue &value)
{
    if (!value.isArray())
    {
        return std::nullopt;
    }
    const QJsonArray values = value.toArray();
    if (values.size() != 9
        || !std::all_of(values.cbegin(),
            values.cend(),
            [](const QJsonValue &entry)
            {
                return entry.isDouble() && std::isfinite(entry.toDouble());
            }))
    {
        return std::nullopt;
    }
    return QTransform(values[0].toDouble(),
        values[1].toDouble(),
        values[2].toDouble(),
        values[3].toDouble(),
        values[4].toDouble(),
        values[5].toDouble(),
        values[6].toDouble(),
        values[7].toDouble(),
        values[8].toDouble());
}

std::optional<Stroke> strokeFromJson(const QJsonValue &value,
    int fileSchemaVersion,
    const QSize &canvasSize,
    QHash<QByteArray, QImage> &maskCache,
    const QHash<QString, QImage> &referencedMasks,
    const QHash<QString, PackedMaskRegion> &referencedBinaryMasks,
    quint64 &distinctMaskBytes,
    QString *error)
{
    if (!value.isObject())
    {
        setError(
            error, DocumentSerializer::tr("A stroke entry is not an object."));
        return std::nullopt;
    }

    const QJsonObject object = value.toObject();
    if (!object.value(QStringLiteral("id")).isString()
        || !object.value(QStringLiteral("seed")).isString()
        || !object.value(QStringLiteral("mode")).isString()
        || !object.value(QStringLiteral("color")).isString()
        || !object.value(QStringLiteral("width")).isDouble()
        || !object.value(QStringLiteral("points")).isArray())
    {
        setError(
            error, DocumentSerializer::tr("A stroke contains invalid fields."));
        return std::nullopt;
    }
    const QJsonArray points = object.value(QStringLiteral("points")).toArray();
    if (points.size() > DocumentLimits::maximumPointsPerStroke)
    {
        setError(error,
            DocumentSerializer::tr("A stroke has an invalid point count."));
        return std::nullopt;
    }

    Stroke stroke;
    const QUuid id(object.value(QStringLiteral("id")).toString());
    if (id.isNull())
    {
        setError(error, DocumentSerializer::tr("A stroke has an invalid ID."));
        return std::nullopt;
    }
    stroke.id = id;
    const QString seedText = object.value(QStringLiteral("seed")).toString();
    if (seedText.isEmpty()
        || !std::all_of(seedText.cbegin(),
            seedText.cend(),
            [](QChar character)
            {
                return character >= QLatin1Char('0')
                       && character <= QLatin1Char('9');
            }))
    {
        setError(
            error, DocumentSerializer::tr("A stroke has an invalid seed."));
        return std::nullopt;
    }
    bool seedValid = false;
    stroke.seed = seedText.toULongLong(&seedValid);
    if (!seedValid)
    {
        setError(
            error, DocumentSerializer::tr("A stroke has an invalid seed."));
        return std::nullopt;
    }
    const QString mode = object.value(QStringLiteral("mode")).toString();
    if (mode != QStringLiteral("paint") && mode != QStringLiteral("erase")
        && mode != QStringLiteral("fill")
        && (fileSchemaVersion < 13
            || mode != QStringLiteral("compositeBoundary"))
        && (fileSchemaVersion < 11 || mode != QStringLiteral("image"))
        && (fileSchemaVersion < 6
            || (mode != QStringLiteral("pixelSelection")
                && mode != QStringLiteral("reframe"))))
    {
        setError(
            error, DocumentSerializer::tr("A stroke has an invalid mode."));
        return std::nullopt;
    }
    if (mode == QStringLiteral("erase"))
    {
        stroke.mode = StrokeMode::Erase;
    }
    else if (mode == QStringLiteral("fill"))
    {
        stroke.mode = StrokeMode::Fill;
    }
    else if (mode == QStringLiteral("image"))
    {
        stroke.mode = StrokeMode::Image;
    }
    else if (mode == QStringLiteral("pixelSelection"))
    {
        stroke.mode = StrokeMode::PixelSelection;
    }
    else if (mode == QStringLiteral("reframe"))
    {
        stroke.mode = StrokeMode::Reframe;
    }
    else if (mode == QStringLiteral("compositeBoundary"))
    {
        stroke.mode = StrokeMode::CompositeBoundary;
    }
    else
    {
        stroke.mode = StrokeMode::Paint;
    }
    const QColor color(object.value(QStringLiteral("color")).toString());
    if (!color.isValid())
    {
        setError(
            error, DocumentSerializer::tr("A stroke has an invalid color."));
        return std::nullopt;
    }
    stroke.color = color;
    stroke.width = object.value(QStringLiteral("width")).toDouble();
    if (!std::isfinite(stroke.width)
        || stroke.width < DocumentLimits::minimumStrokeWidth
        || stroke.width > DocumentLimits::maximumStrokeWidth)
    {
        setError(
            error, DocumentSerializer::tr("A stroke has an invalid width."));
        return std::nullopt;
    }
    if (fileSchemaVersion >= 2)
    {
        const std::optional<BrushSettings> brush =
            brushFromJson(object.value(QStringLiteral("brush")), error);
        if (!brush)
        {
            return std::nullopt;
        }
        stroke.brush = *brush;
    }
    stroke.points.reserve(points.size());

    for (const auto &pointValue : points)
    {
        const std::optional<StrokePoint> point = pointFromJson(pointValue);
        if (!point)
        {
            setError(error,
                DocumentSerializer::tr("A stroke contains an invalid point."));
            return std::nullopt;
        }
        stroke.points.append(*point);
    }
    const bool operationWithoutPoints =
        stroke.mode == StrokeMode::PixelSelection
        || stroke.mode == StrokeMode::Reframe
        || stroke.mode == StrokeMode::Image
        || stroke.mode == StrokeMode::CompositeBoundary;
    if (operationWithoutPoints != points.isEmpty())
    {
        setError(error,
            DocumentSerializer::tr("A stroke has an invalid point count."));
        return std::nullopt;
    }
    if (fileSchemaVersion >= 4)
    {
        if (object.contains(QStringLiteral("clipMask")))
        {
            setError(error,
                DocumentSerializer::tr("A stroke contains a legacy clip "
                                       "mask in a current project."));
            return std::nullopt;
        }
        const QJsonValue clipMaskId =
            object.value(QStringLiteral("clipMaskId"));
        if (!clipMaskId.isUndefined())
        {
            if (!clipMaskId.isString())
            {
                setError(error,
                    DocumentSerializer::tr(
                        "A stroke has an invalid clip mask reference."));
                return std::nullopt;
            }
            const auto mask = referencedMasks.constFind(clipMaskId.toString());
            if (mask == referencedMasks.cend())
            {
                setError(error,
                    DocumentSerializer::tr(
                        "A stroke references a missing clip mask."));
                return std::nullopt;
            }
            stroke.clipMask = mask.value();
        }
        if (fileSchemaVersion >= 5)
        {
            const QJsonValue visibility =
                object.value(QStringLiteral("visibilityClip"));
            if (!visibility.isUndefined())
            {
                if (!visibility.isArray())
                {
                    setError(error,
                        DocumentSerializer::tr(
                            "A stroke has an invalid visibility clip."));
                    return std::nullopt;
                }
                const QJsonArray values = visibility.toArray();
                if (values.size() != 4)
                {
                    setError(error,
                        DocumentSerializer::tr(
                            "A stroke has an invalid visibility clip."));
                    return std::nullopt;
                }
                const std::optional<int> x = integerFromJson(values[0]);
                const std::optional<int> y = integerFromJson(values[1]);
                const std::optional<int> width = integerFromJson(values[2]);
                const std::optional<int> height = integerFromJson(values[3]);
                const int maximumWidth =
                    fileSchemaVersion <= 5 ? canvasSize.width()
                                           : DocumentLimits::maximumCanvasEdge;
                const int maximumHeight =
                    fileSchemaVersion <= 5 ? canvasSize.height()
                                           : DocumentLimits::maximumCanvasEdge;
                const bool validBounds =
                    x && y && width && height && *x >= 0 && *y >= 0
                    && *width > 0 && *height > 0 && *width <= maximumWidth
                    && *height <= maximumHeight && *x <= maximumWidth - *width
                    && *y <= maximumHeight - *height;
                if (!validBounds)
                {
                    setError(error,
                        DocumentSerializer::tr(
                            "A stroke has an invalid visibility clip."));
                    return std::nullopt;
                }
                const QRect rect(*x, *y, *width, *height);
                stroke.visibilityClip = rect;
            }

            const QJsonValue fillMaskId =
                object.value(QStringLiteral("fillMaskId"));
            if (!fillMaskId.isUndefined())
            {
                if (stroke.mode != StrokeMode::Fill || !fillMaskId.isString())
                {
                    setError(error,
                        DocumentSerializer::tr(
                            "A stroke has an invalid fill mask reference."));
                    return std::nullopt;
                }
                const auto mask =
                    referencedMasks.constFind(fillMaskId.toString());
                if (mask == referencedMasks.cend())
                {
                    setError(error,
                        DocumentSerializer::tr(
                            "A stroke references a missing fill mask."));
                    return std::nullopt;
                }
                stroke.fillMask = mask.value();
            }
            if (fileSchemaVersion >= 6)
            {
                const QJsonValue fillCoverageId =
                    object.value(QStringLiteral("fillCoverageId"));
                if (fileSchemaVersion < 10 && !fillCoverageId.isUndefined())
                {
                    setError(error,
                        DocumentSerializer::tr(
                            "A stroke references unsupported fill coverage."));
                    return std::nullopt;
                }
                if (fileSchemaVersion >= 10 && !fillCoverageId.isUndefined())
                {
                    if (stroke.mode != StrokeMode::Fill
                        || !fillCoverageId.isString())
                    {
                        setError(error,
                            DocumentSerializer::tr("A stroke has an invalid "
                                                   "fill coverage reference."));
                        return std::nullopt;
                    }
                    const auto mask = referencedBinaryMasks.constFind(
                        fillCoverageId.toString());
                    if (mask == referencedBinaryMasks.cend())
                    {
                        setError(error,
                            DocumentSerializer::tr(
                                "A stroke references missing fill coverage."));
                        return std::nullopt;
                    }
                    stroke.fillCoverage = mask.value();
                }
                const QJsonValue pixelSelectionPayload =
                    object.value(QStringLiteral("pixelSelection"));
                const QJsonValue reframePayload =
                    object.value(QStringLiteral("reframe"));
                const QJsonValue imagePayload =
                    object.value(QStringLiteral("image"));
                if ((stroke.mode != StrokeMode::PixelSelection
                        && !pixelSelectionPayload.isUndefined())
                    || (stroke.mode != StrokeMode::Reframe
                        && !reframePayload.isUndefined())
                    || (stroke.mode != StrokeMode::Image
                        && !imagePayload.isUndefined()))
                {
                    setError(error,
                        DocumentSerializer::tr("A stroke contains an operation "
                                               "payload for the wrong mode."));
                    return std::nullopt;
                }

                if (stroke.mode == StrokeMode::PixelSelection)
                {
                    const QJsonValue &payloadValue = pixelSelectionPayload;
                    if (!payloadValue.isObject())
                    {
                        setError(error,
                            DocumentSerializer::tr(
                                "A pixel selection operation is invalid."));
                        return std::nullopt;
                    }
                    const QJsonObject payload = payloadValue.toObject();
                    const QString maskId =
                        payload.value(QStringLiteral("maskId")).toString();
                    const auto mask = referencedBinaryMasks.constFind(maskId);
                    const std::optional<QTransform> operationTransform =
                        transformFromJson(
                            payload.value(QStringLiteral("transform")));
                    const std::optional<SamplingMode> sampling =
                        samplingModeFromJson(
                            payload.value(QStringLiteral("sampling")));
                    if (!payload.value(QStringLiteral("maskId")).isString()
                        || mask == referencedBinaryMasks.cend()
                        || !operationTransform || !sampling
                        || !payload.value(QStringLiteral("clearSource"))
                            .isBool()
                        || !payload.value(QStringLiteral("drawDestination"))
                            .isBool())
                    {
                        setError(error,
                            DocumentSerializer::tr(
                                "A pixel selection operation is invalid."));
                        return std::nullopt;
                    }
                    PixelSelectionOp operation;
                    operation.canvasSize = mask->canvasSize;
                    operation.sourceBounds = mask->bounds;
                    operation.packedMask = mask->packedMask;
                    operation.transform = *operationTransform;
                    operation.sampling = *sampling;
                    operation.clearSource =
                        payload.value(QStringLiteral("clearSource")).toBool();
                    operation.drawDestination =
                        payload.value(QStringLiteral("drawDestination"))
                            .toBool();
                    if (!isValidPixelSelectionOp(operation))
                    {
                        setError(error,
                            DocumentSerializer::tr(
                                "A pixel selection operation is invalid."));
                        return std::nullopt;
                    }
                    stroke.pixelSelectionOp = std::move(operation);
                }

                if (stroke.mode == StrokeMode::Reframe)
                {
                    const QJsonValue &payloadValue = reframePayload;
                    if (!payloadValue.isObject())
                    {
                        setError(error,
                            DocumentSerializer::tr(
                                "A reframe operation is invalid."));
                        return std::nullopt;
                    }
                    const QJsonObject payload = payloadValue.toObject();
                    const QString reframeMode =
                        payload.value(QStringLiteral("mode")).toString();
                    const std::optional<SamplingMode> sampling =
                        samplingModeFromJson(
                            payload.value(QStringLiteral("sampling")));
                    const std::optional<QSize> sourceSize = sizeFromJsonArray(
                        payload.value(QStringLiteral("sourceSize")));
                    const std::optional<QSize> targetSize = sizeFromJsonArray(
                        payload.value(QStringLiteral("targetSize")));
                    const QJsonValue offsetValue =
                        payload.value(QStringLiteral("contentOffset"));
                    const QJsonArray offset = offsetValue.toArray();
                    const std::optional<int> offsetX =
                        offset.size() == 2 ? integerFromJson(offset[0])
                                           : std::nullopt;
                    const std::optional<int> offsetY =
                        offset.size() == 2 ? integerFromJson(offset[1])
                                           : std::nullopt;
                    if (!payload.value(QStringLiteral("mode")).isString()
                        || (reframeMode != QStringLiteral("canvas")
                            && reframeMode != QStringLiteral("image"))
                        || !sampling || !sourceSize || !targetSize
                        || !offsetValue.isArray() || !offsetX || !offsetY)
                    {
                        setError(error,
                            DocumentSerializer::tr(
                                "A reframe operation is invalid."));
                        return std::nullopt;
                    }
                    ReframeOp operation;
                    operation.mode = reframeMode == QStringLiteral("image")
                                         ? ReframeMode::Image
                                         : ReframeMode::Canvas;
                    operation.sampling = *sampling;
                    operation.sourceSize = *sourceSize;
                    operation.targetSize = *targetSize;
                    operation.contentOffset = QPoint(*offsetX, *offsetY);
                    if (!isValidReframeOp(operation))
                    {
                        setError(error,
                            DocumentSerializer::tr(
                                "A reframe operation is invalid."));
                        return std::nullopt;
                    }
                    stroke.reframeOp = operation;
                }

                if (stroke.mode == StrokeMode::Image)
                {
                    if (fileSchemaVersion < 11 || !imagePayload.isObject())
                    {
                        setError(error,
                            DocumentSerializer::tr(
                                "An image operation is invalid."));
                        return std::nullopt;
                    }
                    const QJsonObject payload = imagePayload.toObject();
                    const std::optional<QTransform> operationTransform =
                        transformFromJson(
                            payload.value(QStringLiteral("transform")));
                    const std::optional<SamplingMode> sampling =
                        samplingModeFromJson(
                            payload.value(QStringLiteral("sampling")));
                    ImageOp operation;
                    operation.assetId =
                        payload.value(QStringLiteral("assetId")).toString();
                    if (!payload.value(QStringLiteral("assetId")).isString()
                        || !operationTransform || !sampling)
                    {
                        setError(error,
                            DocumentSerializer::tr(
                                "An image operation is invalid."));
                        return std::nullopt;
                    }
                    operation.transform = *operationTransform;
                    operation.sampling = *sampling;
                    if (!isValidImageOp(operation))
                    {
                        setError(error,
                            DocumentSerializer::tr(
                                "An image operation is invalid."));
                        return std::nullopt;
                    }
                    stroke.imageOp = std::move(operation);
                }
            }
        }
    }
    else if (fileSchemaVersion >= 3
             && object.contains(QStringLiteral("clipMask")))
    {
        const std::optional<QImage> clipMask =
            legacyClipMaskFromJson(object.value(QStringLiteral("clipMask")),
                maskCache,
                distinctMaskBytes,
                error);
        if (!clipMask)
        {
            return std::nullopt;
        }
        stroke.clipMask = *clipMask;
    }
    return stroke;
}

QJsonObject layerToJson(const Layer &layer,
    const ClipMaskTable &clipMasks,
    const BinaryMaskTable &binaryMasks)
{
    QJsonArray strokes;
    for (const Stroke &stroke : layer.strokes)
    {
        strokes.append(strokeToJson(stroke, clipMasks, binaryMasks));
    }

    QJsonObject object;
    object.insert(
        QStringLiteral("id"), layer.id.toString(QUuid::WithoutBraces));
    object.insert(QStringLiteral("name"), layer.name);
    object.insert(QStringLiteral("kind"), layerKindName(layer.kind));
    object.insert(QStringLiteral("parentGroupId"),
        layer.parentGroupId.isNull()
            ? QJsonValue(QJsonValue::Null)
            : QJsonValue(layer.parentGroupId.toString(QUuid::WithoutBraces)));
    object.insert(QStringLiteral("clipToLayerBelow"), layer.clipToLayerBelow);
    object.insert(QStringLiteral("visible"), layer.visible);
    object.insert(QStringLiteral("reference"), layer.reference);
    object.insert(QStringLiteral("opacity"), layer.opacity);
    object.insert(
        QStringLiteral("blendMode"), layerBlendModeName(layer.blendMode));
    object.insert(QStringLiteral("initialCanvasSize"),
        QJsonArray{
            layer.initialCanvasSize.width(), layer.initialCanvasSize.height()});
    insertLayerWobbleOverrides(object, layer);
    object.insert(QStringLiteral("strokes"), strokes);
    return object;
}

QJsonObject layerSkeletonToJson(const Layer &layer)
{
    QJsonObject object;
    object.insert(
        QStringLiteral("id"), layer.id.toString(QUuid::WithoutBraces));
    object.insert(QStringLiteral("name"), layer.name);
    object.insert(QStringLiteral("kind"), layerKindName(layer.kind));
    object.insert(QStringLiteral("parentGroupId"),
        layer.parentGroupId.isNull()
            ? QJsonValue(QJsonValue::Null)
            : QJsonValue(layer.parentGroupId.toString(QUuid::WithoutBraces)));
    object.insert(QStringLiteral("clipToLayerBelow"), layer.clipToLayerBelow);
    object.insert(QStringLiteral("visible"), layer.visible);
    object.insert(QStringLiteral("reference"), layer.reference);
    object.insert(QStringLiteral("opacity"), layer.opacity);
    object.insert(
        QStringLiteral("blendMode"), layerBlendModeName(layer.blendMode));
    object.insert(QStringLiteral("initialCanvasSize"),
        QJsonArray{
            layer.initialCanvasSize.width(), layer.initialCanvasSize.height()});
    insertLayerWobbleOverrides(object, layer);
    object.insert(QStringLiteral("strokes"), QJsonArray());
    return object;
}

QJsonObject rootToJson(const Document &document,
    const QJsonArray &layers,
    const QJsonArray &clipMasks,
    const QJsonArray &binaryMasks,
    const QJsonObject &additionalRootFields)
{
    QJsonObject canvas;
    canvas.insert(QStringLiteral("width"), document.size.width());
    canvas.insert(QStringLiteral("height"), document.size.height());
    canvas.insert(QStringLiteral("background"),
        document.background.name(QColor::HexArgb));

    QJsonObject animation;
    animation.insert(QStringLiteral("frames"), document.animationFrames);
    animation.insert(QStringLiteral("fps"), document.framesPerSecond);
    animation.insert(QStringLiteral("wobble"), document.wobbleAmount);
    animation.insert(
        QStringLiteral("motion"), motionSettingsToJson(document.motion));

    QJsonArray rasterAssets;
    for (const RasterAsset &asset : document.rasterAssets)
    {
        QJsonObject entry;
        entry.insert(QStringLiteral("id"), asset.id);
        entry.insert(QStringLiteral("size"),
            QJsonArray{asset.size.width(), asset.size.height()});
        entry.insert(QStringLiteral("data"),
            QString::fromLatin1(asset.compressedRgba.toBase64()));
        rasterAssets.append(entry);
    }

    QJsonObject root;
    root.insert(QStringLiteral("schemaVersion"), schemaVersion);
    root.insert(QStringLiteral("algorithmVersion"), algorithmVersion);
    root.insert(QStringLiteral("canvas"), canvas);
    root.insert(QStringLiteral("animation"), animation);
    root.insert(QStringLiteral("activeLayerId"),
        document.activeLayerId.isNull()
            ? QJsonValue(QJsonValue::Null)
            : QJsonValue(
                  document.activeLayerId.toString(QUuid::WithoutBraces)));
    root.insert(QStringLiteral("layers"), layers);
    root.insert(QStringLiteral("clipMasks"), clipMasks);
    root.insert(QStringLiteral("binaryMasks"), binaryMasks);
    root.insert(QStringLiteral("rasterAssets"), rasterAssets);
    for (auto field = additionalRootFields.constBegin();
        field != additionalRootFields.constEnd();
        ++field)
    {
        root.insert(field.key(), field.value());
    }
    return root;
}

std::optional<Layer> layerFromJson(const QJsonValue &value,
    int fileSchemaVersion,
    const QSize &canvasSize,
    QHash<QByteArray, QImage> &maskCache,
    const QHash<QString, QImage> &referencedMasks,
    const QHash<QString, PackedMaskRegion> &referencedBinaryMasks,
    quint64 &distinctMaskBytes,
    QString *error)
{
    if (!value.isObject())
    {
        setError(
            error, DocumentSerializer::tr("A layer entry is not an object."));
        return std::nullopt;
    }

    const QJsonObject object = value.toObject();
    const QJsonValue referenceValue = object.value(QStringLiteral("reference"));
    if (!object.value(QStringLiteral("id")).isString()
        || !object.value(QStringLiteral("name")).isString()
        || !object.value(QStringLiteral("visible")).isBool()
        || !object.value(QStringLiteral("opacity")).isDouble()
        || !object.value(QStringLiteral("strokes")).isArray()
        || (fileSchemaVersion >= 9 && !referenceValue.isBool())
        || (fileSchemaVersion >= 8
            && (!object.value(QStringLiteral("kind")).isString()
                || (!object.value(QStringLiteral("parentGroupId")).isString()
                    && !object.value(QStringLiteral("parentGroupId")).isNull())
                || !object.value(QStringLiteral("clipToLayerBelow")).isBool()))
        || (fileSchemaVersion >= 7
            && !object.value(QStringLiteral("blendMode")).isString())
        || (fileSchemaVersion >= 6
            && !object.value(QStringLiteral("initialCanvasSize")).isArray()))
    {
        setError(
            error, DocumentSerializer::tr("A layer contains invalid fields."));
        return std::nullopt;
    }
    const QJsonArray strokes =
        object.value(QStringLiteral("strokes")).toArray();
    if (strokes.size() > DocumentLimits::maximumStrokesPerLayer)
    {
        setError(error,
            DocumentSerializer::tr("A layer contains too many strokes."));
        return std::nullopt;
    }

    Layer layer;
    const QUuid id(object.value(QStringLiteral("id")).toString());
    if (id.isNull())
    {
        setError(error, DocumentSerializer::tr("A layer has an invalid ID."));
        return std::nullopt;
    }
    layer.id = id;
    layer.name = object.value(QStringLiteral("name")).toString();
    if (layer.name.trimmed().isEmpty()
        || layer.name.size() > DocumentLimits::maximumLayerNameLength)
    {
        setError(error, DocumentSerializer::tr("A layer has an invalid name."));
        return std::nullopt;
    }
    if (fileSchemaVersion >= 8)
    {
        const std::optional<LayerKind> kind =
            layerKindFromJson(object.value(QStringLiteral("kind")));
        const QJsonValue parentValue =
            object.value(QStringLiteral("parentGroupId"));
        const QUuid parentId =
            parentValue.isNull() ? QUuid() : QUuid(parentValue.toString());
        if (!kind || (!parentValue.isNull() && parentId.isNull()))
        {
            setError(error,
                DocumentSerializer::tr(
                    "A layer has invalid hierarchy fields."));
            return std::nullopt;
        }
        layer.kind = *kind;
        layer.parentGroupId = parentId;
        layer.clipToLayerBelow =
            object.value(QStringLiteral("clipToLayerBelow")).toBool();
    }
    layer.visible = object.value(QStringLiteral("visible")).toBool();
    if (fileSchemaVersion >= 9)
    {
        layer.reference = referenceValue.toBool();
    }
    layer.opacity = object.value(QStringLiteral("opacity")).toDouble();
    if (!std::isfinite(layer.opacity) || layer.opacity < 0.0
        || layer.opacity > 1.0)
    {
        setError(
            error, DocumentSerializer::tr("A layer has an invalid opacity."));
        return std::nullopt;
    }
    if (fileSchemaVersion >= 7)
    {
        const std::optional<LayerBlendMode> blendMode =
            layerBlendModeFromJson(object.value(QStringLiteral("blendMode")));
        if (!blendMode)
        {
            setError(error,
                DocumentSerializer::tr("A layer has an invalid blend mode."));
            return std::nullopt;
        }
        layer.blendMode = *blendMode;
    }
    if (fileSchemaVersion >= 12
        && !readLayerWobbleOverrides(object, layer, error))
    {
        return std::nullopt;
    }
    layer.initialCanvasSize = canvasSize;
    if (fileSchemaVersion >= 6)
    {
        const std::optional<QSize> initialSize = sizeFromJsonArray(
            object.value(QStringLiteral("initialCanvasSize")));
        if (!initialSize
            || initialSize->width() < DocumentLimits::minimumCanvasEdge
            || initialSize->height() < DocumentLimits::minimumCanvasEdge
            || initialSize->width() > DocumentLimits::maximumCanvasEdge
            || initialSize->height() > DocumentLimits::maximumCanvasEdge)
        {
            setError(error,
                DocumentSerializer::tr(
                    "A layer has an invalid initial canvas size."));
            return std::nullopt;
        }
        layer.initialCanvasSize = *initialSize;
    }
    layer.strokes.reserve(strokes.size());

    for (const auto &strokeValue : strokes)
    {
        const std::optional<Stroke> stroke = strokeFromJson(strokeValue,
            fileSchemaVersion,
            canvasSize,
            maskCache,
            referencedMasks,
            referencedBinaryMasks,
            distinctMaskBytes,
            error);
        if (!stroke)
        {
            return std::nullopt;
        }
        layer.strokes.append(*stroke);
    }
    return layer;
}

std::optional<int> integerFromJson(const QJsonValue &value)
{
    if (!value.isDouble())
    {
        return std::nullopt;
    }
    const double number = value.toDouble();
    if (!std::isfinite(number) || std::trunc(number) != number
        || number < static_cast<double>(std::numeric_limits<int>::min())
        || number > static_cast<double>(std::numeric_limits<int>::max()))
    {
        return std::nullopt;
    }
    return static_cast<int>(number);
}
}

}

// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "document/Document.hpp"

#include "document/DocumentLimits.hpp"
#include "document/LayerHierarchy.hpp"

#include <algorithm>
#include <cmath>

namespace ugurugu
{

bool isValidMotionStyle(MotionStyle style)
{
    return style == MotionStyle::Classic || style == MotionStyle::Smooth
           || style == MotionStyle::Stepped;
}

bool isValidMotionSettings(const MotionSettings &settings, int animationFrames)
{
    const bool validPoseCount =
        settings.poseCount >= DocumentLimits::minimumMotionPoseCount
        && settings.poseCount <= DocumentLimits::maximumMotionPoseCount
        && (settings.style == MotionStyle::Classic
            || settings.poseCount <= animationFrames);
    return isValidMotionStyle(settings.style) && validPoseCount
           && settings.detail >= DocumentLimits::minimumMotionDetail
           && settings.detail <= DocumentLimits::maximumMotionDetail
           && std::isfinite(settings.linked) && settings.linked >= 0.0
           && settings.linked <= 1.0 && std::isfinite(settings.randomness)
           && settings.randomness >= 0.0 && settings.randomness <= 1.0
           && std::isfinite(settings.breakAmount) && settings.breakAmount >= 0.0
           && settings.breakAmount <= 1.0 && std::isfinite(settings.breakRange)
           && settings.breakRange >= DocumentLimits::minimumBreakRange
           && settings.breakRange <= DocumentLimits::maximumBreakRange;
}

bool isValidImageOp(const ImageOp &operation)
{
    const QTransform &transform = operation.transform;
    const bool finite =
        std::isfinite(transform.m11()) && std::isfinite(transform.m12())
        && std::isfinite(transform.m13()) && std::isfinite(transform.m21())
        && std::isfinite(transform.m22()) && std::isfinite(transform.m23())
        && std::isfinite(transform.m31()) && std::isfinite(transform.m32())
        && std::isfinite(transform.m33());
    return !operation.assetId.isEmpty() && finite && transform.isAffine()
           && transform.isInvertible()
           && (operation.sampling == SamplingMode::Nearest
               || operation.sampling == SamplingMode::Smooth);
}

bool isValidRasterAssetMetadata(const RasterAsset &asset)
{
    const bool validId = asset.id.size() == 64
                         && std::all_of(asset.id.cbegin(),
                             asset.id.cend(),
                             [](QChar character)
                             {
                                 return (character >= QLatin1Char('0')
                                            && character <= QLatin1Char('9'))
                                        || (character >= QLatin1Char('a')
                                            && character <= QLatin1Char('f'));
                             });
    if (!validId || asset.size.width() <= 0 || asset.size.height() <= 0
        || asset.compressedRgba.isEmpty())
    {
        return false;
    }
    const quint64 width = static_cast<quint64>(asset.size.width());
    const quint64 height = static_cast<quint64>(asset.size.height());
    return width <= DocumentLimits::maximumRasterAssetPixels / height
           && width * height <= DocumentLimits::maximumRasterAssetPixels;
}

bool isValidBrushSettings(const BrushSettings &settings)
{
    const bool validEngine = settings.engine == BrushEngine::Line
                             || settings.engine == BrushEngine::Airbrush
                             || settings.engine == BrushEngine::Spray;
    const bool validTip = settings.tipShape == BrushTipShape::Round
                          || settings.tipShape == BrushTipShape::Square;
    return validEngine && validTip && std::isfinite(settings.opacity)
           && settings.opacity >= 0.0 && settings.opacity <= 1.0
           && std::isfinite(settings.flow) && settings.flow > 0.0
           && settings.flow <= 1.0 && std::isfinite(settings.hardness)
           && settings.hardness >= 0.0 && settings.hardness <= 1.0
           && std::isfinite(settings.spacing)
           && settings.spacing >= DocumentLimits::minimumBrushSpacing
           && settings.spacing <= DocumentLimits::maximumBrushSpacing
           && std::isfinite(settings.scatter) && settings.scatter >= 0.0
           && settings.scatter <= DocumentLimits::maximumBrushScatter
           && std::isfinite(settings.particleSize)
           && settings.particleSize >= DocumentLimits::minimumBrushParticleSize
           && settings.particleSize <= DocumentLimits::maximumBrushParticleSize
           && std::isfinite(settings.density)
           && settings.density >= DocumentLimits::minimumBrushDensity
           && settings.density <= DocumentLimits::maximumBrushDensity
           && std::isfinite(settings.sizeDynamics)
           && settings.sizeDynamics >= 0.0 && settings.sizeDynamics <= 1.0
           && std::isfinite(settings.opacityDynamics)
           && settings.opacityDynamics >= 0.0 && settings.opacityDynamics <= 1.0
           && std::isfinite(settings.sizeJitter) && settings.sizeJitter >= 0.0
           && settings.sizeJitter <= 1.0 && std::isfinite(settings.wobbleScale)
           && settings.wobbleScale >= DocumentLimits::minimumBrushWobbleScale
           && settings.wobbleScale <= DocumentLimits::maximumBrushWobbleScale;
}

bool isValidLayerBlendMode(LayerBlendMode mode)
{
    return mode == LayerBlendMode::Normal || mode == LayerBlendMode::Multiply
           || mode == LayerBlendMode::Screen || mode == LayerBlendMode::Overlay;
}

bool isValidLayerKind(LayerKind kind)
{
    return kind == LayerKind::Paint || kind == LayerKind::Group;
}

Document Document::createDefault(
    const QSize &canvasSize, const QString &initialLayerName)
{
    Document document;
    document.size = canvasSize;
    Layer layer;
    layer.name = initialLayerName.isEmpty() ? QStringLiteral("Layer 1")
                                            : initialLayerName;
    layer.initialCanvasSize = canvasSize;
    document.activeLayerId = layer.id;
    document.layers.append(layer);
    return document;
}

Layer *Document::layer(const QUuid &id)
{
    const int index = layerIndex(id);
    return index >= 0 ? &layers[index] : nullptr;
}

const Layer *Document::layer(const QUuid &id) const
{
    const int index = layerIndex(id);
    return index >= 0 ? &layers[index] : nullptr;
}

int Document::layerIndex(const QUuid &id) const
{
    for (int index = 0; index < layers.size(); ++index)
    {
        if (layers[index].id == id)
        {
            return index;
        }
    }
    return -1;
}

bool Document::isLayerDescendantOf(
    const QUuid &layerId, const QUuid &ancestorGroupId) const
{
    return analyzeLayerHierarchy(*this).isDescendantOf(
        layerId, ancestorGroupId);
}

int Document::layerDepth(const QUuid &id) const
{
    const int depth = analyzeLayerHierarchy(*this).depth(id);
    return depth >= 0 ? depth : 0;
}

qreal effectiveWobbleAmount(const Document &document, const Layer &layer)
{
    return layer.wobbleAmount.value_or(document.wobbleAmount);
}

MotionSettings effectiveMotion(const Document &document, const Layer &layer)
{
    return layer.motion.value_or(document.motion);
}

Document documentForLayer(const Document &document, const Layer &layer)
{
    if (!layer.wobbleAmount && !layer.motion)
    {
        return document;
    }
    Document resolved = document;
    resolved.wobbleAmount = effectiveWobbleAmount(document, layer);
    resolved.motion = effectiveMotion(document, layer);
    return resolved;
}

}

// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "render/StrokeMotionModel.hpp"

#include "render/ClassicStrokeMotion.hpp"
#include "render/DeterministicNoise.hpp"
#include "render/MotionTimeModel.hpp"

#include <algorithm>
#include <limits>

namespace ugurugu::StrokeMotionModel
{

namespace
{

struct PoseSample
{
    int first = 0;
    int second = 0;
    qreal blend = 0.0;
};

// These channels and the linked seed are schema 10 rendering invariants.
constexpr quint64 linkedSeed = 0x94d049bb133111ebULL;
constexpr quint64 widthChannel = 0x739d61a2ULL;
constexpr quint64 swayChannel = 0xc45a9137ULL;
constexpr quint64 detailChannel = 0x82f17b63ULL;
constexpr quint64 tangentChannel = 0x5e2a7d91ULL;
constexpr quint64 randomNormalChannel = 0xb37c4e25ULL;
constexpr quint64 randomTangentChannel = 0x21ad8f79ULL;

int normalizedFrame(int frameIndex, int frameCount)
{
    const int count = std::max(1, frameCount);
    return ((frameIndex % count) + count) % count;
}

PoseSample poseSample(
    int frameIndex, int frameCount, const MotionSettings &settings)
{
    const int frame = normalizedFrame(frameIndex, frameCount);
    if (settings.style == MotionStyle::Smooth)
    {
        const auto sample = MotionTimeModel::smoothSample(
            frame, frameCount, settings.poseCount);
        if (sample)
        {
            return {sample->firstPose, sample->secondPose, sample->blend};
        }
    }
    else if (settings.style == MotionStyle::Stepped)
    {
        const auto pose =
            MotionTimeModel::steppedPose(frame, frameCount, settings.poseCount);
        if (pose)
        {
            return {*pose, *pose, 0.0};
        }
    }
    return {frame, frame, 0.0};
}

qreal interpolate(qreal first, qreal second, qreal blend)
{
    return first + (second - first) * blend;
}

template <typename Sampler>
qreal temporalValue(const PoseSample &pose, Sampler sampler)
{
    return interpolate(sampler(pose.first), sampler(pose.second), pose.blend);
}

qreal linkedSmoothValue(quint64 seed,
    const PoseSample &pose,
    qreal coordinate,
    quint64 channel,
    qreal linked)
{
    const qreal independent = temporalValue(pose,
        [=](int value)
        {
            return DeterministicNoise::smoothValue(
                seed, value, coordinate, channel);
        });
    const qreal shared = temporalValue(pose,
        [=](int value)
        {
            return DeterministicNoise::smoothValue(
                linkedSeed, value, coordinate, channel);
        });
    return interpolate(independent, shared, linked);
}

qreal linkedRandomValue(quint64 seed,
    const PoseSample &pose,
    int sampleIndex,
    quint64 channel,
    qreal linked)
{
    const qreal independent = temporalValue(pose,
        [=](int value)
        {
            return DeterministicNoise::signedValue(
                seed, value, sampleIndex, channel);
        });
    const qreal shared = temporalValue(pose,
        [=](int value)
        {
            return DeterministicNoise::signedValue(
                linkedSeed, value, sampleIndex, channel);
        });
    return interpolate(independent, shared, linked);
}

}

qreal maximumDisplacement(
    qreal strokeWidth, qreal wobbleAmount, const MotionSettings &)
{
    return ClassicStrokeMotion::maximumDisplacement(strokeWidth, wobbleAmount);
}

qreal renderedWidth(qreal strokeWidth,
    quint64 seed,
    int frameIndex,
    int frameCount,
    qreal wobbleAmount,
    const MotionSettings &settings)
{
    if (settings.style == MotionStyle::Classic)
    {
        return ClassicStrokeMotion::renderedWidth(strokeWidth,
            seed,
            normalizedFrame(frameIndex, frameCount),
            wobbleAmount);
    }
    const PoseSample pose = poseSample(frameIndex, frameCount, settings);
    const qreal widthNoise =
        linkedRandomValue(seed, pose, 0, widthChannel, settings.linked);
    const qreal scale = std::clamp(wobbleAmount / 1.6, 0.0, 1.0);
    return std::max(0.5, strokeWidth * (1.0 + widthNoise * scale * 0.025));
}

StrokePoint displacedSample(const StrokePoint &sample,
    const QPointF &unitTangent,
    qreal arcLength,
    qsizetype sampleIndex,
    qreal amplitude,
    quint64 seed,
    int frameIndex,
    int frameCount,
    const MotionSettings &settings)
{
    if (settings.style == MotionStyle::Classic)
    {
        return ClassicStrokeMotion::displacedSample(sample,
            unitTangent,
            arcLength,
            amplitude,
            seed,
            normalizedFrame(frameIndex, frameCount));
    }

    const PoseSample pose = poseSample(frameIndex, frameCount, settings);
    const qreal detailRatio = static_cast<qreal>(settings.detail - 1) / 23.0;
    const qreal detailWavelength = interpolate(32.0, 5.0, detailRatio);
    const qreal sway = linkedSmoothValue(
        seed, pose, arcLength / 30.0, swayChannel, settings.linked);
    const qreal detail = linkedSmoothValue(seed,
        pose,
        arcLength / detailWavelength + 31.0,
        detailChannel,
        settings.linked);
    const int index = static_cast<int>(
        std::min<qsizetype>(sampleIndex, std::numeric_limits<int>::max()));
    const qreal randomNormal = linkedRandomValue(
        seed, pose, index, randomNormalChannel, settings.linked);
    const qreal randomTangent = linkedRandomValue(
        seed, pose, index, randomTangentChannel, settings.linked);
    const qreal normalOffset = interpolate(
        sway * 0.68 + detail * 0.32, randomNormal, settings.randomness);
    const qreal tangentBase = linkedSmoothValue(
        seed, pose, arcLength / 30.0 + 57.0, tangentChannel, settings.linked);
    const qreal tangentOffset =
        interpolate(tangentBase, randomTangent, settings.randomness);
    const QPointF normal(-unitTangent.y(), unitTangent.x());
    const qreal pressureFactor = 0.8 + sample.pressure * 0.2;
    StrokePoint displaced = sample;
    displaced.position += normal * normalOffset * amplitude * pressureFactor
                          + unitTangent * tangentOffset * amplitude * 0.3;
    return displaced;
}

int visibilityPose(
    int frameIndex, int frameCount, const MotionSettings &settings)
{
    return poseSample(frameIndex, frameCount, settings).first;
}

}

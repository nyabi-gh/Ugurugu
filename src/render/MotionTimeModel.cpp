// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "render/MotionTimeModel.hpp"

#include <algorithm>
#include <cmath>

namespace ugurugu::MotionTimeModel
{

namespace
{

bool validCounts(int frameCount, int poseCount)
{
    return frameCount > 0 && poseCount > 0 && poseCount <= frameCount;
}

qreal normalizedPosition(qreal framePosition, int frameCount)
{
    qreal normalized = std::fmod(framePosition, static_cast<qreal>(frameCount));
    if (normalized < 0.0)
    {
        normalized += frameCount;
    }
    return normalized;
}

int normalizedFrame(int frameIndex, int frameCount)
{
    return ((frameIndex % frameCount) + frameCount) % frameCount;
}

qreal smoothStep(qreal value)
{
    const qreal clamped = std::clamp(value, 0.0, 1.0);
    return clamped * clamped * (3.0 - 2.0 * clamped);
}

}

std::optional<SmoothPoseSample> smoothSample(
    qreal framePosition, int frameCount, int poseCount)
{
    if (!std::isfinite(framePosition) || !validCounts(frameCount, poseCount))
    {
        return std::nullopt;
    }
    if (poseCount == 1)
    {
        return SmoothPoseSample{};
    }
    const qreal phase =
        normalizedPosition(framePosition, frameCount) * poseCount / frameCount;
    const int firstPose =
        std::clamp(static_cast<int>(std::floor(phase)), 0, poseCount - 1);
    const qreal fraction = phase - firstPose;
    return SmoothPoseSample{
        firstPose, (firstPose + 1) % poseCount, smoothStep(fraction)};
}

std::optional<int> steppedPose(int frameIndex, int frameCount, int poseCount)
{
    if (!validCounts(frameCount, poseCount))
    {
        return std::nullopt;
    }
    const qint64 frame = normalizedFrame(frameIndex, frameCount);
    return static_cast<int>(frame * poseCount / frameCount);
}

}

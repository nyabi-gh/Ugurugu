// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#pragma once

#include <QtTypes>

#include <optional>

namespace ugurugu::MotionTimeModel
{

struct SmoothPoseSample
{
    int firstPose = 0;
    int secondPose = 0;
    qreal blend = 0.0;

    bool operator==(const SmoothPoseSample &) const = default;
};

// poseCount is the number of deterministic poses in one complete animation
// loop. It is independent of playback FPS and avoids a shortened final hold.
std::optional<SmoothPoseSample> smoothSample(
    qreal framePosition, int frameCount, int poseCount);
std::optional<int> steppedPose(int frameIndex, int frameCount, int poseCount);

}

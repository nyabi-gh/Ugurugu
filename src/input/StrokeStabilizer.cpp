// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "input/StrokeStabilizer.hpp"

#include <algorithm>
#include <cmath>
#include <numbers>

namespace ugurugu
{

namespace
{

// This is the 1 Euro filter (Casiez, Roussel and Vogel, CHI 2012): a low-pass
// whose cutoff rises with the filtered speed, so slow movement is smoothed
// heavily and fast movement keeps its lag low. derivativeCutoff is the
// filter's dcutoff and the paper's default. The minimum cutoff and the speed
// coefficient are its fcmin and beta; strength interpolates each between the
// weakest and strongest ends rather than exposing them separately.
constexpr qreal defaultSampleInterval = 1.0 / 120.0;
constexpr qreal minimumSampleInterval = 1.0 / 1000.0;
constexpr qreal maximumSampleInterval = 0.1;
constexpr qreal derivativeCutoff = 1.0;
constexpr qreal weakestMinimumCutoff = 18.0;
constexpr qreal strongestMinimumCutoff = 0.75;
constexpr qreal weakestSpeedCoefficient = 0.08;
constexpr qreal strongestSpeedCoefficient = 0.008;

qreal lowPassAlpha(qreal cutoff, qreal interval)
{
    const qreal timeConstant = 1.0 / (2.0 * std::numbers::pi_v<qreal> * cutoff);
    return 1.0 / (1.0 + timeConstant / interval);
}

}

qreal StrokeStabilizer::strength() const
{
    return m_strength;
}

void StrokeStabilizer::setStrength(qreal strength)
{
    if (!std::isfinite(strength))
    {
        return;
    }
    m_strength = std::clamp(strength, 0.0, 1.0);
}

void StrokeStabilizer::reset()
{
    m_initialized = false;
    m_previousRawPosition = {};
    m_filteredPosition = {};
    m_filteredVelocity = {};
    m_previousTimestamp = 0;
}

QPointF StrokeStabilizer::begin(const QPointF &position, quint64 timestamp)
{
    m_initialized = true;
    m_previousRawPosition = position;
    m_filteredPosition = position;
    m_filteredVelocity = {};
    m_previousTimestamp = timestamp;
    return position;
}

QPointF StrokeStabilizer::update(const QPointF &position, quint64 timestamp)
{
    if (!m_initialized)
    {
        return begin(position, timestamp);
    }

    const qreal interval = sampleInterval(timestamp);
    const QPointF rawVelocity = (position - m_previousRawPosition) / interval;
    const qreal velocityAlpha = lowPassAlpha(derivativeCutoff, interval);
    m_filteredVelocity += (rawVelocity - m_filteredVelocity) * velocityAlpha;

    if (qFuzzyIsNull(m_strength))
    {
        m_filteredPosition = position;
        rememberRawSample(position, timestamp);
        return position;
    }

    const qreal minimumCutoff =
        std::lerp(weakestMinimumCutoff, strongestMinimumCutoff, m_strength);
    const qreal speedCoefficient = std::lerp(
        weakestSpeedCoefficient, strongestSpeedCoefficient, m_strength);
    const qreal speed =
        std::hypot(m_filteredVelocity.x(), m_filteredVelocity.y());
    const qreal adaptiveAlpha =
        lowPassAlpha(minimumCutoff + speedCoefficient * speed, interval);
    const qreal positionAlpha = std::lerp(1.0, adaptiveAlpha, m_strength);
    m_filteredPosition += (position - m_filteredPosition) * positionAlpha;
    rememberRawSample(position, timestamp);
    return m_filteredPosition;
}

QPointF StrokeStabilizer::finish(const QPointF &position, quint64 timestamp)
{
    if (!m_initialized)
    {
        return begin(position, timestamp);
    }
    // The stroke must end on the position the user actually lifted at, so the
    // filter state is snapped to the raw sample instead of being advanced.
    m_filteredPosition = position;
    m_filteredVelocity = {};
    rememberRawSample(position, timestamp);
    return position;
}

qreal StrokeStabilizer::sampleInterval(quint64 timestamp) const
{
    if (timestamp <= m_previousTimestamp)
    {
        return defaultSampleInterval;
    }
    return std::clamp(
        static_cast<qreal>(timestamp - m_previousTimestamp) / 1000.0,
        minimumSampleInterval,
        maximumSampleInterval);
}

void StrokeStabilizer::rememberRawSample(
    const QPointF &position, quint64 timestamp)
{
    m_previousRawPosition = position;
    m_previousTimestamp = timestamp;
}

}

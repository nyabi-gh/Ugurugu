// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "render/StrokeRenderer.hpp"

#include "document/DocumentLimits.hpp"
#include "render/BrokenLineModel.hpp"
#include "render/ClassicStrokeMotion.hpp"
#include "render/DeterministicNoise.hpp"
#include "render/StrokeMotionModel.hpp"

#include <QRadialGradient>

#include <algorithm>
#include <cmath>
#include <numbers>

namespace ugurugu::StrokeRenderer
{

namespace
{

constexpr qsizetype maximumResampledPoints =
    DocumentLimits::maximumPointsPerStroke;
constexpr qsizetype maximumBrushDabs = 50000;
constexpr qsizetype maximumSprayParticles = 250000;

bool validPoint(const StrokePoint &point)
{
    return std::isfinite(point.position.x())
           && std::isfinite(point.position.y()) && std::isfinite(point.pressure)
           && std::abs(point.position.x())
                  <= DocumentLimits::maximumStoredCoordinateMagnitude
           && std::abs(point.position.y())
                  <= DocumentLimits::maximumStoredCoordinateMagnitude
           && point.pressure >= 0.0 && point.pressure <= 1.0;
}

QPointF normalized(const QPointF &value)
{
    const qreal length = std::hypot(value.x(), value.y());
    if (length <= 0.00001)
    {
        return QPointF(1.0, 0.0);
    }
    return value / length;
}

QPainterPath smoothedPath(const QVector<StrokePoint> &points)
{
    QPainterPath result;
    if (points.isEmpty())
    {
        return result;
    }
    result.moveTo(points.first().position);
    if (points.size() == 1)
    {
        return result;
    }
    for (int index = 1; index < points.size() - 1; ++index)
    {
        const QPointF midpoint =
            (points[index].position + points[index + 1].position) * 0.5;
        result.quadTo(points[index].position, midpoint);
    }
    result.lineTo(points.last().position);
    return result;
}

qreal pressureScale(qreal dynamics, qreal pressure)
{
    const qreal normalizedDynamics = std::clamp(dynamics, 0.0, 1.0);
    return 1.0 - normalizedDynamics
           + std::clamp(pressure, 0.0, 1.0) * normalizedDynamics;
}

qreal pressureWidth(qreal baseWidth, qreal pressure, qreal sizeDynamics)
{
    return std::max(0.5, baseWidth * pressureScale(sizeDynamics, pressure));
}

QColor colorWithOpacity(const QColor &color, qreal opacity)
{
    QColor adjusted = color;
    adjusted.setAlpha(std::clamp(qRound(static_cast<qreal>(color.alpha())
                                        * std::clamp(opacity, 0.0, 1.0)),
        0,
        255));
    return adjusted;
}

void drawPath(QPainter &painter,
    const QPainterPath &path,
    const QColor &color,
    qreal width,
    BrushTipShape tipShape)
{
    const Qt::PenCapStyle capStyle =
        tipShape == BrushTipShape::Square ? Qt::SquareCap : Qt::RoundCap;
    const Qt::PenJoinStyle joinStyle =
        tipShape == BrushTipShape::Square ? Qt::MiterJoin : Qt::RoundJoin;
    painter.setPen(QPen(color, width, Qt::SolidLine, capStyle, joinStyle));
    painter.drawPath(path);
}

void drawLineDot(QPainter &painter,
    const StrokePoint &point,
    const QColor &color,
    qreal baseWidth,
    const BrushSettings &brush)
{
    const qreal width =
        pressureWidth(baseWidth, point.pressure, brush.sizeDynamics);
    const QColor adjusted = colorWithOpacity(color,
        brush.opacity * pressureScale(brush.opacityDynamics, point.pressure));
    painter.setPen(Qt::NoPen);
    painter.setBrush(adjusted);
    if (brush.tipShape == BrushTipShape::Square)
    {
        painter.drawRect(QRectF(point.position.x() - width * 0.5,
            point.position.y() - width * 0.5,
            width,
            width));
    }
    else
    {
        painter.drawEllipse(point.position, width * 0.5, width * 0.5);
    }
}

// One place decides how the runs of a single line stroke become ink, so that
// a caller holding only the runs that reach one tile cannot rasterize them
// differently from a caller holding the whole stroke. At a constant pressure
// the runs are subpaths of one path, stroked in a single pass: a crossing
// between two of them is then one coverage computation and blends once, as it
// does in a full render. Variable pressure has to draw each segment on its
// own because each carries its own width.
void drawLineStrokeRuns(QPainter &painter,
    const QVector<QVector<StrokePoint>> &runs,
    const QColor &color,
    qreal baseWidth,
    const BrushSettings &brush,
    bool variablePressure);

void drawLineStroke(QPainter &painter,
    const QVector<StrokePoint> &points,
    const QColor &color,
    qreal baseWidth,
    const BrushSettings &brush,
    bool variablePressure)
{
    if (points.size() < 2)
    {
        return;
    }
    if (!variablePressure)
    {
        drawLineStrokeRuns(
            painter, {points}, color, baseWidth, brush, variablePressure);
        return;
    }

    const QPointF firstMidpoint =
        (points[0].position + points[1].position) * 0.5;
    QPainterPath firstSegment;
    firstSegment.moveTo(points[0].position);
    firstSegment.lineTo(firstMidpoint);
    drawPath(painter,
        firstSegment,
        colorWithOpacity(color,
            brush.opacity
                * pressureScale(brush.opacityDynamics,
                    (points[0].pressure + points[1].pressure) * 0.5)),
        pressureWidth(baseWidth,
            (points[0].pressure + points[1].pressure) * 0.5,
            brush.sizeDynamics),
        brush.tipShape);

    for (int index = 1; index < points.size() - 1; ++index)
    {
        const QPointF start =
            (points[index - 1].position + points[index].position) * 0.5;
        const QPointF end =
            (points[index].position + points[index + 1].position) * 0.5;
        QPainterPath segment;
        segment.moveTo(start);
        segment.quadTo(points[index].position, end);
        drawPath(painter,
            segment,
            colorWithOpacity(color,
                brush.opacity
                    * pressureScale(
                        brush.opacityDynamics, points[index].pressure)),
            pressureWidth(
                baseWidth, points[index].pressure, brush.sizeDynamics),
            brush.tipShape);
    }

    const int lastIndex = static_cast<int>(points.size()) - 1;
    const QPointF lastMidpoint =
        (points[lastIndex - 1].position + points[lastIndex].position) * 0.5;
    QPainterPath lastSegment;
    lastSegment.moveTo(lastMidpoint);
    lastSegment.lineTo(points[lastIndex].position);
    drawPath(painter,
        lastSegment,
        colorWithOpacity(color,
            brush.opacity
                * pressureScale(brush.opacityDynamics,
                    (points[lastIndex - 1].pressure
                        + points[lastIndex].pressure)
                        * 0.5)),
        pressureWidth(baseWidth,
            (points[lastIndex - 1].pressure + points[lastIndex].pressure) * 0.5,
            brush.sizeDynamics),
        brush.tipShape);
}

void drawLineStrokeRuns(QPainter &painter,
    const QVector<QVector<StrokePoint>> &runs,
    const QColor &color,
    qreal baseWidth,
    const BrushSettings &brush,
    bool variablePressure)
{
    if (variablePressure)
    {
        for (const QVector<StrokePoint> &points : runs)
        {
            drawLineStroke(
                painter, points, color, baseWidth, brush, variablePressure);
        }
        return;
    }
    QPainterPath merged;
    qreal pressure = 1.0;
    bool havePressure = false;
    for (const QVector<StrokePoint> &points : runs)
    {
        if (points.size() < 2)
        {
            continue;
        }
        if (!havePressure)
        {
            pressure = points.first().pressure;
            havePressure = true;
        }
        merged.addPath(smoothedPath(points));
    }
    if (!havePressure)
    {
        return;
    }
    drawPath(painter,
        merged,
        colorWithOpacity(color,
            brush.opacity * pressureScale(brush.opacityDynamics, pressure)),
        pressureWidth(baseWidth, pressure, brush.sizeDynamics),
        brush.tipShape);
}

void drawAirbrushDab(QPainter &painter,
    const StrokePoint &point,
    const QColor &color,
    qreal baseWidth,
    const BrushSettings &brush)
{
    const qreal diameter =
        pressureWidth(baseWidth, point.pressure, brush.sizeDynamics);
    const QColor dabColor = colorWithOpacity(color,
        brush.opacity * brush.flow
            * pressureScale(brush.opacityDynamics, point.pressure));
    if (diameter <= 0.0 || dabColor.alpha() <= 0)
    {
        return;
    }
    const QRectF bounds(point.position.x() - diameter * 0.5,
        point.position.y() - diameter * 0.5,
        diameter,
        diameter);
    painter.setPen(Qt::NoPen);
    if (brush.tipShape == BrushTipShape::Square || brush.hardness >= 0.995)
    {
        painter.setBrush(dabColor);
        if (brush.tipShape == BrushTipShape::Square)
        {
            painter.drawRect(bounds);
        }
        else
        {
            painter.drawEllipse(bounds);
        }
        return;
    }

    QRadialGradient gradient(point.position, diameter * 0.5);
    const qreal innerStop = std::clamp(brush.hardness, 0.0, 0.98);
    gradient.setColorAt(0.0, dabColor);
    if (innerStop > 0.001)
    {
        gradient.setColorAt(innerStop, dabColor);
    }
    QColor edge = dabColor;
    edge.setAlpha(0);
    gradient.setColorAt(1.0, edge);
    painter.setBrush(gradient);
    painter.save();
    painter.setRenderHint(QPainter::Antialiasing, true);
    painter.drawEllipse(bounds);
    painter.restore();
}

void drawSprayDab(QPainter &painter,
    const StrokePoint &point,
    const QColor &color,
    qreal baseWidth,
    const BrushSettings &brush,
    quint64 seed,
    int frameIndex,
    int pointIndex)
{
    const int particlesPerPoint =
        std::clamp(qRound(brush.density * 6.0), 1, 24);
    const int noiseFrame =
        brush.animatedJitter && brush.wobbleScale > 0.0 ? frameIndex : 0;
    const qreal pressureSize =
        pressureScale(brush.sizeDynamics, point.pressure);
    const QColor particleColor = colorWithOpacity(color,
        brush.opacity * brush.flow
            * pressureScale(brush.opacityDynamics, point.pressure));
    painter.setBrush(particleColor);
    for (int particleIndex = 0; particleIndex < particlesPerPoint;
        ++particleIndex)
    {
        const qsizetype emitted =
            static_cast<qsizetype>(pointIndex) * particlesPerPoint
            + particleIndex;
        if (emitted >= maximumSprayParticles)
        {
            return;
        }
        const int noiseIndex = static_cast<int>(emitted);
        const qreal angle = DeterministicNoise::unitValue(
                                seed, noiseFrame, noiseIndex, 0x36d1a53bULL)
                            * std::numbers::pi * 2.0;
        const qreal radius = std::sqrt(DeterministicNoise::unitValue(
                                 seed, noiseFrame, noiseIndex, 0x9c8e31d7ULL))
                             * brush.scatter * baseWidth * 0.5;
        const QPointF position =
            point.position + QPointF(std::cos(angle), std::sin(angle)) * radius;
        const qreal jitter = 1.0
                             + DeterministicNoise::signedValue(
                                   seed, noiseFrame, noiseIndex, 0xa24baed4ULL)
                                   * brush.sizeJitter * 0.75;
        const qreal particleDiameter = std::max(0.5,
            baseWidth * brush.particleSize * pressureSize
                * std::max(0.1, jitter));
        const QRectF bounds(position.x() - particleDiameter * 0.5,
            position.y() - particleDiameter * 0.5,
            particleDiameter,
            particleDiameter);
        if (brush.tipShape == BrushTipShape::Square)
        {
            painter.drawRect(bounds);
        }
        else
        {
            painter.drawEllipse(bounds);
        }
    }
}

qsizetype maximumPointsFor(const Stroke &stroke)
{
    return stroke.brush.engine == BrushEngine::Line ? maximumResampledPoints
                                                    : maximumBrushDabs;
}

qreal spacingFor(const Stroke &stroke, qreal width)
{
    return stroke.brush.engine == BrushEngine::Line
               ? std::clamp(stroke.width * 0.55, 2.0, 5.0)
               : std::max(0.5, width * stroke.brush.spacing);
}

bool sameIdentity(const Stroke &left, const Stroke &right)
{
    return left.id == right.id && left.seed == right.seed
           && left.mode == right.mode && left.color == right.color
           && left.width == right.width && left.brush == right.brush
           && left.visibilityClip == right.visibilityClip
           && left.clipMask.cacheKey() == right.clipMask.cacheKey();
}

bool segmentVisible(const PreparedStroke &prepared, int index)
{
    return prepared.visibleSegments.isEmpty()
           || (index >= 0 && index < prepared.visibleSegments.size()
               && prepared.visibleSegments[index] != 0);
}

bool pointVisible(const PreparedStroke &prepared, int index)
{
    if (prepared.visibleSegments.isEmpty() || prepared.points.size() == 1)
    {
        return true;
    }
    return segmentVisible(prepared, index - 1)
           || segmentVisible(prepared, index);
}

QPainterPath preparedPath(const PreparedStroke &prepared)
{
    if (prepared.visibleSegments.isEmpty())
    {
        return smoothedPath(prepared.points);
    }
    QPainterPath path;
    for (const BrokenLineModel::VisibleRun &run :
        BrokenLineModel::visibleRuns(prepared.visibleSegments))
    {
        path.addPath(smoothedPath(prepared.points.sliced(
            run.firstPoint, run.lastPoint - run.firstPoint + 1)));
    }
    return path;
}

}

const PreparedStroke &IncrementalGeometry::prepared() const
{
    return m_prepared;
}

GeometryUpdate IncrementalGeometry::update(
    const Stroke &stroke, int frameIndex, int frameCount, qreal wobbleAmount)
{
    return updateConfigured(
        stroke, frameIndex, frameCount, wobbleAmount, MotionSettings{});
}

GeometryUpdate IncrementalGeometry::update(
    const Stroke &stroke, int frameIndex, const Document &document)
{
    return updateConfigured(stroke,
        frameIndex,
        document.animationFrames,
        document.wobbleAmount,
        document.motion);
}

GeometryUpdate IncrementalGeometry::updateConfigured(const Stroke &stroke,
    int frameIndex,
    int frameCount,
    qreal wobbleAmount,
    const MotionSettings &motion)
{
    GeometryUpdate result;
    if (stroke.points.isEmpty() || !isValidBrushSettings(stroke.brush)
        || stroke.points.size() > DocumentLimits::maximumPointsPerStroke)
    {
        clear();
        return result;
    }
    const int normalizedCount = std::max(1, frameCount);
    const int normalizedFrame =
        ((frameIndex % normalizedCount) + normalizedCount) % normalizedCount;
    const qreal strokeWobble = wobbleAmount * stroke.brush.wobbleScale;
    const qreal width = StrokeMotionModel::renderedWidth(stroke.width,
        stroke.seed,
        normalizedFrame,
        normalizedCount,
        strokeWobble,
        motion);
    const qreal spacing = spacingFor(stroke, width);
    const qsizetype maximum = maximumPointsFor(stroke);
    const bool canAppend =
        matches(stroke, normalizedFrame, normalizedCount, wobbleAmount, motion)
        && !m_capped && stroke.points.size() > m_sourcePointCount
        && m_sourcePointCount > 0
        && stroke.points[m_sourcePointCount - 1]
               == m_identity.points.constLast();
    const qsizetype oldRegularCount = m_regularSamples.size();
    const bool oldVariablePressure = m_prepared.variablePressure;

    if (!canAppend)
    {
        if (!rebuildSamples(stroke, spacing, maximum))
        {
            clear();
            return result;
        }
        result.changedFrom = 0;
        result.sourcePointsProcessed = stroke.points.size();
        result.rebuilt = true;
    }
    else
    {
        const qsizetype appended = stroke.points.size() - m_sourcePointCount;
        if (!appendSamples(stroke))
        {
            clear();
            return result;
        }
        if (m_capped)
        {
            if (!rebuildSamples(stroke, spacing, maximum))
            {
                clear();
                return result;
            }
            result.changedFrom = 0;
            result.sourcePointsProcessed = stroke.points.size();
            result.rebuilt = true;
        }
        else
        {
            result.changedFrom = std::max<qsizetype>(0, oldRegularCount - 1);
            result.sourcePointsProcessed = appended;
        }
    }

    m_prepared.width = width;
    m_prepared.normalizedFrame = normalizedFrame;
    qreal minimumPressure = m_regularMinimumPressure;
    qreal maximumPressure = m_regularMaximumPressure;
    if (m_samples.size() > m_regularSamples.size())
    {
        minimumPressure =
            std::min(minimumPressure, m_samples.constLast().pressure);
        maximumPressure =
            std::max(maximumPressure, m_samples.constLast().pressure);
    }
    m_prepared.variablePressure = maximumPressure - minimumPressure >= 0.01;
    m_prepared.valid = true;
    displaceSamples(stroke,
        normalizedFrame,
        normalizedCount,
        strokeWobble,
        motion,
        result.changedFrom);
    m_prepared.visibleSegments =
        motion.brokenLine ? BrokenLineModel::visibleSegments(m_prepared.points,
                                stroke.seed,
                                StrokeMotionModel::visibilityPose(
                                    normalizedFrame, normalizedCount, motion),
                                motion.breakAmount,
                                motion.breakRange)
                          : QVector<quint8>();
    result.renderingModeChanged =
        oldVariablePressure != m_prepared.variablePressure;
    if (result.renderingModeChanged)
    {
        result.changedFrom = 0;
    }
    m_identity = stroke;
    m_identity.points = {stroke.points.constLast()};
    m_frameCount = normalizedCount;
    m_normalizedFrame = normalizedFrame;
    m_wobbleAmount = wobbleAmount;
    m_motion = motion;
    m_spacing = spacing;
    m_maximumPoints = maximum;
    m_sourcePointCount = stroke.points.size();
    result.valid = true;
    return result;
}

void IncrementalGeometry::clear()
{
    m_prepared = {};
    m_regularSamples.clear();
    m_samples.clear();
    m_arcLengths.clear();
    m_identity = {};
    m_spacing = 0.0;
    m_distanceToNext = 0.0;
    m_totalLength = 0.0L;
    m_sourcePointCount = 0;
    m_maximumPoints = 0;
    m_frameCount = 0;
    m_normalizedFrame = 0;
    m_wobbleAmount = 0.0;
    m_motion = {};
    m_regularMinimumPressure = 1.0;
    m_regularMaximumPressure = 0.0;
    m_capped = false;
}

bool IncrementalGeometry::matches(const Stroke &stroke,
    int normalizedFrame,
    int frameCount,
    qreal wobbleAmount,
    const MotionSettings &motion) const
{
    return m_prepared.valid && sameIdentity(stroke, m_identity)
           && m_normalizedFrame == normalizedFrame && m_frameCount == frameCount
           && m_wobbleAmount == wobbleAmount && m_motion == motion;
}

bool IncrementalGeometry::rebuildSamples(
    const Stroke &stroke, qreal spacing, qsizetype maximum)
{
    if (!std::isfinite(spacing) || spacing <= 0.0 || maximum < 2
        || maximum > maximumResampledPoints)
    {
        return false;
    }
    m_totalLength = 0.0L;
    for (int index = 0; index < stroke.points.size(); ++index)
    {
        if (!validPoint(stroke.points[index]))
        {
            return false;
        }
        if (index > 0)
        {
            const long double x =
                static_cast<long double>(stroke.points[index].position.x())
                - static_cast<long double>(
                    stroke.points[index - 1].position.x());
            const long double y =
                static_cast<long double>(stroke.points[index].position.y())
                - static_cast<long double>(
                    stroke.points[index - 1].position.y());
            m_totalLength += std::hypot(x, y);
        }
    }
    const long double minimumSpacing =
        m_totalLength / static_cast<long double>(maximum - 1);
    const qreal effectiveSpacing =
        std::max(spacing, static_cast<qreal>(minimumSpacing));
    m_capped = effectiveSpacing > spacing;
    m_spacing = effectiveSpacing;
    m_maximumPoints = maximum;
    m_regularSamples.clear();
    m_regularSamples.reserve(std::min(maximum, stroke.points.size() * 2));
    m_regularSamples.append(stroke.points.first());
    m_regularMinimumPressure = stroke.points.first().pressure;
    m_regularMaximumPressure = stroke.points.first().pressure;
    StrokePoint previous = stroke.points.first();
    m_distanceToNext = effectiveSpacing;
    for (int index = 1; index < stroke.points.size(); ++index)
    {
        const StrokePoint target = stroke.points[index];
        QPointF delta = target.position - previous.position;
        qreal segmentLength = std::hypot(delta.x(), delta.y());
        while (segmentLength >= m_distanceToNext && segmentLength > 0.0)
        {
            if (m_regularSamples.size() >= maximum - 1)
            {
                m_capped = true;
                break;
            }
            const qreal ratio = m_distanceToNext / segmentLength;
            StrokePoint point;
            point.position = previous.position + delta * ratio;
            point.pressure = previous.pressure
                             + (target.pressure - previous.pressure) * ratio;
            m_regularSamples.append(point);
            m_regularMinimumPressure =
                std::min(m_regularMinimumPressure, point.pressure);
            m_regularMaximumPressure =
                std::max(m_regularMaximumPressure, point.pressure);
            previous = point;
            delta = target.position - previous.position;
            segmentLength = std::hypot(delta.x(), delta.y());
            m_distanceToNext = effectiveSpacing;
        }
        if (m_capped && m_regularSamples.size() >= maximum - 1)
        {
            break;
        }
        m_distanceToNext -= segmentLength;
        previous = target;
    }
    m_samples = m_regularSamples;
    const QPointF tail =
        stroke.points.last().position - m_samples.last().position;
    if (std::hypot(tail.x(), tail.y()) > 0.01 && m_samples.size() < maximum)
    {
        m_samples.append(stroke.points.last());
    }
    return !m_samples.isEmpty();
}

bool IncrementalGeometry::appendSamples(const Stroke &stroke)
{
    const qsizetype previousRegularCount = m_regularSamples.size();
    StrokePoint previous = m_identity.points.constLast();
    long double addedLength = 0.0L;
    for (qsizetype index = m_sourcePointCount; index < stroke.points.size();
        ++index)
    {
        const StrokePoint &target = stroke.points[index];
        if (!validPoint(target))
        {
            return false;
        }
        addedLength +=
            std::hypot(static_cast<long double>(target.position.x())
                           - static_cast<long double>(previous.position.x()),
                static_cast<long double>(target.position.y())
                    - static_cast<long double>(previous.position.y()));
        previous = target;
    }
    m_totalLength += addedLength;
    const long double minimumSpacing =
        m_totalLength / static_cast<long double>(m_maximumPoints - 1);
    if (minimumSpacing > static_cast<long double>(m_spacing))
    {
        m_capped = true;
        return true;
    }

    previous = m_identity.points.constLast();
    for (qsizetype index = m_sourcePointCount; index < stroke.points.size();
        ++index)
    {
        const StrokePoint target = stroke.points[index];
        QPointF delta = target.position - previous.position;
        qreal segmentLength = std::hypot(delta.x(), delta.y());
        while (segmentLength >= m_distanceToNext && segmentLength > 0.0)
        {
            if (m_regularSamples.size() >= m_maximumPoints - 1)
            {
                m_capped = true;
                return true;
            }
            const qreal ratio = m_distanceToNext / segmentLength;
            StrokePoint point;
            point.position = previous.position + delta * ratio;
            point.pressure = previous.pressure
                             + (target.pressure - previous.pressure) * ratio;
            m_regularSamples.append(point);
            m_regularMinimumPressure =
                std::min(m_regularMinimumPressure, point.pressure);
            m_regularMaximumPressure =
                std::max(m_regularMaximumPressure, point.pressure);
            previous = point;
            delta = target.position - previous.position;
            segmentLength = std::hypot(delta.x(), delta.y());
            m_distanceToNext = m_spacing;
        }
        m_distanceToNext -= segmentLength;
        previous = target;
    }
    m_samples.resize(previousRegularCount);
    for (qsizetype index = previousRegularCount;
        index < m_regularSamples.size();
        ++index)
    {
        m_samples.append(m_regularSamples[index]);
    }
    const QPointF tail =
        stroke.points.last().position - m_samples.last().position;
    if (std::hypot(tail.x(), tail.y()) > 0.01
        && m_samples.size() < m_maximumPoints)
    {
        m_samples.append(stroke.points.last());
    }
    return true;
}

void IncrementalGeometry::displaceSamples(const Stroke &stroke,
    int normalizedFrame,
    int frameCount,
    qreal wobbleAmount,
    const MotionSettings &motion,
    qsizetype changedFrom)
{
    changedFrom = std::clamp<qsizetype>(changedFrom, 0, m_samples.size());
    m_prepared.points.resize(m_samples.size());
    m_arcLengths.resize(m_samples.size());
    qreal arcLength = changedFrom > 0 ? m_arcLengths[changedFrom - 1] : 0.0;
    if (changedFrom == 0 && !m_samples.isEmpty())
    {
        m_arcLengths[0] = 0.0;
    }
    const qreal amplitude =
        ClassicStrokeMotion::displacementAmplitude(stroke.width, wobbleAmount);
    for (qsizetype index = changedFrom; index < m_samples.size(); ++index)
    {
        if (index > 0)
        {
            const QPointF step =
                m_samples[index].position - m_samples[index - 1].position;
            arcLength += std::hypot(step.x(), step.y());
            m_arcLengths[index] = arcLength;
        }
        const StrokePoint &sample = m_samples[index];
        const QPointF before =
            m_samples[std::max<qsizetype>(0, index - 1)].position;
        const QPointF after =
            m_samples[std::min<qsizetype>(m_samples.size() - 1, index + 1)]
                .position;
        const QPointF tangent = normalized(after - before);
        m_prepared.points[index] = StrokeMotionModel::displacedSample(sample,
            tangent,
            arcLength,
            index,
            amplitude,
            stroke.seed,
            normalizedFrame,
            frameCount,
            motion);
    }
}

PreparedStroke prepare(
    const Stroke &stroke, int frameIndex, int frameCount, qreal wobbleAmount)
{
    IncrementalGeometry geometry;
    geometry.update(stroke, frameIndex, frameCount, wobbleAmount);
    return geometry.prepared();
}

PreparedStroke prepare(
    const Stroke &stroke, int frameIndex, const Document &document)
{
    IncrementalGeometry geometry;
    geometry.update(stroke, frameIndex, document);
    return geometry.prepared();
}

QPainterPath path(
    const Stroke &stroke, int frameIndex, int frameCount, qreal wobbleAmount)
{
    return smoothedPath(
        prepare(stroke, frameIndex, frameCount, wobbleAmount).points);
}

QPainterPath path(
    const Stroke &stroke, int frameIndex, const Document &document)
{
    return preparedPath(prepare(stroke, frameIndex, document));
}

void paint(
    QPainter &painter, const Stroke &stroke, const PreparedStroke &prepared)
{
    if (!prepared.valid || prepared.points.isEmpty())
    {
        return;
    }
    const QColor color =
        stroke.mode == StrokeMode::Erase ? Qt::black : stroke.color;
    switch (stroke.brush.engine)
    {
    case BrushEngine::Line:
        if (prepared.points.size() == 1)
        {
            drawLineDot(painter,
                prepared.points.first(),
                color,
                prepared.width,
                stroke.brush);
        }
        else
        {
            if (prepared.visibleSegments.isEmpty())
            {
                drawLineStroke(painter,
                    prepared.points,
                    color,
                    prepared.width,
                    stroke.brush,
                    prepared.variablePressure);
            }
            else
            {
                for (const BrokenLineModel::VisibleRun &run :
                    BrokenLineModel::visibleRuns(prepared.visibleSegments))
                {
                    drawLineStroke(painter,
                        prepared.points.sliced(
                            run.firstPoint, run.lastPoint - run.firstPoint + 1),
                        color,
                        prepared.width,
                        stroke.brush,
                        prepared.variablePressure);
                }
            }
        }
        break;
    case BrushEngine::Airbrush:
        for (int index = 0; index < prepared.points.size(); ++index)
        {
            if (pointVisible(prepared, index))
            {
                drawAirbrushDab(painter,
                    prepared.points[index],
                    color,
                    prepared.width,
                    stroke.brush);
            }
        }
        break;
    case BrushEngine::Spray:
        painter.setRenderHint(QPainter::Antialiasing, false);
        painter.setPen(Qt::NoPen);
        for (int index = 0; index < prepared.points.size(); ++index)
        {
            if (!pointVisible(prepared, index))
            {
                continue;
            }
            drawSprayDab(painter,
                prepared.points[index],
                color,
                prepared.width,
                stroke.brush,
                stroke.seed,
                prepared.normalizedFrame,
                index);
        }
        break;
    }
}

void paintPrimitives(QPainter &painter,
    const Stroke &stroke,
    const PreparedStroke &prepared,
    const QVector<int> &primitiveIndexes)
{
    if (!prepared.valid || primitiveIndexes.isEmpty())
    {
        return;
    }
    QVector<int> normalizedIndexes;
    const QVector<int> *orderedIndexes = &primitiveIndexes;
    if (!std::is_sorted(primitiveIndexes.cbegin(), primitiveIndexes.cend())
        || std::adjacent_find(
               primitiveIndexes.cbegin(), primitiveIndexes.cend())
               != primitiveIndexes.cend())
    {
        normalizedIndexes = primitiveIndexes;
        std::sort(normalizedIndexes.begin(), normalizedIndexes.end());
        normalizedIndexes.erase(
            std::unique(normalizedIndexes.begin(), normalizedIndexes.end()),
            normalizedIndexes.end());
        orderedIndexes = &normalizedIndexes;
    }
    const QColor color =
        stroke.mode == StrokeMode::Erase ? Qt::black : stroke.color;
    if (stroke.brush.engine == BrushEngine::Airbrush)
    {
        for (const int index : *orderedIndexes)
        {
            if (index >= 0 && index < prepared.points.size()
                && pointVisible(prepared, index))
            {
                drawAirbrushDab(painter,
                    prepared.points[index],
                    color,
                    prepared.width,
                    stroke.brush);
            }
        }
        return;
    }
    if (stroke.brush.engine == BrushEngine::Spray)
    {
        painter.setRenderHint(QPainter::Antialiasing, false);
        painter.setPen(Qt::NoPen);
        for (const int index : *orderedIndexes)
        {
            if (index >= 0 && index < prepared.points.size()
                && pointVisible(prepared, index))
            {
                drawSprayDab(painter,
                    prepared.points[index],
                    color,
                    prepared.width,
                    stroke.brush,
                    stroke.seed,
                    prepared.normalizedFrame,
                    index);
            }
        }
        return;
    }
    if (prepared.points.size() == 1)
    {
        drawLineDot(painter,
            prepared.points.first(),
            color,
            prepared.width,
            stroke.brush);
        return;
    }

    struct Range
    {
        int first;
        int last;
    };
    QVector<Range> ranges;
    for (const int primitive : *orderedIndexes)
    {
        if (primitive < 0 || primitive >= prepared.points.size() - 1)
        {
            continue;
        }
        if (!segmentVisible(prepared, primitive))
        {
            continue;
        }
        int first = primitive;
        int last = primitive;
        for (int offset = 0;
            offset < 2 && first > 0 && segmentVisible(prepared, first - 1);
            ++offset)
        {
            --first;
        }
        for (int offset = 0; offset < 2 && last < prepared.points.size() - 2
                             && segmentVisible(prepared, last + 1);
            ++offset)
        {
            ++last;
        }
        if (!ranges.isEmpty() && first <= ranges.last().last + 1)
        {
            ranges.last().last = std::max(ranges.last().last, last);
        }
        else
        {
            ranges.append({first, last});
        }
    }
    // Ranges are only the pieces of the line this caller asked about, and a
    // full render would hand every piece of one visible run to a single
    // drawLineStrokeRuns. Group them the same way, so ink that crosses itself
    // between two pieces blends exactly as often as it does there. A run ends
    // where the line turns invisible, because a full render stops its stroke
    // at that point too.
    QVector<QVector<StrokePoint>> runs;
    int previousLast = -1;
    for (const Range &range : ranges)
    {
        QVector<StrokePoint> points;
        points.reserve(range.last - range.first + 2);
        for (int index = range.first; index <= range.last + 1; ++index)
        {
            points.append(prepared.points[index]);
        }
        bool sameRun = previousLast >= 0;
        for (int index = previousLast + 1; sameRun && index < range.first;
            ++index)
        {
            sameRun = segmentVisible(prepared, index);
        }
        if (!sameRun && previousLast >= 0)
        {
            drawLineStrokeRuns(painter,
                runs,
                color,
                prepared.width,
                stroke.brush,
                prepared.variablePressure);
            runs.clear();
        }
        runs.append(std::move(points));
        previousLast = range.last;
    }
    drawLineStrokeRuns(painter,
        runs,
        color,
        prepared.width,
        stroke.brush,
        prepared.variablePressure);
}

QRectF primitiveBounds(
    const Stroke &stroke, const PreparedStroke &prepared, int primitiveIndex)
{
    if (!prepared.valid || primitiveIndex < 0
        || primitiveIndex >= primitiveCount(stroke, prepared))
    {
        return {};
    }
    // QPen's default miter limit can extend square-tip joins to twice the pen
    // width.
    const qreal lineReach =
        prepared.width
            * (stroke.brush.tipShape == BrushTipShape::Square ? 2.1 : 0.5)
        + 3.0;
    qreal reach = lineReach;
    if (stroke.brush.engine == BrushEngine::Spray)
    {
        reach = prepared.width
                    * (stroke.brush.scatter * 0.5
                        + stroke.brush.particleSize
                              * (1.0 + stroke.brush.sizeJitter * 0.75) * 0.5)
                + 3.0;
    }
    const QPointF first = prepared.points[primitiveIndex].position;
    if (stroke.brush.engine != BrushEngine::Line || prepared.points.size() == 1)
    {
        if (!pointVisible(prepared, primitiveIndex))
        {
            return {};
        }
        return QRectF(
            first.x() - reach, first.y() - reach, reach * 2.0, reach * 2.0);
    }
    if (!segmentVisible(prepared, primitiveIndex))
    {
        return {};
    }
    const QPointF second = prepared.points[primitiveIndex + 1].position;
    return QRectF(first, second)
        .normalized()
        .adjusted(-reach, -reach, reach, reach);
}

int primitiveCount(const Stroke &stroke, const PreparedStroke &prepared)
{
    if (!prepared.valid || prepared.points.isEmpty())
    {
        return 0;
    }
    return stroke.brush.engine == BrushEngine::Line
               ? std::max(1, static_cast<int>(prepared.points.size()) - 1)
               : static_cast<int>(prepared.points.size());
}

bool repaintIsIdempotent(const Stroke &stroke, const PreparedStroke &prepared)
{
    // An antialiased edge carries partial coverage, and partial coverage
    // composited over itself darkens.
    if (stroke.brush.antialiasing)
    {
        return false;
    }
    // drawLineStroke gives the half segments at either end of a range the
    // average of two pressures instead of the width their own point asks for,
    // so a range that starts elsewhere is a different shape, not a repeat.
    if (prepared.variablePressure)
    {
        return false;
    }
    if (stroke.mode != StrokeMode::Paint && stroke.mode != StrokeMode::Erase)
    {
        return false;
    }
    // Every alpha the dab and path painters can apply on top of the colour.
    if (stroke.brush.opacity < 1.0 || stroke.brush.flow < 1.0
        || stroke.brush.opacityDynamics > 0.0)
    {
        return false;
    }
    // Erasing paints coverage into the destination alpha, so the colour plays
    // no part in it.
    return stroke.mode == StrokeMode::Erase || stroke.color.alpha() == 255;
}

}

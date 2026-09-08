// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "ui/ColorWheel.hpp"

#include <QMouseEvent>
#include <QPainter>
#include <QPainterPath>
#include <QResizeEvent>

#include <algorithm>
#include <cmath>
#include <numbers>

namespace ugurugu
{

namespace
{

constexpr qreal margin = 4.0;
constexpr qreal ringGap = 6.0;
constexpr qreal markerRadius = 6.0;
constexpr qreal tau = 2.0 * std::numbers::pi_v<qreal>;

qreal wrapUnit(qreal value)
{
    const qreal wrapped = std::fmod(value, 1.0);
    return wrapped < 0.0 ? wrapped + 1.0 : wrapped;
}

float colorComponent(qreal value)
{
    return static_cast<float>(std::clamp(value, 0.0, 1.0));
}

// The hue that a point sits at, measured counter-clockwise from the right of
// the ring so that the ring reads the way a colour circle is normally drawn.
qreal hueForPoint(const QPointF &offset)
{
    const qreal angle = std::atan2(-offset.y(), offset.x());
    return wrapUnit(angle / tau);
}

QPointF pointForHue(qreal hue, qreal radius)
{
    const qreal angle = hue * tau;
    return QPointF(std::cos(angle) * radius, -std::sin(angle) * radius);
}

// Barycentric weights of a point against a triangle. The three sum to one and
// are all non-negative exactly when the point is inside.
std::array<qreal, 3> barycentric(
    const QPointF &point, const std::array<QPointF, 3> &corners)
{
    const QPointF a = corners[0];
    const QPointF b = corners[1];
    const QPointF c = corners[2];
    const qreal denominator =
        (b.y() - c.y()) * (a.x() - c.x()) + (c.x() - b.x()) * (a.y() - c.y());
    if (qFuzzyIsNull(denominator))
    {
        return {1.0, 0.0, 0.0};
    }
    const qreal first = ((b.y() - c.y()) * (point.x() - c.x())
                            + (c.x() - b.x()) * (point.y() - c.y()))
                        / denominator;
    const qreal second = ((c.y() - a.y()) * (point.x() - c.x())
                             + (a.x() - c.x()) * (point.y() - c.y()))
                         / denominator;
    return {first, second, 1.0 - first - second};
}

// Saturation and value expressed as weights on the hue, white and black
// corners. Black contributes nothing, so a fully saturated colour is pure hue
// weight and a tint is hue mixed with white.
std::array<qreal, 3> weightsForSaturationValue(qreal saturation, qreal value)
{
    const qreal hueWeight = value * saturation;
    const qreal whiteWeight = value * (1.0 - saturation);
    return {hueWeight, whiteWeight, 1.0 - value};
}

void saturationValueForWeights(
    const std::array<qreal, 3> &weights, qreal &saturation, qreal &value)
{
    const qreal hueWeight = std::max(0.0, weights[0]);
    const qreal whiteWeight = std::max(0.0, weights[1]);
    value = std::clamp(hueWeight + whiteWeight, 0.0, 1.0);
    saturation = value > 0.0 ? std::clamp(hueWeight / value, 0.0, 1.0) : 0.0;
}

}

ColorWheel::ColorWheel(QWidget *parent)
    : QWidget(parent)
{
    setObjectName(QStringLiteral("colorWheel"));
    setCursor(Qt::CrossCursor);
    setFocusPolicy(Qt::ClickFocus);
    QSizePolicy policy(QSizePolicy::Expanding, QSizePolicy::Preferred);
    policy.setHeightForWidth(true);
    setSizePolicy(policy);
}

QColor ColorWheel::color() const
{
    QColor result = QColor::fromHsvF(colorComponent(m_hue),
        colorComponent(m_saturation),
        colorComponent(m_value));
    result.setAlphaF(colorComponent(m_alpha));
    return result;
}

ColorWheel::Shape ColorWheel::shape() const
{
    return m_shape;
}

QSize ColorWheel::sizeHint() const
{
    return QSize(200, 200);
}

QSize ColorWheel::minimumSizeHint() const
{
    return QSize(96, 96);
}

int ColorWheel::heightForWidth(int width) const
{
    return width;
}

void ColorWheel::setColor(const QColor &color)
{
    if (!color.isValid())
    {
        return;
    }
    const QColor hsv = color.toHsv();
    // A grey has no hue of its own, so keep the ring where the artist left it
    // rather than snapping it back to red.
    const qreal hue = hsv.hueF() < 0.0 ? m_hue : hsv.hueF();
    const qreal saturation = hsv.saturationF();
    const qreal value = hsv.valueF();
    const qreal alpha = color.alphaF();
    if (qFuzzyCompare(hue + 1.0, m_hue + 1.0)
        && qFuzzyCompare(saturation + 1.0, m_saturation + 1.0)
        && qFuzzyCompare(value + 1.0, m_value + 1.0)
        && qFuzzyCompare(alpha + 1.0, m_alpha + 1.0))
    {
        return;
    }
    m_hue = hue;
    m_saturation = saturation;
    m_value = value;
    m_alpha = alpha;
    update();
}

void ColorWheel::setShape(Shape shape)
{
    if (m_shape == shape)
    {
        return;
    }
    m_shape = shape;
    m_fieldRadius = -1.0;
    update();
    Q_EMIT shapeChanged(shape);
}

ColorWheel::Geometry ColorWheel::geometry() const
{
    Geometry metrics;
    const qreal side = std::min(width(), height());
    metrics.outerRadius = side * 0.5 - margin;
    if (metrics.outerRadius <= 0.0)
    {
        return metrics;
    }
    metrics.center = QPointF(width() * 0.5, height() * 0.5);
    metrics.ringThickness = std::max(10.0, side * 0.11);
    metrics.innerRadius = metrics.outerRadius - metrics.ringThickness - ringGap;
    metrics.valid = metrics.innerRadius > 8.0;
    return metrics;
}

QRectF ColorWheel::squareRect(const Geometry &metrics) const
{
    // The largest square that fits inside the inner circle.
    const qreal side = metrics.innerRadius * std::numbers::sqrt2_v<qreal>;
    return QRectF(metrics.center.x() - side * 0.5,
        metrics.center.y() - side * 0.5,
        side,
        side);
}

std::array<QPointF, 3> ColorWheel::triangleCorners(
    const Geometry &metrics) const
{
    const qreal radius = metrics.innerRadius;
    const qreal vertical = radius * std::sqrt(3.0) * 0.5;
    return {metrics.center + QPointF(radius, 0.0),
        metrics.center + QPointF(-radius * 0.5, -vertical),
        metrics.center + QPointF(-radius * 0.5, vertical)};
}

QPointF ColorWheel::fieldMarker(const Geometry &metrics) const
{
    if (m_shape == Shape::Square)
    {
        const QRectF square = squareRect(metrics);
        return QPointF(square.left() + square.width() * m_saturation,
            square.top() + square.height() * (1.0 - m_value));
    }
    const std::array<QPointF, 3> corners = triangleCorners(metrics);
    const std::array<qreal, 3> weights =
        weightsForSaturationValue(m_saturation, m_value);
    return QPointF(corners[0].x() * weights[0] + corners[1].x() * weights[1]
                       + corners[2].x() * weights[2],
        corners[0].y() * weights[0] + corners[1].y() * weights[1]
            + corners[2].y() * weights[2]);
}

void ColorWheel::rebuildRing(const Geometry &metrics)
{
    const int side = static_cast<int>(std::ceil(metrics.outerRadius * 2.0));
    if (side <= 0)
    {
        m_ring = QImage();
        return;
    }
    const qreal ratio = devicePixelRatioF();
    QImage ring(
        QSize(static_cast<int>(side * ratio), static_cast<int>(side * ratio)),
        QImage::Format_ARGB32_Premultiplied);
    if (ring.isNull())
    {
        m_ring = QImage();
        return;
    }
    ring.setDevicePixelRatio(ratio);
    ring.fill(Qt::transparent);
    const qreal center = side * 0.5;
    const qreal outer = metrics.outerRadius;
    const qreal inner = metrics.outerRadius - metrics.ringThickness;
    for (int y = 0; y < ring.height(); ++y)
    {
        auto *line = reinterpret_cast<QRgb *>(ring.scanLine(y));
        const qreal deviceY = (y + 0.5) / ratio - center;
        for (int x = 0; x < ring.width(); ++x)
        {
            const qreal deviceX = (x + 0.5) / ratio - center;
            const qreal distance = std::hypot(deviceX, deviceY);
            if (distance > outer || distance < inner)
            {
                continue;
            }
            // Feathering the two rims by a pixel keeps the ring from looking
            // stair-stepped without paying for supersampling.
            const qreal edge = std::min(outer - distance, distance - inner);
            const qreal coverage = std::clamp(edge, 0.0, 1.0);
            const QColor hue = QColor::fromHsvF(
                colorComponent(hueForPoint(QPointF(deviceX, deviceY))),
                1.0,
                1.0);
            line[x] = qPremultiply(qRgba(hue.red(),
                hue.green(),
                hue.blue(),
                static_cast<int>(std::lround(coverage * 255.0))));
        }
    }
    m_ring = std::move(ring);
    m_ringRadius = metrics.outerRadius;
}

void ColorWheel::rebuildField(const Geometry &metrics)
{
    const qreal ratio = devicePixelRatioF();
    const int side =
        static_cast<int>(std::ceil(metrics.innerRadius * 2.0 * ratio));
    if (side <= 0)
    {
        m_field = QImage();
        return;
    }
    QImage field(QSize(side, side), QImage::Format_ARGB32_Premultiplied);
    if (field.isNull())
    {
        m_field = QImage();
        return;
    }
    field.setDevicePixelRatio(ratio);
    field.fill(Qt::transparent);
    const QPointF origin(metrics.center.x() - metrics.innerRadius,
        metrics.center.y() - metrics.innerRadius);

    if (m_shape == Shape::Square)
    {
        const QRectF square = squareRect(metrics);
        for (int y = 0; y < field.height(); ++y)
        {
            auto *line = reinterpret_cast<QRgb *>(field.scanLine(y));
            const qreal deviceY = origin.y() + (y + 0.5) / ratio;
            const qreal value =
                1.0 - (deviceY - square.top()) / square.height();
            for (int x = 0; x < field.width(); ++x)
            {
                const qreal deviceX = origin.x() + (x + 0.5) / ratio;
                const qreal saturation =
                    (deviceX - square.left()) / square.width();
                if (saturation < 0.0 || saturation > 1.0 || value < 0.0
                    || value > 1.0)
                {
                    continue;
                }
                const QColor sample = QColor::fromHsvF(colorComponent(m_hue),
                    colorComponent(saturation),
                    colorComponent(value));
                line[x] = qPremultiply(sample.rgba());
            }
        }
    }
    else
    {
        const std::array<QPointF, 3> corners = triangleCorners(metrics);
        const QColor hueColor =
            QColor::fromHsvF(colorComponent(m_hue), 1.0, 1.0);
        for (int y = 0; y < field.height(); ++y)
        {
            auto *line = reinterpret_cast<QRgb *>(field.scanLine(y));
            const qreal deviceY = origin.y() + (y + 0.5) / ratio;
            for (int x = 0; x < field.width(); ++x)
            {
                const qreal deviceX = origin.x() + (x + 0.5) / ratio;
                const std::array<qreal, 3> weights =
                    barycentric(QPointF(deviceX, deviceY), corners);
                if (weights[0] < 0.0 || weights[1] < 0.0 || weights[2] < 0.0)
                {
                    continue;
                }
                // Hue, white and black mixed linearly, which is what makes the
                // corners read as pure hue, white and black.
                const qreal red = hueColor.redF() * weights[0] + weights[1];
                const qreal green = hueColor.greenF() * weights[0] + weights[1];
                const qreal blue = hueColor.blueF() * weights[0] + weights[1];
                line[x] = qPremultiply(
                    qRgb(static_cast<int>(
                             std::lround(std::clamp(red, 0.0, 1.0) * 255.0)),
                        static_cast<int>(
                            std::lround(std::clamp(green, 0.0, 1.0) * 255.0)),
                        static_cast<int>(
                            std::lround(std::clamp(blue, 0.0, 1.0) * 255.0))));
            }
        }
    }
    m_field = std::move(field);
    m_fieldHue = m_hue;
    m_fieldRadius = metrics.innerRadius;
    m_fieldShape = m_shape;
}

void ColorWheel::paintEvent(QPaintEvent *)
{
    const Geometry metrics = geometry();
    if (!metrics.valid)
    {
        return;
    }
    if (m_ring.isNull() || !qFuzzyCompare(m_ringRadius, metrics.outerRadius))
    {
        rebuildRing(metrics);
    }
    // The square is hue-independent in layout but not in colour, so both
    // shapes rebuild whenever the hue moves.
    if (m_field.isNull() || m_fieldShape != m_shape
        || !qFuzzyCompare(m_fieldRadius, metrics.innerRadius)
        || !qFuzzyCompare(m_fieldHue + 1.0, m_hue + 1.0))
    {
        rebuildField(metrics);
    }

    QPainter painter(this);
    painter.setRenderHint(QPainter::Antialiasing, true);
    if (!m_ring.isNull())
    {
        painter.drawImage(QPointF(metrics.center.x() - metrics.outerRadius,
                              metrics.center.y() - metrics.outerRadius),
            m_ring);
    }
    if (!m_field.isNull())
    {
        painter.drawImage(QPointF(metrics.center.x() - metrics.innerRadius,
                              metrics.center.y() - metrics.innerRadius),
            m_field);
    }

    const auto drawMarker = [&painter](const QPointF &position)
    {
        painter.setBrush(Qt::NoBrush);
        painter.setPen(QPen(QColor(0, 0, 0, 170), 3.0));
        painter.drawEllipse(position, markerRadius, markerRadius);
        painter.setPen(QPen(Qt::white, 1.6));
        painter.drawEllipse(position, markerRadius, markerRadius);
    };
    drawMarker(metrics.center
               + pointForHue(
                   m_hue, metrics.outerRadius - metrics.ringThickness * 0.5));
    drawMarker(fieldMarker(metrics));
}

void ColorWheel::resizeEvent(QResizeEvent *event)
{
    m_ringRadius = -1.0;
    m_fieldRadius = -1.0;
    QWidget::resizeEvent(event);
}

void ColorWheel::applyRing(const QPointF &position, const Geometry &metrics)
{
    const qreal hue = hueForPoint(position - metrics.center);
    if (qFuzzyCompare(hue + 1.0, m_hue + 1.0))
    {
        return;
    }
    m_hue = hue;
    update();
    emitColor();
}

void ColorWheel::applyField(const QPointF &position, const Geometry &metrics)
{
    qreal saturation = m_saturation;
    qreal value = m_value;
    if (m_shape == Shape::Square)
    {
        const QRectF square = squareRect(metrics);
        saturation = std::clamp(
            (position.x() - square.left()) / square.width(), 0.0, 1.0);
        value = std::clamp(
            1.0 - (position.y() - square.top()) / square.height(), 0.0, 1.0);
    }
    else
    {
        std::array<qreal, 3> weights =
            barycentric(position, triangleCorners(metrics));
        // Dragging past an edge should slide along it rather than stop, so the
        // weights are clamped back onto the triangle instead of rejected.
        for (qreal &weight : weights)
        {
            weight = std::max(0.0, weight);
        }
        const qreal total = weights[0] + weights[1] + weights[2];
        if (total <= 0.0)
        {
            return;
        }
        for (qreal &weight : weights)
        {
            weight /= total;
        }
        saturationValueForWeights(weights, saturation, value);
    }
    if (qFuzzyCompare(saturation + 1.0, m_saturation + 1.0)
        && qFuzzyCompare(value + 1.0, m_value + 1.0))
    {
        return;
    }
    m_saturation = saturation;
    m_value = value;
    update();
    emitColor();
}

void ColorWheel::emitColor()
{
    Q_EMIT colorChanged(color());
}

void ColorWheel::mousePressEvent(QMouseEvent *event)
{
    const Geometry metrics = geometry();
    if (event->button() != Qt::LeftButton || !metrics.valid)
    {
        QWidget::mousePressEvent(event);
        return;
    }
    const QPointF position = event->position();
    const qreal distance = std::hypot(
        position.x() - metrics.center.x(), position.y() - metrics.center.y());
    if (distance > metrics.outerRadius - metrics.ringThickness
        && distance <= metrics.outerRadius + markerRadius)
    {
        m_drag = Drag::Ring;
        applyRing(position, metrics);
    }
    else if (distance <= metrics.innerRadius)
    {
        m_drag = Drag::Field;
        applyField(position, metrics);
    }
    event->accept();
}

void ColorWheel::mouseMoveEvent(QMouseEvent *event)
{
    const Geometry metrics = geometry();
    if (m_drag == Drag::None || !metrics.valid)
    {
        QWidget::mouseMoveEvent(event);
        return;
    }
    if (m_drag == Drag::Ring)
    {
        applyRing(event->position(), metrics);
    }
    else
    {
        applyField(event->position(), metrics);
    }
    event->accept();
}

void ColorWheel::mouseReleaseEvent(QMouseEvent *event)
{
    if (event->button() == Qt::LeftButton)
    {
        m_drag = Drag::None;
        event->accept();
        return;
    }
    QWidget::mouseReleaseEvent(event);
}

}

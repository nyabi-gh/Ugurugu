// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#pragma once

#include <QColor>
#include <QImage>
#include <QPointF>
#include <QWidget>

#include <array>

namespace ugurugu
{

// A hue ring around an inner field, the arrangement colour pickers in drawing
// apps settled on. The inner field comes in both shapes artists expect: a
// saturation/value square, and a triangle whose pure-hue corner tracks the
// ring.
class ColorWheel final : public QWidget
{
    Q_OBJECT

public:
    enum class Shape
    {
        Square,
        Triangle
    };

    explicit ColorWheel(QWidget *parent = nullptr);

    QColor color() const;
    Shape shape() const;
    QSize sizeHint() const override;
    QSize minimumSizeHint() const override;
    int heightForWidth(int width) const override;

public Q_SLOTS:
    void setColor(const QColor &color);
    void setShape(Shape shape);

Q_SIGNALS:
    void colorChanged(const QColor &color);
    void shapeChanged(Shape shape);

protected:
    void paintEvent(QPaintEvent *event) override;
    void resizeEvent(QResizeEvent *event) override;
    void mousePressEvent(QMouseEvent *event) override;
    void mouseMoveEvent(QMouseEvent *event) override;
    void mouseReleaseEvent(QMouseEvent *event) override;

private:
    enum class Drag
    {
        None,
        Ring,
        Field
    };

    struct Geometry
    {
        QPointF center;
        qreal outerRadius = 0.0;
        qreal innerRadius = 0.0;
        qreal ringThickness = 0.0;
        bool valid = false;
    };

    Geometry geometry() const;
    QRectF squareRect(const Geometry &metrics) const;
    // Hue, white and black corners, in that order.
    std::array<QPointF, 3> triangleCorners(const Geometry &metrics) const;
    QPointF fieldMarker(const Geometry &metrics) const;

    void rebuildRing(const Geometry &metrics);
    void rebuildField(const Geometry &metrics);
    void applyRing(const QPointF &position, const Geometry &metrics);
    void applyField(const QPointF &position, const Geometry &metrics);
    void emitColor();

    qreal m_hue = 0.0;
    qreal m_saturation = 1.0;
    qreal m_value = 1.0;
    qreal m_alpha = 1.0;
    Shape m_shape = Shape::Square;
    Drag m_drag = Drag::None;

    QImage m_ring;
    QImage m_field;
    qreal m_ringRadius = -1.0;
    qreal m_fieldHue = -1.0;
    qreal m_fieldRadius = -1.0;
    Shape m_fieldShape = Shape::Square;
};

}

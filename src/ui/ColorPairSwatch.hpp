// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#pragma once

#include <QColor>
#include <QWidget>

class QPainter;

namespace ugurugu
{

class ColorPairSwatch final : public QWidget
{
    Q_OBJECT

public:
    explicit ColorPairSwatch(QWidget *parent = nullptr);

    QColor currentColor() const;
    QColor previousColor() const;
    QSize sizeHint() const override;

public slots:
    void setCurrentColor(const QColor &color);

signals:
    void colorSelected(const QColor &color);

protected:
    void paintEvent(QPaintEvent *event) override;
    void mousePressEvent(QMouseEvent *event) override;

private:
    QRect currentRect() const;
    QRect previousRect() const;
    static void paintColor(QPainter &painter,
        const QRect &rect,
        const QColor &color,
        const QColor &border,
        int borderWidth);
    void refreshAccessibleText();

    QColor m_current = Qt::black;
    QColor m_previous = Qt::white;
};

}

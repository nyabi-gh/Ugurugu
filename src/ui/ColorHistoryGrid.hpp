// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#pragma once

#include <QColor>
#include <QTimer>
#include <QVector>
#include <QWidget>

class QToolButton;
class QGridLayout;
class QResizeEvent;

namespace ugurugu
{

class ColorHistoryGrid final : public QWidget
{
    Q_OBJECT

public:
    explicit ColorHistoryGrid(QWidget *parent = nullptr);
    ~ColorHistoryGrid() override;

    QSize minimumSizeHint() const override;
    void setActiveColor(const QColor &color);
    void recordColor(const QColor &color);
    void clear();

signals:
    void colorSelected(const QColor &color);

protected:
    void resizeEvent(QResizeEvent *event) override;

private:
    void relayoutForWidth(int width);
    void refreshButtons();
    void persist() const;

    QGridLayout *m_layout = nullptr;
    QVector<QColor> m_colors;
    QVector<QToolButton *> m_buttons;
    QColor m_activeColor;
    QTimer m_persistTimer;
    int m_columns = 0;
};

}

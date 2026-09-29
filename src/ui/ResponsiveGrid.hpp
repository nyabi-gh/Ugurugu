// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#pragma once

#include <QList>
#include <QWidget>

class QGridLayout;
class QResizeEvent;

namespace ugurugu
{

class ResponsiveGrid final : public QWidget
{
public:
    ResponsiveGrid(int minimumColumnWidth,
        int maximumColumnCount,
        int spacing,
        QWidget *parent = nullptr);

    void addWidget(QWidget *widget);
    QSize minimumSizeHint() const override;

protected:
    void resizeEvent(QResizeEvent *event) override;

private:
    void relayout();

    QGridLayout *m_layout = nullptr;
    QList<QWidget *> m_widgets;
    int m_minimumColumnWidth;
    int m_maximumColumnCount;
    int m_spacing;
    int m_columnCount = 0;
    int m_stretchRow = -1;
};

}

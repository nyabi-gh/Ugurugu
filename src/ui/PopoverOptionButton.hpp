// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#pragma once

#include <QAbstractButton>

class QPainter;

namespace ugurugu
{

class PopoverOptionButton : public QAbstractButton
{
public:
    PopoverOptionButton(
        QString title, QString description, QWidget *parent = nullptr);

    QSize sizeHint() const override;

protected:
    void paintEvent(QPaintEvent *event) final;
    virtual void paintPreview(
        QPainter &painter, const QRectF &bounds) const = 0;

private:
    QString m_title;
    QString m_description;
};

}

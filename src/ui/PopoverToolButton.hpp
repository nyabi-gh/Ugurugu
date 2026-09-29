// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#pragma once

#include "ui/Icons.hpp"

#include <QTimer>
#include <QToolButton>
#include <QVariantAnimation>

namespace ugurugu
{

class ToolPopover;

class PopoverToolButton final : public QToolButton
{
    Q_OBJECT

public:
    explicit PopoverToolButton(QWidget *parent = nullptr);

    void setPopover(ToolPopover *popover);
    void setHoverGlyph(IconGlyph glyph);

protected:
    void enterEvent(QEnterEvent *event) override;
    void mousePressEvent(QMouseEvent *event) override;
    void mouseReleaseEvent(QMouseEvent *event) override;
    void paintEvent(QPaintEvent *event) override;

private:
    void openPopover();
    void applyWobbleFrame(qreal phase);

    ToolPopover *m_popover = nullptr;
    QTimer m_longPressTimer;
    bool m_checkedAtPress = false;
    IconGlyph m_hoverGlyph = IconGlyph::Brush;
    bool m_hasHoverGlyph = false;
    QVariantAnimation m_hoverAnimation;
};

}

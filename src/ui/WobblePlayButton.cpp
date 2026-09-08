// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "ui/WobblePlayButton.hpp"

#include "ui/Icons.hpp"
#include "ui/Theme.hpp"

namespace ugurugu
{

WobblePlayButton::WobblePlayButton(QWidget *parent)
    : QToolButton(parent)
{
    setCheckable(true);
    setIconSize(QSize(20, 20));
    connect(this,
        &QToolButton::toggled,
        this,
        [this](bool)
        {
            refreshIcon();
        });
    refreshIcon();
}

void WobblePlayButton::refreshIcon()
{
    const IconGlyph glyph = isChecked() ? IconGlyph::Pause : IconGlyph::Play;
    const QColor color =
        isChecked() ? Theme::accentText() : Theme::textPrimary();
    const int size = iconSize().width();
    QIcon icon;
    icon.addPixmap(Icons::pixmap(glyph, size, color, 0.0, devicePixelRatio()),
        QIcon::Normal,
        isChecked() ? QIcon::On : QIcon::Off);
    icon.addPixmap(
        Icons::pixmap(
            glyph, size, Theme::textDisabled(), 0.0, devicePixelRatio()),
        QIcon::Disabled,
        isChecked() ? QIcon::On : QIcon::Off);
    setIcon(icon);
}

}

// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#pragma once

#include <QColor>
#include <QFont>

class QApplication;

namespace ugurugu
{

struct Theme final
{
    // Steps below the system interface font. Every smaller label in the app
    // names one of these instead of a pixel count, because a pixel count
    // ignores the Windows text size and macOS display settings that the
    // system font already carries.
    enum class TextRole
    {
        Title,
        Label,
        Caption,
    };

    static QColor chromeBackground();
    static QColor statusBackground();
    static QColor canvasBackground();
    static QColor panelBackground();
    static QColor controlBackground();
    static QColor hoverBackground();
    static QColor border();
    static QColor canvasBorder();
    static QColor textPrimary();
    static QColor textMuted();
    static QColor textDisabled();
    static QColor accent();
    static QColor accentPressed();
    static QColor accentText();
    static QColor defaultAccent();

    static QFont scaledFont(const QFont &base, TextRole role);
    static int fontPixelSize(TextRole role);
    static int minimumTextPixelSize();

    static void apply(QApplication &application);
    static void setAccent(QApplication &application, const QColor &color);
};

}

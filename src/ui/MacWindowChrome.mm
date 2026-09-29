// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "ui/MacWindowChrome.hpp"

#include "ui/Theme.hpp"

#include <QGuiApplication>
#include <QWidget>

#import <AppKit/AppKit.h>

namespace ugurugu
{

void applySeamlessTitleBar(QWidget *window)
{
    if (QGuiApplication::platformName() != QStringLiteral("cocoa"))
    {
        return;
    }
    // On Cocoa, Qt documents WId as the NSView pointer even though the public
    // cross-platform type is an integer-sized handle.
    // NOLINTNEXTLINE(bugprone-casting-through-void,performance-no-int-to-ptr)
    NSView *view = (__bridge NSView *)reinterpret_cast<void *>(window->winId());
    NSWindow *nativeWindow = view.window;
    if (!nativeWindow)
    {
        return;
    }
    nativeWindow.titlebarAppearsTransparent = YES;
    nativeWindow.appearance =
        [NSAppearance appearanceNamed:NSAppearanceNameDarkAqua];
    const QColor chrome = Theme::chromeBackground();
    nativeWindow.backgroundColor = [NSColor colorWithSRGBRed:chrome.redF()
                                                       green:chrome.greenF()
                                                        blue:chrome.blueF()
                                                       alpha:1.0];
}

}

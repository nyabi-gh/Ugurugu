// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#pragma once

#include <QKeySequence>
#include <QList>

class QAction;

namespace ugurugu
{

class ShortcutBinding final
{
public:
    static void initialize(QAction *action,
        const QKeySequence &defaultPrimary,
        const QList<QKeySequence> &aliases = {});
    static QKeySequence primary(const QAction *action);
    static QKeySequence defaultPrimary(const QAction *action);
    static bool hasShortcut(
        const QAction *action, const QKeySequence &shortcut);
    static void resolveAliasConflicts(const QList<QAction *> &actions);
    static void setPrimary(QAction *action, const QKeySequence &shortcut);
    static void restoreDefault(QAction *action);
};

}

// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "ui/ShortcutBinding.hpp"

#include <QAction>
#include <QSettings>
#include <QStringList>

#include <algorithm>

namespace ugurugu
{

namespace
{

constexpr auto defaultPrimaryProperty = "defaultShortcut";
constexpr auto primaryProperty = "primaryShortcut";
constexpr auto aliasProperty = "shortcutAliases";

QString settingsKey(const QAction *action)
{
    return QStringLiteral("shortcuts/%1").arg(action->objectName());
}

QKeySequence sequenceProperty(const QAction *action, const char *name)
{
    return action ? QKeySequence::fromString(action->property(name).toString(),
                        QKeySequence::PortableText)
                  : QKeySequence();
}

QList<QKeySequence> aliases(const QAction *action)
{
    QList<QKeySequence> result;
    if (!action)
    {
        return result;
    }
    const QStringList stored = action->property(aliasProperty).toStringList();
    result.reserve(stored.size());
    for (const QString &value : stored)
    {
        const QKeySequence shortcut =
            QKeySequence::fromString(value, QKeySequence::PortableText);
        if (!shortcut.isEmpty() && !result.contains(shortcut))
        {
            result.append(shortcut);
        }
    }
    return result;
}

void apply(QAction *action, const QKeySequence &primary)
{
    QList<QKeySequence> shortcuts;
    if (!primary.isEmpty())
    {
        shortcuts.append(primary);
    }
    for (const QKeySequence &alias : aliases(action))
    {
        if (!shortcuts.contains(alias))
        {
            shortcuts.append(alias);
        }
    }
    action->setProperty(
        primaryProperty, primary.toString(QKeySequence::PortableText));
    action->setShortcuts(shortcuts);
}

QKeySequence storedPrimary(
    const QAction *action, const QKeySequence &defaultPrimary)
{
    const QSettings settings;
    const QString key = settingsKey(action);
    if (!settings.contains(key))
    {
        return defaultPrimary;
    }
    const QString stored = settings.value(key).toString();
    const QKeySequence shortcut =
        QKeySequence::fromString(stored, QKeySequence::PortableText);
    return !stored.isEmpty() && shortcut.isEmpty() ? defaultPrimary : shortcut;
}

}

void ShortcutBinding::initialize(QAction *action,
    const QKeySequence &defaultPrimary,
    const QList<QKeySequence> &shortcutAliases)
{
    if (!action)
    {
        return;
    }
    action->setProperty(defaultPrimaryProperty,
        defaultPrimary.toString(QKeySequence::PortableText));
    QStringList storedAliases;
    storedAliases.reserve(shortcutAliases.size());
    for (const QKeySequence &alias : shortcutAliases)
    {
        if (!alias.isEmpty())
        {
            const QString stored = alias.toString(QKeySequence::PortableText);
            if (!storedAliases.contains(stored))
            {
                storedAliases.append(stored);
            }
        }
    }
    action->setProperty(aliasProperty, storedAliases);
    apply(action, storedPrimary(action, defaultPrimary));
}

QKeySequence ShortcutBinding::primary(const QAction *action)
{
    if (!action)
    {
        return {};
    }
    if (action->property(primaryProperty).isValid())
    {
        return sequenceProperty(action, primaryProperty);
    }
    return action->shortcut();
}

QKeySequence ShortcutBinding::defaultPrimary(const QAction *action)
{
    return sequenceProperty(action, defaultPrimaryProperty);
}

bool ShortcutBinding::hasShortcut(
    const QAction *action, const QKeySequence &shortcut)
{
    return action && !shortcut.isEmpty()
           && action->shortcuts().contains(shortcut);
}

void ShortcutBinding::resolveAliasConflicts(const QList<QAction *> &actions)
{
    QList<QKeySequence> claimedAliases;
    for (QAction *action : actions)
    {
        if (!action)
        {
            continue;
        }

        const QKeySequence actionPrimary = primary(action);
        QList<QKeySequence> shortcuts;
        if (!actionPrimary.isEmpty())
        {
            shortcuts.append(actionPrimary);
        }

        for (const QKeySequence &alias : aliases(action))
        {
            const bool usedByOtherPrimary = std::ranges::any_of(actions,
                [action, &alias](const QAction *other)
                {
                    return other && other != action && primary(other) == alias;
                });
            if (!usedByOtherPrimary && !claimedAliases.contains(alias)
                && !shortcuts.contains(alias))
            {
                shortcuts.append(alias);
                claimedAliases.append(alias);
            }
        }
        action->setShortcuts(shortcuts);
    }
}

void ShortcutBinding::setPrimary(QAction *action, const QKeySequence &shortcut)
{
    if (!action)
    {
        return;
    }
    apply(action, shortcut);
    QSettings settings;
    const QString key = settingsKey(action);
    if (shortcut == defaultPrimary(action))
    {
        settings.remove(key);
    }
    else
    {
        settings.setValue(key, shortcut.toString(QKeySequence::PortableText));
    }
}

void ShortcutBinding::restoreDefault(QAction *action)
{
    if (!action)
    {
        return;
    }
    apply(action, defaultPrimary(action));
    QSettings().remove(settingsKey(action));
}

}

// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#pragma once

#include <QLocale>
#include <QTranslator>

namespace ugurugu
{

// Installs Ugurugu's translation and Qt's own (standard buttons, the colour
// and file dialogs) for the interface locale, and removes them again. Both
// are compiled into the app: windeployqt merges Qt's catalogs under another
// name and macdeployqt ships none, so a deployed copy found nothing to load.
class InterfaceTranslators final
{
public:
    explicit InterfaceTranslators(const QLocale &locale);
    ~InterfaceTranslators();

    InterfaceTranslators(const InterfaceTranslators &) = delete;
    InterfaceTranslators &operator=(const InterfaceTranslators &) = delete;

    bool translatesQt() const;
    bool translatesApplication() const;

private:
    QTranslator m_qt;
    QTranslator m_application;
    bool m_qtInstalled = false;
    bool m_applicationInstalled = false;
};

}

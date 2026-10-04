// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "ui/InterfaceTranslators.hpp"

#include <QCoreApplication>

namespace ugurugu
{

InterfaceTranslators::InterfaceTranslators(const QLocale &locale)
{
    const QString directory = QStringLiteral(":/i18n");
    const QString separator = QStringLiteral("_");
    // Translators installed later are searched first, so Ugurugu's own
    // strings win over Qt's where both have a translation.
    if (m_qt.load(locale, QStringLiteral("qtbase"), separator, directory))
    {
        m_qtInstalled = QCoreApplication::installTranslator(&m_qt);
    }
    if (m_application.load(
            locale, QStringLiteral("ugurugu"), separator, directory))
    {
        m_applicationInstalled =
            QCoreApplication::installTranslator(&m_application);
    }
}

InterfaceTranslators::~InterfaceTranslators()
{
    if (m_applicationInstalled)
    {
        QCoreApplication::removeTranslator(&m_application);
    }
    if (m_qtInstalled)
    {
        QCoreApplication::removeTranslator(&m_qt);
    }
}

bool InterfaceTranslators::translatesQt() const
{
    return m_qtInstalled;
}

bool InterfaceTranslators::translatesApplication() const
{
    return m_applicationInstalled;
}

}

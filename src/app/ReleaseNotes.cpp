// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "app/ReleaseNotes.hpp"

#include <QHash>
#include <QRegularExpression>
#include <QStringList>

namespace ugurugu
{

QString localizedReleaseNotes(const QString &markdown, const QString &language)
{
    static const QRegularExpression markerPattern(
        QStringLiteral("^<!--\\s*lang:([A-Za-z-]+)\\s*-->$"));

    QStringList order;
    QHash<QString, QStringList> sections;
    QString current;
    for (const QString &line : markdown.split(QLatin1Char('\n')))
    {
        const QRegularExpressionMatch match =
            markerPattern.match(line.trimmed());
        if (match.hasMatch())
        {
            current = match.captured(1).toLower();
            if (!sections.contains(current))
            {
                order.append(current);
                sections.insert(current, {});
            }
            continue;
        }
        if (!current.isEmpty())
        {
            sections[current].append(line);
        }
    }

    if (order.isEmpty())
    {
        return markdown.trimmed();
    }

    const QString normalized = language.toLower()
                                   .section(QLatin1Char('_'), 0, 0)
                                   .section(QLatin1Char('-'), 0, 0);
    QString selected;
    if (sections.contains(normalized))
    {
        selected = normalized;
    }
    else if (sections.contains(QStringLiteral("en")))
    {
        selected = QStringLiteral("en");
    }
    else
    {
        selected = order.first();
    }
    return sections.value(selected).join(QLatin1Char('\n')).trimmed();
}

}

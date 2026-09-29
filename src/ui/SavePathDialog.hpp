// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#pragma once

#include <QList>
#include <QString>
#include <QStringList>

class QWidget;

namespace ugurugu
{

class SavePathDialog final
{
public:
    struct Format
    {
        QString filter;
        QString defaultExtension;
        QStringList acceptedExtensions;
    };

    static QString getSaveFileName(QWidget *parent,
        const QString &caption,
        const QString &startPath,
        const QList<Format> &formats);
    static QString confirmedFinalPath(
        QWidget *parent, const QString &selectedPath, const Format &format);
};

}

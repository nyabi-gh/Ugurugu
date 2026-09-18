// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "ui/SavePathDialog.hpp"

#include <QCoreApplication>
#include <QDir>
#include <QFileDialog>
#include <QFileInfo>
#include <QMessageBox>

namespace ugurugu
{

namespace
{

QStringList acceptedExtensions(const SavePathDialog::Format &format)
{
    return format.acceptedExtensions.isEmpty()
               ? QStringList{format.defaultExtension}
               : format.acceptedExtensions;
}

bool acceptsExtension(
    const SavePathDialog::Format &format, const QString &extension)
{
    for (const QString &accepted : acceptedExtensions(format))
    {
        if (accepted.compare(extension, Qt::CaseInsensitive) == 0)
        {
            return true;
        }
    }
    return false;
}

const SavePathDialog::Format &selectedFormat(
    const QList<SavePathDialog::Format> &formats,
    const QString &filePath,
    const QString &selectedFilter)
{
    const QString extension = QFileInfo(filePath).suffix();
    if (!extension.isEmpty())
    {
        for (const SavePathDialog::Format &format : formats)
        {
            if (acceptsExtension(format, extension))
            {
                return format;
            }
        }
    }
    for (const SavePathDialog::Format &format : formats)
    {
        if (format.filter == selectedFilter)
        {
            return format;
        }
    }
    return formats.first();
}

QString normalizedPath(
    const QString &filePath, const SavePathDialog::Format &format)
{
    if (acceptsExtension(format, QFileInfo(filePath).suffix()))
    {
        return filePath;
    }
    return filePath + QStringLiteral(".") + format.defaultExtension;
}

}

QString SavePathDialog::getSaveFileName(QWidget *parent,
    const QString &caption,
    const QString &startPath,
    const QList<Format> &formats)
{
    if (formats.isEmpty())
    {
        return {};
    }

    QStringList filters;
    for (const Format &format : formats)
    {
        filters.append(format.filter);
    }

    const QFileInfo startInfo(startPath);
    QFileDialog dialog(parent,
        caption,
        startInfo.absolutePath(),
        filters.join(QStringLiteral(";;")));
    dialog.setAcceptMode(QFileDialog::AcceptSave);
    dialog.setFileMode(QFileDialog::AnyFile);
    dialog.selectFile(startInfo.fileName());
    dialog.selectNameFilter(formats.first().filter);
    dialog.setDefaultSuffix(formats.first().defaultExtension);
    QObject::connect(&dialog,
        &QFileDialog::filterSelected,
        &dialog,
        [&dialog, formats](const QString &filter)
        {
            for (const Format &format : formats)
            {
                if (format.filter == filter)
                {
                    dialog.setDefaultSuffix(format.defaultExtension);
                    return;
                }
            }
        });
    if (dialog.exec() != QDialog::Accepted || dialog.selectedFiles().isEmpty())
    {
        return {};
    }

    const QString selectedPath = dialog.selectedFiles().first();
    return confirmedFinalPath(parent,
        selectedPath,
        selectedFormat(formats, selectedPath, dialog.selectedNameFilter()));
}

QString SavePathDialog::confirmedFinalPath(
    QWidget *parent, const QString &selectedPath, const Format &format)
{
    if (selectedPath.isEmpty() || format.defaultExtension.isEmpty())
    {
        return {};
    }
    const QString filePath = normalizedPath(selectedPath, format);
    if (filePath == selectedPath || !QFileInfo::exists(filePath))
    {
        return filePath;
    }

    QMessageBox confirmation(QMessageBox::Warning,
        QCoreApplication::translate("SavePathDialog", "Replace existing file?"),
        QCoreApplication::translate("SavePathDialog",
            "The file \"%1\" already exists.\nDo you want to replace it?")
            .arg(QDir::toNativeSeparators(filePath)),
        QMessageBox::Yes | QMessageBox::No,
        parent);
    confirmation.setObjectName(QStringLiteral("finalOverwriteConfirmation"));
    confirmation.setTextFormat(Qt::PlainText);
    confirmation.setDefaultButton(QMessageBox::No);
    confirmation.setEscapeButton(QMessageBox::No);
    return confirmation.exec() == QMessageBox::Yes ? filePath : QString();
}

}

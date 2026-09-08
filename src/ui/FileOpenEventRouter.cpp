// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "ui/FileOpenEventRouter.hpp"

#include <QApplication>
#include <QEvent>
#include <QFileInfo>
#include <QFileOpenEvent>
#include <QScopedValueRollback>
#include <QTimer>

#include <utility>

namespace ugurugu
{

FileOpenEventRouter::FileOpenEventRouter(
    std::function<void(const QString &)> openFile, QObject *parent)
    : QObject(parent)
    , m_openFile(std::move(openFile))
{
    m_dispatchTimer.setSingleShot(true);
    connect(&m_dispatchTimer,
        &QTimer::timeout,
        this,
        &FileOpenEventRouter::dispatchPendingFiles);
}

void FileOpenEventRouter::setReady(bool ready)
{
    m_ready = ready;
    if (m_ready)
    {
        dispatchPendingFiles();
    }
}

QString FileOpenEventRouter::takePendingFile()
{
    return m_pendingFiles.isEmpty() ? QString() : m_pendingFiles.takeFirst();
}

void FileOpenEventRouter::discardPendingFile(const QString &filePath)
{
    m_pendingFiles.removeAll(QFileInfo(filePath).absoluteFilePath());
}

void FileOpenEventRouter::discardPendingFiles()
{
    m_pendingFiles.clear();
}

void FileOpenEventRouter::dispatchPendingFiles()
{
    if (!m_ready || m_dispatching)
    {
        return;
    }
    if (QApplication::activeModalWidget())
    {
        schedulePendingDispatch(250);
        return;
    }
    QScopedValueRollback<bool> dispatching(m_dispatching, true);
    while (!m_pendingFiles.isEmpty() && !QApplication::activeModalWidget())
    {
        m_openFile(m_pendingFiles.takeFirst());
    }
}

void FileOpenEventRouter::schedulePendingDispatch(int delayMilliseconds)
{
    if (m_dispatchTimer.isActive() && delayMilliseconds > 0)
    {
        return;
    }
    m_dispatchTimer.start(delayMilliseconds);
}

bool FileOpenEventRouter::eventFilter(QObject *watched, QEvent *event)
{
    if (event->type() == QEvent::FileOpen)
    {
        const auto *fileEvent = static_cast<QFileOpenEvent *>(event);
        if (!fileEvent->file().isEmpty())
        {
            const QString filePath =
                QFileInfo(fileEvent->file()).absoluteFilePath();
            m_pendingFiles.append(filePath);
            dispatchPendingFiles();
            return true;
        }
    }
    if (m_ready && !m_pendingFiles.isEmpty()
        && (event->type() == QEvent::Hide || event->type() == QEvent::Close
            || event->type() == QEvent::Destroy))
    {
        schedulePendingDispatch(0);
    }
    return QObject::eventFilter(watched, event);
}

}

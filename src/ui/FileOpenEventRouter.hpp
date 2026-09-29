// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#pragma once

#include <QObject>
#include <QStringList>
#include <QTimer>

#include <functional>

namespace ugurugu
{

class FileOpenEventRouter final : public QObject
{
public:
    explicit FileOpenEventRouter(std::function<void(const QString &)> openFile,
        QObject *parent = nullptr);

    void setReady(bool ready);
    QString takePendingFile();
    void discardPendingFile(const QString &filePath);
    void discardPendingFiles();

protected:
    bool eventFilter(QObject *watched, QEvent *event) override;

private:
    void dispatchPendingFiles();
    void schedulePendingDispatch(int delayMilliseconds);

    std::function<void(const QString &)> m_openFile;
    QStringList m_pendingFiles;
    QTimer m_dispatchTimer;
    bool m_ready = false;
    bool m_dispatching = false;
};

}

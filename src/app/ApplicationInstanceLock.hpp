// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#pragma once

#include <QString>

#include <memory>

class QLockFile;

namespace ugurugu
{

class ApplicationInstanceLock final
{
public:
    enum class AcquireResult
    {
        Acquired,
        AlreadyRunning,
        Failed
    };

    explicit ApplicationInstanceLock(const QString &filePath = {});
    ~ApplicationInstanceLock();

    ApplicationInstanceLock(const ApplicationInstanceLock &) = delete;
    ApplicationInstanceLock &operator=(
        const ApplicationInstanceLock &) = delete;

    AcquireResult acquire(QString *error = nullptr);
    bool isAcquired() const;
    QString filePath() const;

private:
    QString m_filePath;
    std::unique_ptr<QLockFile> m_lock;
    bool m_acquired = false;
};

}

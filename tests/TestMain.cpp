// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "TestSuites.hpp"
#include "app/ApplicationInstanceLock.hpp"
#include "support/UiTestSuites.hpp"
#include "ui/Theme.hpp"

#include <QApplication>
#include <QEventLoop>
#include <QFile>
#include <QSettings>
#include <QTemporaryDir>
#include <QTimer>

#include <array>

namespace
{

struct Suite
{
    const char *name;
    int (*run)(int, char **);
};

constexpr std::array suites{
    Suite{"app", ugurugu::runAppPolicyTests},
    Suite{"document", ugurugu::runDocumentTests},
    Suite{"render", ugurugu::runRenderEngineTests},
    Suite{"gif", ugurugu::runGifWriterTests},
    Suite{"webp", ugurugu::runWebPWriterTests},
    Suite{"mask", ugurugu::runMaskRegressionTests},
    Suite{"release_notes", ugurugu::runReleaseNotesTests},
    Suite{"stabilizer", ugurugu::runStrokeStabilizerTests},
    Suite{"ui_shell", ugurugu::runUiShellTests},
    Suite{"ui_selection", ugurugu::runUiSelectionTests},
    Suite{"ui_viewport", ugurugu::runUiViewportTests},
    Suite{"ui_drawing_tools", ugurugu::runUiDrawingToolTests},
    Suite{"ui_session", ugurugu::runUiSessionTests},
};

}

int main(int argc, char **argv)
{
    QTemporaryDir settingsDirectory;
    if (!settingsDirectory.isValid())
    {
        qCritical("Could not create an isolated test settings directory.");
        return 1;
    }

    QSettings::setDefaultFormat(QSettings::IniFormat);
    QSettings::setPath(
        QSettings::IniFormat, QSettings::UserScope, settingsDirectory.path());
    QSettings::setPath(
        QSettings::IniFormat, QSettings::SystemScope, settingsDirectory.path());
    if (!qputenv("UGURUGU_RECOVERY_PATH",
            settingsDirectory.filePath(QStringLiteral("recovery.ugu"))
                .toUtf8()))
    {
        qCritical("Could not isolate the test recovery file.");
        return 1;
    }

    QApplication application(argc, argv);
    QApplication::setApplicationName(QStringLiteral("Ugurugu"));
    QApplication::setApplicationDisplayName(QStringLiteral("Ugurugu"));
    QApplication::setApplicationVersion(QStringLiteral(UGURUGU_VERSION));
    QApplication::setOrganizationName(QStringLiteral("Ugurugu"));
    QApplication::setOrganizationDomain(QStringLiteral("ugurugu.dev"));
    ugurugu::Theme::apply(application);

    const QString lockProbePath =
        qEnvironmentVariable("UGURUGU_INSTANCE_LOCK_PROBE_PATH");
    if (!lockProbePath.isEmpty())
    {
        ugurugu::ApplicationInstanceLock lock(lockProbePath);
        const auto result = lock.acquire();
        QFile ready(
            qEnvironmentVariable("UGURUGU_INSTANCE_LOCK_PROBE_READY_PATH"));
        if (!ready.open(QIODevice::WriteOnly))
        {
            return 3;
        }
        const QByteArray status =
            result == ugurugu::ApplicationInstanceLock::AcquireResult::Acquired
                ? QByteArrayLiteral("acquired")
                : QByteArrayLiteral("failed");
        if (ready.write(status) != status.size())
        {
            return 3;
        }
        ready.close();
        if (result != ugurugu::ApplicationInstanceLock::AcquireResult::Acquired)
        {
            return 4;
        }
        QEventLoop loop;
        QTimer::singleShot(30000, &loop, &QEventLoop::quit);
        return loop.exec();
    }

    const QByteArray requested = qgetenv("UGURUGU_TEST_SUITE");
    int result = 0;
    bool matched = requested.isEmpty();
    for (const Suite &suite : suites)
    {
        if (!requested.isEmpty() && requested != suite.name)
        {
            continue;
        }
        matched = true;
        result |= suite.run(argc, argv);
    }
    if (!matched)
    {
        qCritical("Unknown UGURUGU_TEST_SUITE: %s", requested.constData());
        return 2;
    }
    return result;
}

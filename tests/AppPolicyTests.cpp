// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "app/ApplicationInstanceLock.hpp"
#include "app/UpdateCheckPolicy.hpp"
#include "ui/FileOpenEventRouter.hpp"

#include <QApplication>
#include <QDateTime>
#include <QDialog>
#include <QFile>
#include <QFileOpenEvent>
#include <QProcess>
#include <QProcessEnvironment>
#include <QScopeGuard>
#include <QTemporaryDir>
#include <QtTest>

#include <algorithm>

namespace ugurugu
{

class AppPolicyTests final : public QObject
{
    Q_OBJECT

private slots:
    void enforcesSingleApplicationInstance()
    {
        QTemporaryDir directory;
        QVERIFY(directory.isValid());
        const QString lockPath =
            directory.filePath(QStringLiteral("instance.lock"));
        {
            ApplicationInstanceLock first(lockPath);
            ApplicationInstanceLock second(lockPath);
            QString error;
            QCOMPARE(first.acquire(&error),
                ApplicationInstanceLock::AcquireResult::Acquired);
            QVERIFY2(error.isEmpty(), qPrintable(error));
#if !defined(Q_OS_WIN)
            QFile lockFile(lockPath);
            QVERIFY(lockFile.open(QIODevice::ReadWrite));
            QVERIFY(lockFile.setFileTime(
                QDateTime::currentDateTimeUtc().addSecs(-60),
                QFileDevice::FileModificationTime));
            lockFile.close();
#endif
            QCOMPARE(second.acquire(),
                ApplicationInstanceLock::AcquireResult::AlreadyRunning);
        }
        ApplicationInstanceLock next(lockPath);
        QCOMPARE(
            next.acquire(), ApplicationInstanceLock::AcquireResult::Acquired);
    }

    void enforcesSingleApplicationInstanceAcrossProcesses()
    {
        QTemporaryDir directory;
        QVERIFY(directory.isValid());
        const QString lockPath =
            directory.filePath(QStringLiteral("instance.lock"));
        const QString readyPath =
            directory.filePath(QStringLiteral("probe.ready"));

        QProcess child;
        QProcessEnvironment environment =
            QProcessEnvironment::systemEnvironment();
        environment.insert(
            QStringLiteral("UGURUGU_INSTANCE_LOCK_PROBE_PATH"), lockPath);
        environment.insert(
            QStringLiteral("UGURUGU_INSTANCE_LOCK_PROBE_READY_PATH"),
            readyPath);
        child.setProcessEnvironment(environment);
        child.setProcessChannelMode(QProcess::MergedChannels);
        [[maybe_unused]] const auto stopChild = qScopeGuard(
            [&child]()
            {
                if (child.state() != QProcess::NotRunning)
                {
                    child.kill();
                    child.waitForFinished(3000);
                }
            });
        child.start(QCoreApplication::applicationFilePath());
        QVERIFY2(child.waitForStarted(3000), qPrintable(child.errorString()));
        QTRY_VERIFY_WITH_TIMEOUT(QFileInfo::exists(readyPath), 3000);
        QFile readyFile(readyPath);
        QVERIFY(readyFile.open(QIODevice::ReadOnly));
        QCOMPARE(readyFile.readAll(), QByteArrayLiteral("acquired"));
        readyFile.close();

        ApplicationInstanceLock competing(lockPath);
        QCOMPARE(competing.acquire(),
            ApplicationInstanceLock::AcquireResult::AlreadyRunning);

        child.kill();
        QVERIFY(child.waitForFinished(3000));
        ApplicationInstanceLock recovered(lockPath);
        QCOMPARE(recovered.acquire(),
            ApplicationInstanceLock::AcquireResult::Acquired);
    }

    void defersFileOpenEventsUntilStartupIsResolved()
    {
        QTemporaryDir directory;
        QVERIFY(directory.isValid());
        const QString pendingPath =
            directory.filePath(QStringLiteral("pending.ugu"));
        const QString deferredPath =
            directory.filePath(QStringLiteral("deferred.ugu"));
        const QString readyPath =
            directory.filePath(QStringLiteral("ready.ugu"));
        QStringList opened;
        FileOpenEventRouter router(
            [&opened](const QString &filePath)
            {
                opened.append(filePath);
            });
        QObject target;
        target.installEventFilter(&router);

        QFileOpenEvent pending(pendingPath);
        QApplication::sendEvent(&target, &pending);
        QVERIFY(opened.isEmpty());
        QCOMPARE(router.takePendingFile(), pendingPath);

        QFileOpenEvent deferred(deferredPath);
        QApplication::sendEvent(&target, &deferred);
        router.setReady(true);
        QCOMPARE(opened, QStringList{deferredPath});
        QFileOpenEvent ready(readyPath);
        QApplication::sendEvent(&target, &ready);
        QCOMPARE(opened, (QStringList{deferredPath, readyPath}));
    }

    void removesConsumedStartupFileFromDeferredEvents()
    {
        QTemporaryDir directory;
        QVERIFY(directory.isValid());
        const QString startupPath =
            directory.filePath(QStringLiteral("startup.ugu"));
        const QString otherPath =
            directory.filePath(QStringLiteral("other.ugu"));
        QStringList opened;
        FileOpenEventRouter router(
            [&opened](const QString &filePath)
            {
                opened.append(filePath);
            });
        QObject target;
        target.installEventFilter(&router);

        QFileOpenEvent duplicateOne(startupPath);
        QFileOpenEvent other(otherPath);
        QFileOpenEvent duplicateTwo(startupPath);
        QApplication::sendEvent(&target, &duplicateOne);
        QApplication::sendEvent(&target, &other);
        QApplication::sendEvent(&target, &duplicateTwo);

        router.discardPendingFile(startupPath);
        router.setReady(true);

        QCOMPARE(opened, QStringList{otherPath});
    }

    void serializesNestedFileOpenEvents()
    {
        QTemporaryDir directory;
        QVERIFY(directory.isValid());
        const QString firstPath =
            directory.filePath(QStringLiteral("first.ugu"));
        const QString nestedPath =
            directory.filePath(QStringLiteral("nested.ugu"));
        QObject target;
        QStringList opened;
        int callbackDepth = 0;
        int maximumCallbackDepth = 0;
        FileOpenEventRouter router(
            [&](const QString &filePath)
            {
                ++callbackDepth;
                maximumCallbackDepth =
                    std::max(maximumCallbackDepth, callbackDepth);
                opened.append(filePath);
                if (filePath == firstPath)
                {
                    QFileOpenEvent nested(nestedPath);
                    QApplication::sendEvent(&target, &nested);
                }
                --callbackDepth;
            });
        target.installEventFilter(&router);
        router.setReady(true);

        QFileOpenEvent first(firstPath);
        QApplication::sendEvent(&target, &first);

        QCOMPARE(opened, (QStringList{firstPath, nestedPath}));
        QCOMPARE(maximumCallbackDepth, 1);
    }

    void defersFileOpenEventsWhileAModalDialogIsActive()
    {
        QTemporaryDir directory;
        QVERIFY(directory.isValid());
        const QString filePath =
            directory.filePath(QStringLiteral("deferred-modal.ugu"));
        QStringList opened;
        FileOpenEventRouter router(
            [&opened](const QString &openedPath)
            {
                opened.append(openedPath);
            });
        qApp->installEventFilter(&router);
        router.setReady(true);
        QDialog dialog;
        dialog.setModal(true);
        dialog.show();
        QTRY_COMPARE(QApplication::activeModalWidget(), &dialog);

        QFileOpenEvent event(filePath);
        QApplication::sendEvent(qApp, &event);
        QVERIFY(opened.isEmpty());

        dialog.hide();
        QTRY_COMPARE(opened, QStringList{filePath});
        qApp->removeEventFilter(&router);
    }

    void limitsAutomaticUpdateChecksToOnePerInterval()
    {
        const QDateTime now(QDate(2026, 8, 1), QTime(12, 0), QTimeZone::UTC);
        QVERIFY(UpdateCheckPolicy::isAutomaticCheckDue({}, now));
        QVERIFY(!UpdateCheckPolicy::isAutomaticCheckDue(
            now.addSecs(-UpdateCheckPolicy::automaticCheckIntervalSeconds + 1),
            now));
        QVERIFY(UpdateCheckPolicy::isAutomaticCheckDue(
            now.addSecs(-UpdateCheckPolicy::automaticCheckIntervalSeconds),
            now));
    }

    void retriesAfterSystemClockMovesBackward()
    {
        const QDateTime now(QDate(2026, 8, 1), QTime(12, 0), QTimeZone::UTC);
        QVERIFY(UpdateCheckPolicy::isAutomaticCheckDue(now.addDays(1), now));
        QVERIFY(!UpdateCheckPolicy::isAutomaticCheckDue(now, {}));
    }
};

int runAppPolicyTests(int argc, char **argv)
{
    AppPolicyTests tests;
    return QTest::qExec(&tests, argc, argv);
}

}

#include "AppPolicyTests.moc"

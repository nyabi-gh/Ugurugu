// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "app/UpdateCheckPolicy.hpp"
#include "app/UpdateController.hpp"

#include <QApplication>
#include <QDateTime>
#include <QFutureWatcher>
#include <QMessageBox>
#include <QPointer>
#include <QProgressDialog>
#include <QPromise>
#include <QPushButton>
#include <QSettings>
#include <QThreadPool>
#include <QTimer>
#include <QtConcurrentRun>

#include <spdlog/spdlog.h>

#include <Velopack.hpp>
#include <algorithm>
#include <exception>
#include <memory>
#include <optional>
#include <string>
#include <type_traits>

namespace ugurugu
{

namespace
{

constexpr auto repositoryUrl = "https://github.com/nyabi-gh/Ugurugu";
constexpr auto lastAutomaticCheckKey = "updates/lastAutomaticCheck";

// Checks and downloads wait on the network and cannot be canceled. Shutdown
// joins the global pool, so they run on one of their own, which is never
// destroyed because its destructor would wait for them as well. Their
// results reach the UI through watchers that go with the window.
QThreadPool *networkPool()
{
    static QThreadPool *const pool = new QThreadPool;
    return pool;
}

std::unique_ptr<Velopack::UpdateManager> createUpdateManager()
{
    auto source = std::make_unique<Velopack::GithubSource>(repositoryUrl);
    return std::make_unique<Velopack::UpdateManager>(std::move(source));
}

struct CheckResult
{
    std::optional<Velopack::UpdateInfo> update;
    QString error;
};

struct DownloadResult
{
    std::optional<Velopack::UpdateInfo> update;
    QString error;
};

}

class UpdateController::Impl
{
public:
    explicit Impl(UpdateController *owner, QWidget *window)
        : owner(owner)
        , window(window)
    {
    }

    void startAutomaticCheckIfDue()
    {
        QSettings settings;
        const QDateTime now = QDateTime::currentDateTimeUtc();
        const QDateTime lastCheck =
            settings.value(QString::fromLatin1(lastAutomaticCheckKey))
                .toDateTime();
        if (!UpdateCheckPolicy::isAutomaticCheckDue(lastCheck, now))
        {
            return;
        }
        startCheck(false);
    }

    // Only a completed check consumes the interval. Claiming it before the
    // request would make an offline launch cost a full interval, which is the
    // case where the user is most likely to be missing a fix. A failure
    // therefore retries at the next launch, not sooner: nothing reschedules
    // this within a session.
    void recordAutomaticCheck()
    {
        QSettings settings;
        settings.setValue(QString::fromLatin1(lastAutomaticCheckKey),
            QDateTime::currentDateTimeUtc());
    }

    void startCheck(bool interactive)
    {
        if (busy)
        {
            return;
        }
        busy = true;

        auto *watcher = new QFutureWatcher<CheckResult>(owner);
        QObject::connect(watcher,
            &QFutureWatcher<CheckResult>::finished,
            owner,
            [this, watcher, interactive]()
            {
                CheckResult result;
                try
                {
                    result = watcher->result();
                }
                catch (const std::exception &error)
                {
                    result.error = QString::fromUtf8(error.what());
                }
                catch (...)
                {
                    result.error = UpdateController::tr(
                        "The update check failed unexpectedly.");
                }
                watcher->deleteLater();
                busy = false;

                if (!result.error.isEmpty())
                {
                    spdlog::warn("Update check failed: {}",
                        result.error.toUtf8().constData());
                    if (interactive)
                    {
                        QMessageBox::warning(window,
                            UpdateController::tr("Update failed"),
                            UpdateController::tr(
                                "Could not check for updates.\n\n%1")
                                .arg(result.error));
                    }
                    return;
                }

                if (!interactive)
                {
                    recordAutomaticCheck();
                }

                if (!result.update)
                {
                    if (interactive)
                    {
                        QMessageBox::information(window,
                            UpdateController::tr("You're up to date"),
                            UpdateController::tr(
                                "Ugurugu %1 is the latest version.")
                                .arg(QApplication::applicationVersion()));
                    }
                    return;
                }

                offerUpdate(*result.update);
            });

        watcher->setFuture(QtConcurrent::run(networkPool(),
            []()
            {
                try
                {
                    auto manager = createUpdateManager();
                    return CheckResult{manager->CheckForUpdates(), {}};
                }
                catch (const std::exception &error)
                {
                    return CheckResult{{}, QString::fromUtf8(error.what())};
                }
            }));
    }

    void offerUpdate(const Velopack::UpdateInfo &update)
    {
        QMessageBox box(QMessageBox::Information,
            UpdateController::tr("Update available"),
            UpdateController::tr("Ugurugu %1 is available.")
                .arg(QString::fromStdString(update.TargetFullRelease.Version)),
            QMessageBox::Cancel,
            window);
        box.setObjectName(QStringLiteral("updateAvailableDialog"));
        box.setInformativeText(
            UpdateController::tr("Download and install it now?"));
        QPushButton *install = box.addButton(
            UpdateController::tr("Update"), QMessageBox::AcceptRole);
        box.setDefaultButton(install);
        box.exec();
        if (box.clickedButton() == install)
        {
            startDownload(update);
        }
    }

    void startDownload(const Velopack::UpdateInfo &update)
    {
        busy = true;
        auto *progress =
            new QProgressDialog(UpdateController::tr("Downloading update…"),
                QString(),
                0,
                100,
                window);
        progress->setWindowTitle(UpdateController::tr("Ugurugu update"));
        progress->setCancelButton(nullptr);
        progress->setMinimumDuration(0);
        progress->setValue(0);
        progress->show();

        auto *watcher = new QFutureWatcher<DownloadResult>(owner);
        QObject::connect(watcher,
            &QFutureWatcher<DownloadResult>::progressValueChanged,
            progress,
            &QProgressDialog::setValue);
        QObject::connect(watcher,
            &QFutureWatcher<DownloadResult>::finished,
            owner,
            [this, watcher, progress]()
            {
                DownloadResult result;
                try
                {
                    result = watcher->result();
                }
                catch (const std::exception &error)
                {
                    result.error = QString::fromUtf8(error.what());
                }
                catch (...)
                {
                    result.error = UpdateController::tr(
                        "The update download failed unexpectedly.");
                }
                watcher->deleteLater();
                progress->close();
                progress->deleteLater();
                busy = false;

                if (!result.error.isEmpty())
                {
                    spdlog::error("Update download failed: {}",
                        result.error.toUtf8().constData());
                    QMessageBox::warning(window,
                        UpdateController::tr("Update failed"),
                        UpdateController::tr(
                            "Could not download the update.\n\n%1")
                            .arg(result.error));
                    return;
                }
                if (result.update)
                {
                    applyUpdate(*result.update);
                }
            });

        const auto sharedUpdate =
            std::make_shared<const Velopack::UpdateInfo>(update);
        watcher->setFuture(QtConcurrent::run(networkPool(),
            [sharedUpdate](QPromise<DownloadResult> &promise)
            {
                promise.setProgressRange(0, 100);
                try
                {
                    auto manager = createUpdateManager();
                    manager->DownloadUpdates(
                        *sharedUpdate,
                        [](void *data, size_t value)
                        {
                            auto *downloadPromise =
                                static_cast<QPromise<DownloadResult> *>(data);
                            downloadPromise->setProgressValue(
                                std::clamp(static_cast<int>(value), 0, 100));
                        },
                        &promise);
                    promise.addResult(DownloadResult{*sharedUpdate, {}});
                }
                catch (const std::exception &error)
                {
                    promise.addResult(
                        DownloadResult{{}, QString::fromUtf8(error.what())});
                }
            }));
    }

    void applyUpdate(const Velopack::UpdateInfo &update)
    {
        std::unique_ptr<Velopack::UpdateManager> manager;
        try
        {
            manager = createUpdateManager();
        }
        catch (const std::exception &error)
        {
            QMessageBox::warning(window,
                UpdateController::tr("Update failed"),
                UpdateController::tr(
                    "The update was downloaded but could not be installed."
                    "\n\n%1")
                    .arg(QString::fromUtf8(error.what())));
            return;
        }

        const bool quitOnLastWindowClosed = qApp->quitOnLastWindowClosed();
        qApp->setQuitOnLastWindowClosed(false);
        if (window && !window->close())
        {
            qApp->setQuitOnLastWindowClosed(quitOnLastWindowClosed);
            return;
        }

        try
        {
            manager->WaitExitThenApplyUpdates(update);
            qApp->quit();
        }
        catch (const std::exception &error)
        {
            qApp->setQuitOnLastWindowClosed(quitOnLastWindowClosed);
            if (window)
            {
                window->show();
            }
            QMessageBox::warning(window,
                UpdateController::tr("Update failed"),
                UpdateController::tr(
                    "The update was downloaded but could not be installed."
                    "\n\n%1")
                    .arg(QString::fromUtf8(error.what())));
        }
    }

    UpdateController *owner = nullptr;
    QPointer<QWidget> window;
    bool busy = false;
};

void UpdateController::initialize()
{
    Velopack::VelopackApp::Build().Run();
}

UpdateController::UpdateController(QWidget *window, QObject *parent)
    : QObject(parent)
    , m_impl(std::make_unique<Impl>(this, window))
{
    QTimer::singleShot(2000,
        this,
        [this]()
        {
            m_impl->startAutomaticCheckIfDue();
        });
}

UpdateController::~UpdateController() = default;

void UpdateController::checkForUpdates()
{
    m_impl->startCheck(true);
}

}

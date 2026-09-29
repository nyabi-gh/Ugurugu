// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "app/Logging.hpp"

#include <QDir>
#include <QStandardPaths>

#include <spdlog/sinks/rotating_file_sink.h>
#include <spdlog/sinks/stdout_color_sinks.h>
#include <spdlog/spdlog.h>

#include <cstdlib>
#include <memory>
#include <string>
#include <vector>

namespace ugurugu
{

namespace
{

QString currentLogFilePath;

void qtMessageHandler(
    QtMsgType type, const QMessageLogContext &, const QString &message)
{
    const std::string text = message.toUtf8().toStdString();
    switch (type)
    {
    case QtDebugMsg:
        spdlog::debug("{}", text);
        break;
    case QtInfoMsg:
        spdlog::info("{}", text);
        break;
    case QtWarningMsg:
        spdlog::warn("{}", text);
        break;
    case QtCriticalMsg:
        spdlog::error("{}", text);
        break;
    case QtFatalMsg:
        spdlog::critical("{}", text);
        spdlog::shutdown();
        std::abort();
    }
}

}

void Logging::initialize()
{
    const QString directory =
        QStandardPaths::writableLocation(QStandardPaths::AppLocalDataLocation);
    QDir().mkpath(directory);
    currentLogFilePath =
        QDir(directory).filePath(QStringLiteral("Ugurugu.log"));

    std::vector<spdlog::sink_ptr> sinks;
    sinks.push_back(std::make_shared<spdlog::sinks::stdout_color_sink_mt>());
    QString fileLoggingError;

    try
    {
#if defined(SPDLOG_WCHAR_FILENAMES)
        const spdlog::filename_t path =
            QDir::toNativeSeparators(currentLogFilePath).toStdWString();
#else
        const spdlog::filename_t path =
            currentLogFilePath.toUtf8().toStdString();
#endif
        sinks.push_back(std::make_shared<spdlog::sinks::rotating_file_sink_mt>(
            path, 2 * 1024 * 1024, 3));
    }
    catch (const spdlog::spdlog_ex &error)
    {
        fileLoggingError = QString::fromUtf8(error.what());
    }

    auto logger =
        std::make_shared<spdlog::logger>("ugurugu", sinks.begin(), sinks.end());
    logger->set_pattern("[%Y-%m-%d %H:%M:%S.%e] [%^%l%$] %v");
#if defined(QT_DEBUG)
    logger->set_level(spdlog::level::debug);
#else
    logger->set_level(spdlog::level::info);
#endif
    logger->flush_on(spdlog::level::warn);
    spdlog::set_default_logger(logger);
    qInstallMessageHandler(qtMessageHandler);
    spdlog::info(
        "Logging initialized at {}", currentLogFilePath.toUtf8().constData());
    if (!fileLoggingError.isEmpty())
    {
        spdlog::warn("File logging is unavailable: {}",
            fileLoggingError.toUtf8().constData());
    }
}

void Logging::shutdown()
{
    qInstallMessageHandler(nullptr);
    spdlog::shutdown();
}

QString Logging::logFilePath()
{
    return currentLogFilePath;
}

}

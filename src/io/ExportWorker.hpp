// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#pragma once

#include "document/Document.hpp"

#include <QMutex>
#include <QObject>
#include <QSize>
#include <QString>
#include <QThread>
#include <QWaitCondition>

#include <optional>

namespace ugurugu
{

class ExportWorker final : public QObject
{
    Q_OBJECT

public:
    enum class Kind
    {
        Image,
        Gif,
        WebP
    };
    Q_ENUM(Kind)

    explicit ExportWorker(QObject *parent = nullptr);
    ~ExportWorker() override;

    // Output size and transparency for one GIF export. `outputSize` is the
    // encoded frame size, which is what the memory budget is checked against,
    // so a scaled-down export can succeed where the native size cannot.
    struct AnimationOptions
    {
        QSize outputSize;
        bool preserveTransparency = true;
    };

    bool startImage(
        Document document, int frame, const QString &filePath, bool jpeg);
    bool startGif(Document document,
        const QString &filePath,
        const AnimationOptions &options);
    bool startWebP(Document document,
        const QString &filePath,
        const AnimationOptions &options);
    void cancel();
    bool isBusy() const;
    bool waitForIdle(int timeoutMilliseconds = -1);
    // Nothing in an export throws deliberately, so the only way to exercise
    // the worker's exception boundary is to make the next write raise.
    void throwFromNextExportForTesting();

signals:
    void progress(Kind kind, int value, int maximum);
    void finished(Kind kind,
        bool success,
        bool canceled,
        const QString &filePath,
        const QString &error);

private:
    struct Request
    {
        Kind kind = Kind::Image;
        Document document;
        int frame = 0;
        QString filePath;
        bool jpeg = false;
        AnimationOptions animation;
    };

    bool start(Request request);
    void process();
    bool canceled() const;
    void postProgress(Kind kind, int value, int maximum);
    void complete(const Request &request,
        bool success,
        bool wasCanceled,
        const QString &error);
    bool writeImage(const Request &request, QString *error);
    bool writeAnimation(const Request &request, QString *error);
    void raiseRequestedTestingFailure();

    QThread m_thread;
    // Lives on m_thread but is destroyed here, on the owning thread. The
    // destructor quits and joins that thread first, and the context owns no
    // timers, no children and no queued events by then, so nothing on the
    // worker side is left to touch it.
    QObject m_workerContext;
    mutable QMutex m_mutex;
    QWaitCondition m_idleCondition;
    std::optional<Request> m_request;
    bool m_busy = false;
    bool m_cancelRequested = false;
    bool m_throwFromNextExportForTesting = false;
};

}

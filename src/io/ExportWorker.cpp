// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "io/ExportWorker.hpp"

#include "io/AnimationExportPolicy.hpp"
#include "io/GifWriter.hpp"
#include "io/WebPWriter.hpp"
#include "render/RenderEngine.hpp"

#include <QDeadlineTimer>
#include <QImageWriter>
#include <QMutexLocker>
#include <QPainter>
#include <QSaveFile>

#include <stdexcept>
#include <utility>

namespace ugurugu
{

ExportWorker::ExportWorker(QObject *parent)
    : QObject(parent)
{
    m_thread.setObjectName(QStringLiteral("ExportWorker"));
    m_workerContext.moveToThread(&m_thread);
    m_thread.start();
}

ExportWorker::~ExportWorker()
{
    // Waits without a deadline on purpose. Cancellation is polled between
    // frames, so an export in progress stops within one frame, and giving up
    // on it would leave the encoder writing into a file this object owns.
    cancel();
    waitForIdle();
    m_thread.quit();
    m_thread.wait();
}

bool ExportWorker::startImage(
    Document document, int frame, const QString &filePath, bool jpeg)
{
    return start({Kind::Image, std::move(document), frame, filePath, jpeg, {}});
}

bool ExportWorker::startGif(
    Document document, const QString &filePath, const AnimationOptions &options)
{
    return start({Kind::Gif, std::move(document), 0, filePath, false, options});
}

bool ExportWorker::startWebP(
    Document document, const QString &filePath, const AnimationOptions &options)
{
    return start(
        {Kind::WebP, std::move(document), 0, filePath, false, options});
}

bool ExportWorker::start(Request request)
{
    {
        QMutexLocker locker(&m_mutex);
        if (m_busy || m_request)
        {
            return false;
        }
        m_request = std::move(request);
        m_busy = true;
        m_cancelRequested = false;
    }
    QMetaObject::invokeMethod(
        &m_workerContext,
        [this]()
        {
            process();
        },
        Qt::QueuedConnection);
    return true;
}

void ExportWorker::cancel()
{
    QMutexLocker locker(&m_mutex);
    m_cancelRequested = true;
}

void ExportWorker::throwFromNextExportForTesting()
{
    QMutexLocker locker(&m_mutex);
    m_throwFromNextExportForTesting = true;
}

bool ExportWorker::isBusy() const
{
    QMutexLocker locker(&m_mutex);
    return m_busy;
}

bool ExportWorker::waitForIdle(int timeoutMilliseconds)
{
    QDeadlineTimer deadline = timeoutMilliseconds < 0
                                  ? QDeadlineTimer(QDeadlineTimer::Forever)
                                  : QDeadlineTimer(timeoutMilliseconds);
    QMutexLocker locker(&m_mutex);
    while (m_busy || m_request)
    {
        if (!m_idleCondition.wait(&m_mutex, deadline))
        {
            return false;
        }
    }
    return true;
}

void ExportWorker::process()
{
    Request request;
    {
        QMutexLocker locker(&m_mutex);
        if (!m_request)
        {
            m_busy = false;
            m_idleCondition.wakeAll();
            return;
        }
        request = std::move(*m_request);
        m_request.reset();
    }

    QString error;
    bool success = false;
    if (!canceled())
    {
        // An exception leaving this handler would unwind through the worker's
        // event loop and terminate the process with the busy flag still set,
        // so the export it belongs to could never be reported or retried.
        try
        {
            success = request.kind == Kind::Image
                          ? writeImage(request, &error)
                          : writeAnimation(request, &error);
        }
        catch (const std::exception &exception)
        {
            success = false;
            error = tr("The export failed unexpectedly: %1")
                        .arg(QString::fromUtf8(exception.what()));
        }
        catch (...)
        {
            success = false;
            error = tr("The export failed unexpectedly.");
        }
    }
    const bool wasCanceled = !success && canceled();
    complete(request, success, wasCanceled, error);
}

bool ExportWorker::canceled() const
{
    QMutexLocker locker(&m_mutex);
    return m_cancelRequested;
}

void ExportWorker::postProgress(Kind kind, int value, int maximum)
{
    QMetaObject::invokeMethod(
        this,
        [this, kind, value, maximum]()
        {
            emit progress(kind, value, maximum);
        },
        Qt::QueuedConnection);
}

void ExportWorker::complete(const Request &request,
    bool success,
    bool wasCanceled,
    const QString &error)
{
    {
        QMutexLocker locker(&m_mutex);
        m_busy = false;
        m_cancelRequested = false;
        m_idleCondition.wakeAll();
    }
    QMetaObject::invokeMethod(
        this,
        [this, request, success, wasCanceled, error]()
        {
            emit finished(
                request.kind, success, wasCanceled, request.filePath, error);
        },
        Qt::QueuedConnection);
}

void ExportWorker::raiseRequestedTestingFailure()
{
    bool raise = false;
    {
        QMutexLocker locker(&m_mutex);
        raise = std::exchange(m_throwFromNextExportForTesting, false);
    }
    if (raise)
    {
        throw std::runtime_error("Injected export failure.");
    }
}

bool ExportWorker::writeImage(const Request &request, QString *error)
{
    raiseRequestedTestingFailure();
    QImage image = RenderEngine::render(request.document, request.frame);
    if (image.isNull())
    {
        *error = tr("The image could not be rendered.");
        return false;
    }
    if (request.jpeg && image.hasAlphaChannel())
    {
        // JPEG cannot store alpha. Compositing onto white keeps a transparent
        // background looking like the paper the user drew on; letting the
        // writer drop the channel would silently produce black instead.
        QImage opaque(image.size(), QImage::Format_RGB32);
        if (opaque.isNull())
        {
            *error = tr("The image could not be rendered.");
            return false;
        }
        opaque.fill(Qt::white);
        QPainter painter(&opaque);
        painter.drawImage(0, 0, image);
        painter.end();
        image = std::move(opaque);
    }
    if (canceled())
    {
        return false;
    }

    QSaveFile file(request.filePath);
    if (!file.open(QIODevice::WriteOnly))
    {
        *error = file.errorString();
        return false;
    }
    QImageWriter writer(&file, request.jpeg ? "JPEG" : "PNG");
    if (request.jpeg)
    {
        writer.setQuality(92);
    }
    if (!writer.write(image))
    {
        *error = writer.errorString();
        file.cancelWriting();
        return false;
    }
    if (canceled())
    {
        file.cancelWriting();
        return false;
    }
    if (!file.commit())
    {
        *error = file.errorString();
        return false;
    }
    return true;
}

bool ExportWorker::writeAnimation(const Request &request, QString *error)
{
    raiseRequestedTestingFailure();
    const QSize outputSize = request.animation.outputSize.isValid()
                                     && !request.animation.outputSize.isEmpty()
                                 ? request.animation.outputSize
                                 : request.document.size;
    const bool nativeSize = outputSize == request.document.size;

    QVector<QImage> frames;
    frames.reserve(request.document.animationFrames);
    postProgress(request.kind, 0, request.document.animationFrames);
    for (int frame = 0; frame < request.document.animationFrames; ++frame)
    {
        if (canceled())
        {
            return false;
        }
        // Exported pixels must not come from the preview replay path, which
        // trades exactness for speed; NativeExact renders natively and only
        // then scales.
        QImage image = nativeSize
                           ? RenderEngine::render(request.document, frame)
                           : RenderEngine::renderScaled(request.document,
                                 frame,
                                 outputSize,
                                 RenderEngine::ScaledRenderMode::NativeExact);
        if (image.isNull())
        {
            *error = tr("An animation frame could not be rendered.");
            return false;
        }
        if (!request.animation.preserveTransparency && image.hasAlphaChannel())
        {
            // Flattening onto white here rather than in the encoder keeps the
            // decision with the user's choice; the encoder only ever sees
            // frames that already mean what was asked for.
            QImage opaque(image.size(), QImage::Format_ARGB32);
            if (opaque.isNull())
            {
                *error = tr("An animation frame could not be rendered.");
                return false;
            }
            opaque.fill(Qt::white);
            QPainter painter(&opaque);
            painter.drawImage(0, 0, image);
            painter.end();
            image = std::move(opaque);
        }
        frames.append(std::move(image));
        postProgress(request.kind, frame + 1, request.document.animationFrames);
    }
    if (canceled())
    {
        return false;
    }
    const auto isCanceled = [this]()
    {
        return canceled();
    };
    if (request.kind == Kind::WebP)
    {
        return WebPWriter::write(request.filePath,
            frames,
            AnimationExportPolicy::frameDurations(
                request.document.animationFrames,
                request.document.framesPerSecond,
                1000),
            error,
            isCanceled);
    }
    return GifWriter::write(request.filePath,
        frames,
        AnimationExportPolicy::frameDurations(request.document.animationFrames,
            request.document.framesPerSecond,
            100),
        error,
        isCanceled);
}

}

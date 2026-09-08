// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "io/AnimationExportPolicy.hpp"
#include "io/DocumentSerializer.hpp"
#include "io/GifWriter.hpp"
#include "wasm/BridgeDocument.hpp"

#include <QFile>
#include <QVector>

#include <optional>
#include <utility>

using namespace ugurugu::wasm;

extern "C"
{
    // Encodes every animation frame into a GIF, matching the desktop export
    // path (NativeExact pixels, drift-corrected centisecond delays, alpha
    // preserved). The frame set is held in memory like the desktop worker, so
    // documents over the web budget are refused up front instead of risking a
    // tab kill. The pointer stays valid until the next export or close on the
    // same handle; the size comes from ugu_export_size().
    EMSCRIPTEN_KEEPALIVE const std::uint8_t *ugu_export_gif(
        BridgeDocument *handle)
    {
        const ugurugu::Document &document = handle->controller->document();
        constexpr qint64 webFrameSetBudget = 128LL * 1024LL * 1024LL;
        const qint64 frameSetBytes = static_cast<qint64>(document.size.width())
                                     * document.size.height() * 4LL
                                     * document.animationFrames;
        if (frameSetBytes > webFrameSetBudget)
        {
            setError(StatusExportTooLarge,
                QByteArrayLiteral("document is too large for web GIF export"));
            return nullptr;
        }

        QVector<QImage> frames;
        frames.reserve(document.animationFrames);
        for (int frame = 0; frame < document.animationFrames; ++frame)
        {
            QImage image = ugurugu::RenderEngine::render(document, frame);
            if (image.isNull())
            {
                setError(StatusRenderFailed,
                    QByteArrayLiteral("an animation frame could not render"));
                return nullptr;
            }
            frames.append(std::move(image));
        }

        const QString path = QStringLiteral("/ugurugu-export.gif");
        QString error;
        if (!ugurugu::GifWriter::write(path,
                frames,
                ugurugu::AnimationExportPolicy::frameDurations(
                    document.animationFrames, document.framesPerSecond, 100),
                &error))
        {
            setError(StatusExportFailed, error.toUtf8());
            return nullptr;
        }
        QFile output(path);
        if (!output.open(QIODevice::ReadOnly))
        {
            setError(StatusExportFailed,
                QByteArrayLiteral("encoded GIF could not be read"));
            return nullptr;
        }
        handle->exportBytes = output.readAll();
        output.close();
        QFile::remove(path);
        if (handle->exportBytes.isEmpty())
        {
            setError(
                StatusExportFailed, QByteArrayLiteral("encoded GIF is empty"));
            return nullptr;
        }
        clearError();
        return reinterpret_cast<const std::uint8_t *>(
            handle->exportBytes.constData());
    }

    EMSCRIPTEN_KEEPALIVE int ugu_export_size(const BridgeDocument *handle)
    {
        return static_cast<int>(handle->exportBytes.size());
    }

    // The pointer stays valid until the next serialize or close on the same
    // handle.
    EMSCRIPTEN_KEEPALIVE const std::uint8_t *ugu_serialize(
        BridgeDocument *handle)
    {
        handle->serialized =
            ugurugu::DocumentSerializer::toJson(handle->controller->document());
        if (handle->serialized.isEmpty())
        {
            setError(StatusExportFailed,
                QByteArrayLiteral("serialization produced no bytes"));
            return nullptr;
        }
        clearError();
        return reinterpret_cast<const std::uint8_t *>(
            handle->serialized.constData());
    }

    EMSCRIPTEN_KEEPALIVE int ugu_serialized_size(const BridgeDocument *handle)
    {
        return static_cast<int>(handle->serialized.size());
    }
}

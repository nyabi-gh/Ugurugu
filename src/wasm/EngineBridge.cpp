// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "document/DocumentLimits.hpp"
#include "io/serializer/SerializerSchema.hpp"
#include "wasm/BridgeDocument.hpp"

#include <QSize>
#include <QString>

#include <algorithm>
#include <cmath>
#include <memory>
#include <optional>
#include <utility>

using namespace ugurugu::wasm;

extern "C"
{
    // Bumped whenever an exported function is added, removed, or changes
    // meaning. The web worker refuses to run against a build whose version it
    // does not know, which turns a stale engine artifact into one clear error
    // instead of a missing-export TypeError somewhere later.
    EMSCRIPTEN_KEEPALIVE int ugu_abi_version()
    {
        return 9;
    }

    EMSCRIPTEN_KEEPALIVE int ugu_schema_version()
    {
        return ugurugu::serializer_detail::schemaVersion;
    }

    EMSCRIPTEN_KEEPALIVE const char *ugu_last_error()
    {
        return lastError().constData();
    }

    // 0 ok, 1 invalid argument, 2 out of memory, 3 invalid document,
    // 4 no paint layer, 5 stroke rejected, 6 render failed,
    // 7 export too large, 8 export failed, 9 no selection,
    // 10 empty region, 11 layer not drawable. Callers should branch on this
    // and use ugu_last_error() only for diagnostics.
    EMSCRIPTEN_KEEPALIVE int ugu_last_error_code()
    {
        return lastErrorCode();
    }

    EMSCRIPTEN_KEEPALIVE BridgeDocument *ugu_document_new(int width, int height)
    {
        if (width < ugurugu::DocumentLimits::minimumCanvasEdge
            || height < ugurugu::DocumentLimits::minimumCanvasEdge
            || width > ugurugu::DocumentLimits::maximumCanvasEdge
            || height > ugurugu::DocumentLimits::maximumCanvasEdge)
        {
            setError(StatusInvalidArgument,
                QByteArrayLiteral("canvas size is outside the engine limits"));
            return nullptr;
        }
        auto handle = std::make_unique<BridgeDocument>();
        attachSelectionHistory(handle.get());
        QString error;
        if (!handle->controller->newDocument(QSize(width, height), &error))
        {
            setError(StatusOutOfMemory, error.toUtf8());
            return nullptr;
        }
        clearError();
        return handle.release();
    }

    // Caps how many operations the undo stack keeps. The web shell sets this
    // from its memory profile because the engine has no byte-level history
    // budget; the desktop default of 64 is the upper bound.
    EMSCRIPTEN_KEEPALIVE void ugu_set_undo_limit(
        BridgeDocument *handle, int limit)
    {
        if (limit > 0)
        {
            handle->controller->undoStack()->setUndoLimit(limit);
        }
    }

    EMSCRIPTEN_KEEPALIVE BridgeDocument *ugu_document_open(
        const std::uint8_t *data, int size)
    {
        QString error;
        const QByteArray bytes(reinterpret_cast<const char *>(data), size);
        auto document = ugurugu::DocumentSerializer::fromJson(bytes, &error);
        if (!document)
        {
            setError(StatusDocumentInvalid, error.toUtf8());
            return nullptr;
        }
        auto handle = std::make_unique<BridgeDocument>();
        attachSelectionHistory(handle.get());
        if (!handle->controller->loadDocument(std::move(*document), &error))
        {
            setError(StatusDocumentInvalid, error.toUtf8());
            return nullptr;
        }
        clearError();
        return handle.release();
    }

    EMSCRIPTEN_KEEPALIVE void ugu_document_close(BridgeDocument *handle)
    {
        delete handle;
    }

    EMSCRIPTEN_KEEPALIVE int ugu_document_width(const BridgeDocument *handle)
    {
        return handle->controller->document().size.width();
    }

    EMSCRIPTEN_KEEPALIVE int ugu_document_height(const BridgeDocument *handle)
    {
        return handle->controller->document().size.height();
    }

    EMSCRIPTEN_KEEPALIVE int ugu_document_frame_count(
        const BridgeDocument *handle)
    {
        return handle->controller->document().animationFrames;
    }

    EMSCRIPTEN_KEEPALIVE int ugu_document_layer_count(
        const BridgeDocument *handle)
    {
        return static_cast<int>(handle->controller->document().layers.size());
    }

    EMSCRIPTEN_KEEPALIVE double ugu_document_fps(const BridgeDocument *handle)
    {
        return handle->controller->document().framesPerSecond;
    }

    EMSCRIPTEN_KEEPALIVE double ugu_document_wobble(
        const BridgeDocument *handle)
    {
        return handle->controller->document().wobbleAmount;
    }

    EMSCRIPTEN_KEEPALIVE int ugu_motion_style(const BridgeDocument *handle)
    {
        return static_cast<int>(handle->controller->document().motion.style);
    }

    EMSCRIPTEN_KEEPALIVE int ugu_motion_pose_count(const BridgeDocument *handle)
    {
        return handle->controller->document().motion.poseCount;
    }

    EMSCRIPTEN_KEEPALIVE int ugu_motion_detail(const BridgeDocument *handle)
    {
        return handle->controller->document().motion.detail;
    }

    EMSCRIPTEN_KEEPALIVE double ugu_motion_linked(const BridgeDocument *handle)
    {
        return handle->controller->document().motion.linked;
    }

    EMSCRIPTEN_KEEPALIVE double ugu_motion_randomness(
        const BridgeDocument *handle)
    {
        return handle->controller->document().motion.randomness;
    }

    EMSCRIPTEN_KEEPALIVE int ugu_motion_broken_line(
        const BridgeDocument *handle)
    {
        return handle->controller->document().motion.brokenLine ? 1 : 0;
    }

    EMSCRIPTEN_KEEPALIVE double ugu_motion_break_amount(
        const BridgeDocument *handle)
    {
        return handle->controller->document().motion.breakAmount;
    }

    EMSCRIPTEN_KEEPALIVE double ugu_motion_break_range(
        const BridgeDocument *handle)
    {
        return handle->controller->document().motion.breakRange;
    }

    // The whole wobble state in one call, so a change the artist makes with
    // one control is one undo entry rather than one per field.
    EMSCRIPTEN_KEEPALIVE int ugu_set_wobble(BridgeDocument *handle,
        double amount,
        int style,
        int poseCount,
        int detail,
        double linked,
        double randomness,
        int brokenLine,
        double breakAmount,
        double breakRange)
    {
        const auto motion = motionFromValues(style,
            poseCount,
            detail,
            linked,
            randomness,
            brokenLine,
            breakAmount,
            breakRange);
        if (!motion || !std::isfinite(amount))
        {
            setError(StatusInvalidArgument,
                QByteArrayLiteral("the wobble settings are out of range"));
            return 0;
        }
        if (!handle->controller->applyMotionPreset(amount, *motion))
        {
            setError(StatusInvalidArgument,
                QByteArrayLiteral("the wobble settings were rejected"));
            return 0;
        }
        invalidateSplit(handle);
        clearError();
        return 1;
    }

    EMSCRIPTEN_KEEPALIVE void ugu_set_animation_frames(
        BridgeDocument *handle, int frames)
    {
        handle->controller->setAnimationFrames(frames);
        invalidateSplit(handle);
    }

    EMSCRIPTEN_KEEPALIVE void ugu_set_fps(BridgeDocument *handle, double fps)
    {
        handle->controller->setFramesPerSecond(fps);
    }

    // Scales the artwork with the canvas. contentOffset-free by definition:
    // every stroke moves in proportion.
    EMSCRIPTEN_KEEPALIVE int ugu_resize_image(
        BridgeDocument *handle, int width, int height)
    {
        if (!handle->controller->resizeImage(QSize(width, height)))
        {
            setError(StatusInvalidArgument,
                QByteArrayLiteral("the image could not be resized"));
            return 0;
        }
        afterCanvasResize(handle);
        clearError();
        return 1;
    }

    // Keeps the artwork at its size and moves it by contentOffset inside the
    // new canvas, which is how the desktop crops and extends.
    EMSCRIPTEN_KEEPALIVE int ugu_resize_canvas(
        BridgeDocument *handle, int width, int height, int offsetX, int offsetY)
    {
        if (!handle->controller->resizeCanvas(
                QSize(width, height), QPoint(offsetX, offsetY)))
        {
            setError(StatusInvalidArgument,
                QByteArrayLiteral("the canvas could not be resized"));
            return 0;
        }
        afterCanvasResize(handle);
        clearError();
        return 1;
    }

    EMSCRIPTEN_KEEPALIVE int ugu_can_undo(const BridgeDocument *handle)
    {
        return handle->controller->undoStack()->canUndo() ? 1 : 0;
    }

    EMSCRIPTEN_KEEPALIVE int ugu_can_redo(const BridgeDocument *handle)
    {
        return handle->controller->undoStack()->canRedo() ? 1 : 0;
    }

    EMSCRIPTEN_KEEPALIVE void ugu_undo(BridgeDocument *handle)
    {
        handle->controller->undoStack()->undo();
        invalidateSplit(handle);
    }

    EMSCRIPTEN_KEEPALIVE void ugu_redo(BridgeDocument *handle)
    {
        handle->controller->undoStack()->redo();
        invalidateSplit(handle);
    }
}

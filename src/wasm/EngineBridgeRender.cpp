// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "wasm/BridgeDocument.hpp"

#include <QRect>

using namespace ugurugu::wasm;

extern "C"
{
    // The returned buffer is QImage::Format_ARGB32_Premultiplied: on the
    // little-endian wasm heap each pixel is the byte sequence B, G, R, A with
    // premultiplied color channels. Rows are ugu_frame_bytes_per_line() apart.
    // The pointer stays valid until the next render or close on the same
    // handle. ugu_dirty_* describe the region the last render actually
    // changed; consumers may upload just that region.
    EMSCRIPTEN_KEEPALIVE const std::uint8_t *ugu_render_frame(
        BridgeDocument *handle, int frameIndex)
    {
        if (handle->strokeInProgress)
        {
            // Playback advancing frames mid-stroke. A committed render would
            // wipe the live stroke off the screen and replace the image the
            // incremental compositor patches into, so later patches would land
            // on the wrong frame's pixels. Follow the playback frame with a
            // full preview instead; the incremental path stays off for the
            // rest of this stroke because its base image is gone.
            handle->strokeFrame = frameIndex;
            handle->incrementalActive = false;
            renderFullStrokePreview(handle);
        }
        else
        {
            renderCommittedFrame(handle, frameIndex);
        }
        if (handle->renderedFrame.isNull())
        {
            setError(StatusRenderFailed,
                QByteArrayLiteral("render produced a null frame"));
            return nullptr;
        }
        clearError();
        return handle->renderedFrame.constBits();
    }

    EMSCRIPTEN_KEEPALIVE const std::uint8_t *ugu_frame_pixels(
        const BridgeDocument *handle)
    {
        return handle->renderedFrame.constBits();
    }

    EMSCRIPTEN_KEEPALIVE int ugu_frame_width(const BridgeDocument *handle)
    {
        return handle->renderedFrame.width();
    }

    EMSCRIPTEN_KEEPALIVE int ugu_frame_height(const BridgeDocument *handle)
    {
        return handle->renderedFrame.height();
    }

    EMSCRIPTEN_KEEPALIVE int ugu_frame_bytes_per_line(
        const BridgeDocument *handle)
    {
        return static_cast<int>(handle->renderedFrame.bytesPerLine());
    }

    EMSCRIPTEN_KEEPALIVE int ugu_dirty_x(const BridgeDocument *handle)
    {
        return handle->dirty.x();
    }

    EMSCRIPTEN_KEEPALIVE int ugu_dirty_y(const BridgeDocument *handle)
    {
        return handle->dirty.y();
    }

    EMSCRIPTEN_KEEPALIVE int ugu_dirty_width(const BridgeDocument *handle)
    {
        return handle->dirty.width();
    }

    EMSCRIPTEN_KEEPALIVE int ugu_dirty_height(const BridgeDocument *handle)
    {
        return handle->dirty.height();
    }
}

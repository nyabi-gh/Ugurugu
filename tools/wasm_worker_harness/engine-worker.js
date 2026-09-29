// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

importScripts("ugurugu_engine_spike.js");

async function digestRows(engine, pixels, width, height, stride) {
    const packed = new Uint8Array(width * 4 * height);
    for (let row = 0; row < height; row += 1) {
        const start = pixels + row * stride;
        packed.set(
            engine.HEAPU8.subarray(start, start + width * 4),
            row * width * 4,
        );
    }
    const digest = await crypto.subtle.digest("SHA-256", packed);
    return Array.from(new Uint8Array(digest))
        .map((byte) => byte.toString(16).padStart(2, "0"))
        .join("");
}

self.onmessage = async (event) => {
    try {
        const bytes = new Uint8Array(event.data);
        const engine = await createUguruguEngine();
        const pointer = engine._malloc(bytes.length);
        engine.HEAPU8.set(bytes, pointer);
        const handle = engine._ugu_document_open(pointer, bytes.length);
        engine._free(pointer);
        if (!handle) {
            throw new Error(engine.UTF8ToString(engine._ugu_last_error()));
        }

        const frameCount = engine._ugu_document_frame_count(handle);
        const sampledFrames = [0, Math.floor(frameCount / 2), frameCount - 1];
        const frames = [];
        for (const frame of sampledFrames) {
            const pixels = engine._ugu_render_frame(handle, frame);
            if (!pixels) {
                throw new Error(engine.UTF8ToString(engine._ugu_last_error()));
            }
            frames.push({
                frame,
                width: engine._ugu_frame_width(handle),
                height: engine._ugu_frame_height(handle),
                digest: await digestRows(
                    engine,
                    pixels,
                    engine._ugu_frame_width(handle),
                    engine._ugu_frame_height(handle),
                    engine._ugu_frame_bytes_per_line(handle),
                ),
            });
        }

        const serializedPointer = engine._ugu_serialize(handle);
        if (!serializedPointer) {
            throw new Error(engine.UTF8ToString(engine._ugu_last_error()));
        }
        const serializedSize = engine._ugu_serialized_size(handle);
        const serialized = engine.HEAPU8.slice(
            serializedPointer,
            serializedPointer + serializedSize,
        );
        const serializedDigest = await crypto.subtle.digest(
            "SHA-256",
            serialized,
        );

        engine._ugu_document_close(handle);
        postMessage({
            ok: true,
            schemaVersion: engine._ugu_schema_version(),
            width: frames[0].width,
            height: frames[0].height,
            frameCount,
            frames,
            serializedBytes: serializedSize,
            serializedDigest: Array.from(new Uint8Array(serializedDigest))
                .map((byte) => byte.toString(16).padStart(2, "0"))
                .join(""),
        });
    } catch (error) {
        postMessage({ ok: false, error: String(error) });
    }
};

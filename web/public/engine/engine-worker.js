// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyattic)

importScripts("ugurugu_engine_spike.js");

// Must match ugu_abi_version() in src/wasm/EngineBridge.cpp. A stale artifact
// under public/engine used to surface as "… is not a function" deep inside an
// unrelated call; refusing here names the real problem instead.
const expectedAbiVersion = 9;

const enginePromise = createUguruguEngine().then((engine) => {
    const version = engine._ugu_abi_version?.();
    if (version !== expectedAbiVersion) {
        throw new Error(
            `engine ABI ${version ?? "unknown"} does not match the shell's ` +
                `${expectedAbiVersion}; rebuild the wasm-release preset and ` +
                `run npm run sync-engine`,
        );
    }
    return engine;
});
let documentHandle = 0;
// The outline is only worth reading back when it changed, so the worker
// remembers what it last sent. Replies leave the worker in request order, so
// the shell always sees a revision it can trust.
let sentSelectionRevision = -1;

function requireDocument() {
    if (!documentHandle) {
        throw new Error("no document open");
    }
    return documentHandle;
}

function historyState(engine) {
    const handle = requireDocument();
    return {
        canUndo: engine._ugu_can_undo(handle) === 1,
        canRedo: engine._ugu_can_redo(handle) === 1,
    };
}

function layerList(engine) {
    const handle = requireDocument();
    const count = engine._ugu_document_layer_count(handle);
    const layers = [];
    for (let index = 0; index < count; index += 1) {
        const wobble = engine.UTF8ToString(engine._ugu_layer_wobble(handle, index));
        layers.push({
            wobble: wobble ? JSON.parse(wobble) : null,
            index,
            // Stable across adds, removes and moves, unlike index. The shell
            // resolves a queued layer operation back to a row through it.
            id: engine.UTF8ToString(engine._ugu_layer_id(handle, index)),
            name: engine.UTF8ToString(
                engine._ugu_layer_name(handle, index),
            ),
            group: engine._ugu_layer_kind(handle, index) === 1,
            visible: engine._ugu_layer_visible(handle, index) === 1,
            reference: engine._ugu_layer_reference(handle, index) === 1,
            opacity: engine._ugu_layer_opacity(handle, index),
            active: engine._ugu_layer_is_active(handle, index) === 1,
            depth: engine._ugu_layer_depth(handle, index),
            blendMode: engine._ugu_layer_blend_mode(handle, index),
            clipped: engine._ugu_layer_clip_to_below(handle, index) === 1,
            // Available (0) means the merge would go through; the other
            // values are the reason it would not, and the shell says which.
            mergeStatus: engine._ugu_layer_merge_down_status(handle, index),
        });
    }
    return layers;
}

function presetList(engine) {
    const handle = requireDocument();
    const count = engine._ugu_brush_preset_count();
    const presets = [];
    for (let index = 0; index < count; index += 1) {
        presets.push({
            index,
            name: engine.UTF8ToString(
                engine._ugu_brush_preset_name(handle, index),
            ),
            defaultSize: engine._ugu_brush_preset_default_size(index),
        });
    }
    return presets;
}

function eraserPresetList(engine) {
    const handle = requireDocument();
    const count = engine._ugu_eraser_preset_count();
    const presets = [];
    for (let index = 0; index < count; index += 1) {
        presets.push({
            index,
            name: engine.UTF8ToString(
                engine._ugu_eraser_preset_name(handle, index),
            ),
            defaultSize: engine._ugu_eraser_preset_default_size(index),
        });
    }
    return presets;
}

function engineError(engine) {
    return new Error(
        `${engine.UTF8ToString(engine._ugu_last_error())} ` +
            `(code ${engine._ugu_last_error_code()})`,
    );
}

function documentMeta(engine) {
    const handle = requireDocument();
    return {
        abiVersion: engine._ugu_abi_version(),
        schemaVersion: engine._ugu_schema_version(),
        width: engine._ugu_document_width(handle),
        height: engine._ugu_document_height(handle),
        frameCount: engine._ugu_document_frame_count(handle),
        layerCount: engine._ugu_document_layer_count(handle),
        fps: engine._ugu_document_fps(handle),
        wobble: {
            amount: engine._ugu_document_wobble(handle),
            style: engine._ugu_motion_style(handle),
            poseCount: engine._ugu_motion_pose_count(handle),
            detail: engine._ugu_motion_detail(handle),
            linked: engine._ugu_motion_linked(handle),
            randomness: engine._ugu_motion_randomness(handle),
            brokenLine: engine._ugu_motion_broken_line(handle) === 1,
            breakAmount: engine._ugu_motion_break_amount(handle),
            breakRange: engine._ugu_motion_break_range(handle),
        },
        presets: presetList(engine),
        eraserPresets: eraserPresetList(engine),
        layers: layerList(engine),
        ...historyState(engine),
    };
}

function adoptDocument(engine, handle, undoLimit) {
    documentHandle = handle;
    // A fresh handle starts at revision 0 with no selection; forcing a
    // mismatch makes the first reply carry that empty outline.
    sentSelectionRevision = -1;
    if (undoLimit > 0) {
        engine._ugu_set_undo_limit(handle, undoLimit);
    }
    return documentMeta(engine);
}

function closeDocument(engine) {
    if (documentHandle) {
        engine._ugu_document_close(documentHandle);
        documentHandle = 0;
    }
}

// Both of these build the replacement first and only then let go of what is
// already open. Closing up front used to destroy the artist's document on the
// way to a failure — a corrupt file left the shell with no document at all and
// nothing to save. The cost is that two documents are live for the length of
// the parse, which MemoryPolicy's import ceiling already bounds.
function createDocument(engine, width, height, undoLimit) {
    const handle = engine._ugu_document_new(width, height);
    if (!handle) {
        throw engineError(engine);
    }
    closeDocument(engine);
    return adoptDocument(engine, handle, undoLimit);
}

function openDocument(engine, bytes, undoLimit) {
    const pointer = engine._malloc(bytes.length);
    engine.HEAPU8.set(bytes, pointer);
    const handle = engine._ugu_document_open(pointer, bytes.length);
    engine._free(pointer);
    if (!handle) {
        throw engineError(engine);
    }
    closeDocument(engine);
    return adoptDocument(engine, handle, undoLimit);
}

// The engine hands out premultiplied BGRA rows; putImageData wants straight
// RGBA, so pixel regions are swizzled and unpremultiplied here in the worker
// before they cross to the main thread.
function bgraToRgba(engine, pointer, offsetX, offsetY, width, height, stride) {
    const rgba = new Uint8ClampedArray(width * height * 4);
    for (let row = 0; row < height; row += 1) {
        const sourceStart = pointer + (offsetY + row) * stride + offsetX * 4;
        const source = engine.HEAPU8.subarray(
            sourceStart,
            sourceStart + width * 4,
        );
        const targetOffset = row * width * 4;
        for (let x = 0; x < width; x += 1) {
            const b = source[x * 4];
            const g = source[x * 4 + 1];
            const r = source[x * 4 + 2];
            const a = source[x * 4 + 3];
            const target = targetOffset + x * 4;
            if (a === 0 || a === 255) {
                rgba[target] = r;
                rgba[target + 1] = g;
                rgba[target + 2] = b;
            } else {
                rgba[target] = Math.round((r * 255) / a);
                rgba[target + 1] = Math.round((g * 255) / a);
                rgba[target + 2] = Math.round((b * 255) / a);
            }
            rgba[target + 3] = a;
        }
    }
    return rgba;
}

function dirtyRegion(engine) {
    const handle = requireDocument();
    const rect = {
        x: engine._ugu_dirty_x(handle),
        y: engine._ugu_dirty_y(handle),
        width: engine._ugu_dirty_width(handle),
        height: engine._ugu_dirty_height(handle),
    };
    if (rect.width <= 0 || rect.height <= 0) {
        return { rect: { x: 0, y: 0, width: 0, height: 0 }, pixels: null };
    }
    const rgba = bgraToRgba(
        engine,
        engine._ugu_frame_pixels(handle),
        rect.x,
        rect.y,
        rect.width,
        rect.height,
        engine._ugu_frame_bytes_per_line(handle),
    );
    return { rect, pixels: rgba.buffer };
}

function layerThumbnails(engine, devicePixelRatio) {
    const handle = requireDocument();
    const count = engine._ugu_document_layer_count(handle);
    const thumbnails = [];
    const transfers = [];
    for (let index = 0; index < count; index += 1) {
        const pointer = engine._ugu_layer_thumbnail(
            handle,
            index,
            devicePixelRatio,
        );
        if (!pointer) {
            thumbnails.push({ index, width: 0, height: 0, pixels: null });
            continue;
        }
        const width = engine._ugu_thumbnail_width(handle);
        const height = engine._ugu_thumbnail_height(handle);
        const rgba = bgraToRgba(
            engine,
            pointer,
            0,
            0,
            width,
            height,
            engine._ugu_thumbnail_bytes_per_line(handle),
        );
        thumbnails.push({ index, width, height, pixels: rgba.buffer });
        transfers.push(rgba.buffer);
    }
    return { thumbnails, transfers };
}

function selectionState(engine) {
    const handle = requireDocument();
    const revision = engine._ugu_selection_revision(handle);
    const state = {
        revision,
        active: engine._ugu_selection_active(handle) === 1,
        outline: null,
    };
    if (revision !== sentSelectionRevision) {
        sentSelectionRevision = revision;
        const size = engine._ugu_selection_outline_size(handle);
        if (size > 0) {
            const pointer = engine._ugu_selection_outline(handle);
            // Copies out of the wasm heap: the buffer moves when memory grows.
            state.outline = engine.HEAPU8.slice(
                pointer,
                pointer + size * 4,
            ).buffer;
        } else {
            state.outline = new ArrayBuffer(0);
        }
    }
    return state;
}

// For operations that move no pixels: the reply carries the outline and the
// panel state but no image data.
function stateReply(engine, id, meta = null) {
    const selection = selectionState(engine);
    postMessage(
        {
            id,
            ok: true,
            rect: { x: 0, y: 0, width: 0, height: 0 },
            pixels: null,
            layers: layerList(engine),
            selection,
            meta,
            ...historyState(engine),
        },
        selection.outline ? [selection.outline] : [],
    );
}

// meta rides along when a document property the shell mirrors — canvas size,
// frame count, fps, wobble — changed under the operation.
function regionReply(engine, id, meta = null) {
    const { rect, pixels } = dirtyRegion(engine);
    const selection = selectionState(engine);
    const message = {
        id,
        ok: true,
        rect,
        pixels,
        layers: layerList(engine),
        selection,
        meta,
        ...historyState(engine),
    };
    const transfers = [];
    if (pixels) {
        transfers.push(pixels);
    }
    if (selection.outline) {
        transfers.push(selection.outline);
    }
    postMessage(message, transfers);
}

function fullRender(engine, frame) {
    const handle = requireDocument();
    if (!engine._ugu_render_frame(handle, frame)) {
        throw engineError(engine);
    }
}

// Copies a flat x, y point list into the wasm heap as doubles and runs the
// call with it, freeing the buffer whichever way the call goes.
function withPoints(engine, points, run) {
    const bytes = points.length * 8;
    const pointer = engine._malloc(bytes);
    try {
        new Float64Array(engine.HEAPU8.buffer, pointer, points.length).set(
            points,
        );
        return run(pointer);
    } finally {
        engine._free(pointer);
    }
}

function serializeDocument(engine) {
    const handle = requireDocument();
    const pointer = engine._ugu_serialize(handle);
    if (!pointer) {
        throw engineError(engine);
    }
    const size = engine._ugu_serialized_size(handle);
    return engine.HEAPU8.slice(pointer, pointer + size).buffer;
}

function exportGif(engine) {
    const handle = requireDocument();
    const pointer = engine._ugu_export_gif(handle);
    if (!pointer) {
        throw engineError(engine);
    }
    const size = engine._ugu_export_size(handle);
    return engine.HEAPU8.slice(pointer, pointer + size).buffer;
}

self.onmessage = async (event) => {
    const { id, type } = event.data;
    try {
        const engine = await enginePromise;
        const handleFor = () => requireDocument();
        if (type === "open") {
            postMessage({
                id,
                ok: true,
                meta: openDocument(
                    engine,
                    new Uint8Array(event.data.bytes),
                    event.data.undoLimit,
                ),
            });
            return;
        }
        if (type === "create") {
            postMessage({
                id,
                ok: true,
                meta: createDocument(
                    engine,
                    event.data.width,
                    event.data.height,
                    event.data.undoLimit,
                ),
            });
            return;
        }
        if (type === "serialize") {
            const bytes = serializeDocument(engine);
            postMessage({ id, ok: true, bytes }, [bytes]);
            return;
        }
        if (type === "exportGif") {
            const bytes = exportGif(engine);
            postMessage({ id, ok: true, bytes }, [bytes]);
            return;
        }
        if (type === "layerThumbnails") {
            const { thumbnails, transfers } = layerThumbnails(
                engine,
                event.data.devicePixelRatio,
            );
            postMessage({ id, ok: true, thumbnails }, transfers);
            return;
        }
        if (type === "render") {
            fullRender(engine, event.data.frame);
        } else if (type === "brush") {
            const { red, green, blue, alpha, width, erase } = event.data;
            engine._ugu_set_brush(
                handleFor(),
                red,
                green,
                blue,
                alpha,
                width,
                erase ? 1 : 0,
            );
            postMessage({ id, ok: true });
            return;
        } else if (type === "brushPreset") {
            engine._ugu_set_brush_preset(handleFor(), event.data.index);
            postMessage({ id, ok: true });
            return;
        } else if (type === "eraserPreset") {
            engine._ugu_set_eraser_preset(handleFor(), event.data.index);
            postMessage({ id, ok: true });
            return;
        } else if (type === "brushAntialiasing") {
            engine._ugu_set_brush_antialiasing(
                handleFor(),
                event.data.antialiasing ? 1 : 0,
            );
            postMessage({ id, ok: true });
            return;
        } else if (type === "stabilization") {
            engine._ugu_set_stabilization(handleFor(), event.data.strength);
            postMessage({ id, ok: true });
            return;
        } else if (type === "fillOptions") {
            engine._ugu_set_fill_options(
                handleFor(),
                event.data.reference,
                event.data.comparison,
                event.data.tolerance,
                event.data.antialiasing ? 1 : 0,
            );
            postMessage({ id, ok: true });
            return;
        } else if (type === "bucketFill") {
            if (
                !engine._ugu_bucket_fill(
                    handleFor(),
                    event.data.frame,
                    event.data.x,
                    event.data.y,
                )
            ) {
                throw engineError(engine);
            }
        } else if (type === "selectionShape") {
            const applied = withPoints(engine, event.data.points, (pointer) =>
                engine._ugu_selection_shape(
                    handleFor(),
                    event.data.frame,
                    event.data.shape,
                    pointer,
                    event.data.points.length / 2,
                    event.data.combine,
                    event.data.paint ? 1 : 0,
                ),
            );
            if (!applied) {
                throw engineError(engine);
            }
            // Paint mode commits a fill; Select mode only moves the ants.
            if (event.data.paint) {
                regionReply(engine, id);
            } else {
                stateReply(engine, id);
            }
            return;
        } else if (type === "selectionFlood") {
            if (
                !engine._ugu_selection_flood(
                    handleFor(),
                    event.data.frame,
                    event.data.x,
                    event.data.y,
                    event.data.combine,
                )
            ) {
                throw engineError(engine);
            }
            stateReply(engine, id);
            return;
        } else if (type === "selectionAll") {
            if (!engine._ugu_selection_all(handleFor())) {
                throw engineError(engine);
            }
            stateReply(engine, id);
            return;
        } else if (type === "selectionInvert") {
            if (!engine._ugu_selection_invert(handleFor())) {
                throw engineError(engine);
            }
            stateReply(engine, id);
            return;
        } else if (type === "selectionClear") {
            engine._ugu_selection_clear(handleFor());
            stateReply(engine, id);
            return;
        } else if (type === "selectionTransformBegin") {
            if (
                !engine._ugu_selection_transform_begin(
                    handleFor(),
                    event.data.frame,
                )
            ) {
                throw engineError(engine);
            }
        } else if (type === "selectionTransformUpdate") {
            const [m11, m12, m21, m22, dx, dy] = event.data.matrix;
            if (
                !engine._ugu_selection_transform_update(
                    handleFor(),
                    m11,
                    m12,
                    m21,
                    m22,
                    dx,
                    dy,
                )
            ) {
                throw engineError(engine);
            }
        } else if (type === "selectionTransformApply") {
            if (!engine._ugu_selection_transform_apply(handleFor())) {
                throw engineError(engine);
            }
        } else if (type === "selectionTransformCancel") {
            engine._ugu_selection_transform_cancel(handleFor());
        } else if (type === "selectionFill") {
            if (!engine._ugu_selection_fill(handleFor(), event.data.frame)) {
                throw engineError(engine);
            }
        } else if (type === "selectionDelete") {
            if (!engine._ugu_selection_delete(handleFor(), event.data.frame)) {
                throw engineError(engine);
            }
        } else if (type === "strokeBegin") {
            const started = engine._ugu_stroke_begin(
                handleFor(),
                event.data.frame,
                event.data.x,
                event.data.y,
                event.data.pressure,
                event.data.timestamp,
            );
            if (!started) {
                throw engineError(engine);
            }
            engine._ugu_stroke_render(handleFor());
        } else if (type === "strokeAppend") {
            const handle = handleFor();
            const points = event.data.points;
            for (let index = 0; index < points.length; index += 4) {
                engine._ugu_stroke_append(
                    handle,
                    points[index],
                    points[index + 1],
                    points[index + 2],
                    points[index + 3],
                );
            }
            engine._ugu_stroke_render(handle);
        } else if (type === "strokeEnd") {
            // 0 Added, 1 AddedWithResampledPoints; anything else means the
            // stroke was dropped, which must not pass silently.
            if (engine._ugu_stroke_end(handleFor()) > 1) {
                throw engineError(engine);
            }
        } else if (type === "undo" || type === "redo") {
            const handle = handleFor();
            if (type === "undo") {
                engine._ugu_undo(handle);
            } else {
                engine._ugu_redo(handle);
            }
            // History reaches document properties too — canvas size, frames,
            // fps, wobble — so the shell is told what it is looking at now,
            // and the frame it asked for may no longer exist.
            const count = engine._ugu_document_frame_count(handle);
            fullRender(engine, Math.min(event.data.frame, count - 1));
            regionReply(engine, id, documentMeta(engine));
            return;
        } else if (type === "text") {
            const values = new Float64Array(event.data.commands);
            const pointer = engine._malloc(values.byteLength);
            if (!pointer) throw new Error("Not enough memory for text.");
            try {
                engine.HEAPU8.set(new Uint8Array(values.buffer), pointer);
                if (!engine._ugu_text_path(handleFor(), event.data.index, pointer,
                    values.length, event.data.x, event.data.y, event.data.width,
                    event.data.filled ? 1 : 0, event.data.color)) {
                    throw engineError(engine);
                }
            } finally { engine._free(pointer); }
            fullRender(engine, event.data.frame);
        } else if (type === "insertImage") {
            const pixels = new Uint8Array(event.data.pixels);
            const name = new TextEncoder().encode(event.data.name + "\0");
            const pointer = engine._malloc(pixels.length);
            const namePointer = engine._malloc(name.length);
            try {
                if (!pointer || !namePointer) throw new Error("Not enough memory for the image.");
                engine.HEAPU8.set(pixels, pointer);
                engine.HEAPU8.set(name, namePointer);
                if (!engine._ugu_insert_image(handleFor(), pointer, pixels.length,
                    event.data.width, event.data.height, namePointer)) {
                    throw engineError(engine);
                }
            } finally { engine._free(pointer); engine._free(namePointer); }
            fullRender(engine, event.data.frame);
        } else if (type === "layerActivate") {
            engine._ugu_layer_activate(handleFor(), event.data.index);
            fullRender(engine, event.data.frame);
        } else if (type === "layerVisible") {
            engine._ugu_layer_set_visible(
                handleFor(),
                event.data.index,
                event.data.visible ? 1 : 0,
            );
            fullRender(engine, event.data.frame);
        } else if (type === "wobble") {
            const settings = event.data.wobble;
            if (
                !engine._ugu_set_wobble(
                    handleFor(),
                    settings.amount,
                    settings.style,
                    settings.poseCount,
                    settings.detail,
                    settings.linked,
                    settings.randomness,
                    settings.brokenLine ? 1 : 0,
                    settings.breakAmount,
                    settings.breakRange,
                )
            ) {
                throw engineError(engine);
            }
            fullRender(engine, event.data.frame);
            regionReply(engine, id, documentMeta(engine));
            return;
        } else if (type === "layerWobble") {
            const settings = event.data.wobble ?? documentMeta(engine).wobble;
            if (!engine._ugu_layer_set_wobble(handleFor(), event.data.index,
                event.data.wobble ? 1 : 0, settings.amount, settings.style,
                settings.poseCount, settings.detail, settings.linked,
                settings.randomness, settings.brokenLine ? 1 : 0,
                settings.breakAmount, settings.breakRange)) {
                throw engineError(engine);
            }
            fullRender(engine, event.data.frame);
        } else if (type === "animationFrames") {
            const handle = handleFor();
            engine._ugu_set_animation_frames(handle, event.data.frames);
            // The frame the shell was showing may no longer exist.
            const count = engine._ugu_document_frame_count(handle);
            fullRender(engine, Math.min(event.data.frame, count - 1));
            regionReply(engine, id, documentMeta(engine));
            return;
        } else if (type === "framesPerSecond") {
            engine._ugu_set_fps(handleFor(), event.data.fps);
            stateReply(engine, id, documentMeta(engine));
            return;
        } else if (type === "resizeImage") {
            if (
                !engine._ugu_resize_image(
                    handleFor(),
                    event.data.width,
                    event.data.height,
                )
            ) {
                throw engineError(engine);
            }
            fullRender(engine, event.data.frame);
            regionReply(engine, id, documentMeta(engine));
            return;
        } else if (type === "resizeCanvas") {
            if (
                !engine._ugu_resize_canvas(
                    handleFor(),
                    event.data.width,
                    event.data.height,
                    event.data.offsetX,
                    event.data.offsetY,
                )
            ) {
                throw engineError(engine);
            }
            fullRender(engine, event.data.frame);
            regionReply(engine, id, documentMeta(engine));
            return;
        } else if (type === "layerReference") {
            engine._ugu_layer_set_reference(
                handleFor(),
                event.data.index,
                event.data.reference ? 1 : 0,
            );
            // The flag only picks which layers a flood fill reads; no pixel
            // in the frame moves, so nothing has to be rendered or uploaded.
            stateReply(engine, id);
            return;
        } else if (type === "layerOpacity") {
            engine._ugu_layer_set_opacity(
                handleFor(),
                event.data.index,
                event.data.opacity,
            );
            fullRender(engine, event.data.frame);
        } else if (type === "layerAdd") {
            engine._ugu_layer_add(handleFor());
            fullRender(engine, event.data.frame);
        } else if (type === "layerRemove") {
            engine._ugu_layer_remove(handleFor(), event.data.index);
            fullRender(engine, event.data.frame);
        } else if (type === "layerRename") {
            const utf8 = new TextEncoder().encode(event.data.name + "\0");
            const namePointer = engine._malloc(utf8.length);
            engine.HEAPU8.set(utf8, namePointer);
            engine._ugu_layer_rename(
                handleFor(),
                event.data.index,
                namePointer,
            );
            engine._free(namePointer);
            fullRender(engine, event.data.frame);
        } else if (type === "layerMove") {
            engine._ugu_layer_move(
                handleFor(),
                event.data.index,
                event.data.offset,
            );
            fullRender(engine, event.data.frame);
        } else if (type === "layerAddGroup") {
            engine._ugu_layer_add_group(handleFor(), event.data.index);
            fullRender(engine, event.data.frame);
        } else if (type === "layerDuplicate") {
            engine._ugu_layer_duplicate(handleFor(), event.data.index);
            fullRender(engine, event.data.frame);
        } else if (type === "layerClear") {
            engine._ugu_layer_clear(handleFor(), event.data.index);
            fullRender(engine, event.data.frame);
        } else if (type === "layerMergeDown") {
            if (!engine._ugu_layer_merge_down(handleFor(), event.data.index)) {
                throw engineError(engine);
            }
            fullRender(engine, event.data.frame);
        } else if (type === "layerBlendMode") {
            engine._ugu_layer_set_blend_mode(
                handleFor(),
                event.data.index,
                event.data.mode,
            );
            fullRender(engine, event.data.frame);
        } else if (type === "layerClipToBelow") {
            engine._ugu_layer_set_clip_to_below(
                handleFor(),
                event.data.index,
                event.data.clipped ? 1 : 0,
            );
            fullRender(engine, event.data.frame);
        } else if (type === "layerParentGroup") {
            engine._ugu_layer_set_parent_group(
                handleFor(),
                event.data.index,
                event.data.groupIndex,
            );
            fullRender(engine, event.data.frame);
        } else if (type === "clipboardCopy") {
            if (!engine._ugu_selection_copy(handleFor(), event.data.frame)) {
                throw engineError(engine);
            }
            fullRender(engine, event.data.frame);
        } else if (type === "clipboardCut") {
            if (!engine._ugu_selection_cut(handleFor(), event.data.frame)) {
                throw engineError(engine);
            }
            fullRender(engine, event.data.frame);
        } else if (type === "clipboardPaste") {
            if (!engine._ugu_clipboard_paste(handleFor(), event.data.frame)) {
                throw engineError(engine);
            }
            fullRender(engine, event.data.frame);
        } else {
            throw new Error(`unknown request: ${type}`);
        }
        regionReply(engine, id);
    } catch (error) {
        postMessage({ id, ok: false, error: String(error) });
    }
};

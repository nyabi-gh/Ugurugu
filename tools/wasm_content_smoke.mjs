// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyattic)

import assert from "node:assert/strict";
const { default: createEngine } = await import(
    new URL(
        "../out/build/wasm-release/ugurugu_engine_spike.js",
        import.meta.url,
    ).href
);
const engine = await createEngine();
assert.equal(engine._ugu_abi_version(), 9);
const handle = engine._ugu_document_new(128, 96);
assert.ok(handle);
const serialize = (h = handle) => {
    const pointer = engine._ugu_serialize(h);
    assert.ok(pointer);
    return Buffer.from(
        engine.HEAPU8.subarray(
            pointer,
            pointer + engine._ugu_serialized_size(h),
        ),
    );
};
const withBytes = (bytes, fn) => {
    const pointer = engine._malloc(bytes.byteLength);
    assert.ok(pointer);
    try {
        engine.HEAPU8.set(bytes, pointer);
        return fn(pointer);
    } finally {
        engine._free(pointer);
    }
};
const blank = serialize();
const wobble = (enabled, amount = 0) =>
    engine._ugu_layer_set_wobble(
        handle,
        0,
        enabled,
        amount,
        1,
        8,
        12,
        1,
        0,
        0,
        0.35,
        24,
    );
assert.equal(wobble(1), 1);
assert.equal(
    JSON.parse(engine.UTF8ToString(engine._ugu_layer_wobble(handle, 0))).amount,
    0,
);
assert.equal(
    JSON.parse(serialize()).animation.wobble,
    JSON.parse(blank).animation.wobble,
);
assert.equal(wobble(0), 1);
assert.equal(engine.UTF8ToString(engine._ugu_layer_wobble(handle, 0)), "");
engine._ugu_undo(handle);
assert.equal(
    JSON.parse(engine.UTF8ToString(engine._ugu_layer_wobble(handle, 0))).amount,
    0,
);
engine._ugu_undo(handle);
assert.deepEqual(serialize(), blank);

// A glyph-like outer ring and a hole, laid out without any QGuiApplication.
const commands = new Float64Array([
    0, 0, 0, 0, 0, 0, 0, 1, 50, 0, 0, 0, 0, 0, 1, 50, 50, 0, 0, 0, 0, 1, 0, 50,
    0, 0, 0, 0, 4, 0, 0, 0, 0, 0, 0, 0, 15, 15, 0, 0, 0, 0, 1, 15, 35, 0, 0, 0,
    0, 1, 35, 35, 0, 0, 0, 0, 1, 35, 15, 0, 0, 0, 0, 4, 0, 0, 0, 0, 0, 0,
]);
const addText = (count = commands.length, target = handle) =>
    withBytes(new Uint8Array(commands.buffer), (pointer) =>
        engine._ugu_text_path(
            target,
            0,
            pointer,
            count,
            10,
            10,
            2,
            1,
            0xff1d2129,
        ),
    );
assert.equal(addText(commands.length - 1), 0);
assert.deepEqual(
    serialize(),
    blank,
    "invalid text must leave the document unchanged",
);
engine._ugu_layer_set_visible(handle, 0, 0);
const hidden = serialize();
assert.equal(addText(), 0);
assert.deepEqual(serialize(), hidden, "text must not paint a hidden layer");
engine._ugu_undo(handle);
assert.equal(addText(), 1);
const text = serialize();
assert.ok(JSON.parse(text).layers[0].strokes.length >= 3);
engine._ugu_undo(handle);
assert.deepEqual(
    serialize(),
    blank,
    "one undo removes every glyph contour and fill",
);
engine._ugu_redo(handle);
assert.deepEqual(serialize(), text);

const rgba = new Uint8Array(8 * 4 * 4);
for (let i = 0; i < rgba.length; i += 4) {
    rgba.set([255, 20, 40, 128], i);
}
withBytes(new TextEncoder().encode("Sample.png\0"), (name) =>
    withBytes(rgba, (pixels) => {
        assert.equal(
            engine._ugu_insert_image(
                handle,
                pixels,
                rgba.length - 1,
                8,
                4,
                name,
            ),
            0,
        );
        assert.deepEqual(serialize(), text);
        assert.equal(
            engine._ugu_insert_image(handle, pixels, rgba.length, 8, 4, name),
            1,
        );
    }),
);
const image = serialize();
assert.equal(JSON.parse(image).layers.length, 2);
assert.equal(JSON.parse(image).layers[1].name, "Sample");
engine._ugu_undo(handle);
assert.deepEqual(serialize(), text);
engine._ugu_redo(handle);
assert.deepEqual(serialize(), image);
const reopened = withBytes(image, (pointer) =>
    engine._ugu_document_open(pointer, image.length),
);
assert.ok(reopened);
assert.deepEqual(
    serialize(reopened),
    image,
    "all new content survives a serializer round trip",
);
// Leave only one stroke slot, then try to insert a multi-contour glyph.
// Rejection of the second contour must roll back the first one as well.
const nearLimit = JSON.parse(blank);
const prototype = JSON.parse(text).layers[0].strokes.find(
    (stroke) => stroke.mode === "paint",
);
nearLimit.layers[0].strokes = Array.from({ length: 19999 }, (_, index) => ({
    ...prototype,
    id: `${index.toString(16).padStart(8, "0")}-0000-4000-8000-000000000000`,
    points: [
        [1, 1, 1],
        [2, 2, 1],
    ],
}));
const limitBytes = Buffer.from(JSON.stringify(nearLimit));
const limited = withBytes(limitBytes, (pointer) =>
    engine._ugu_document_open(pointer, limitBytes.length),
);
assert.ok(limited);
const beforeRejectedText = serialize(limited);
assert.equal(addText(commands.length, limited), 0);
assert.deepEqual(
    serialize(limited),
    beforeRejectedText,
    "rejected text rolls back every partial contour",
);
engine._ugu_document_close(limited);
engine._ugu_document_close(reopened);
engine._ugu_document_close(handle);
console.log(
    "OK: layer wobble, headless text, image insertion, rejection, undo/redo and round trip",
);

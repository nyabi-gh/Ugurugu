// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

// Measures the wasm engine against one or more .ugu documents: document open
// time, first-render and stroke-begin latency, per-batch stroke append+render
// latency percentiles, serialize time, and peak wasm linear memory. Wasm
// memory only grows, so the heap size after each phase is that phase's peak.
//
// Stroke begin is reported twice because the engine promotes the layer split
// across a committed stroke. "cold" is the first stroke after a full render or
// an undo, which has to build the split; "warm" is every following stroke,
// which reuses it. Real drawing is overwhelmingly the warm path, so the loop
// here deliberately does not undo between strokes.
//
// usage: node tools/wasm_engine_bench.mjs <document.ugu> [more.ugu ...]

import { readFile } from "node:fs/promises";
import { basename } from "node:path";
import { fileURLToPath } from "node:url";
import { performance } from "node:perf_hooks";

const modulePath = new URL(
    "../out/build/wasm-release/ugurugu_engine_spike.js",
    import.meta.url,
);

const strokesPerDocument = 8;
const batchesPerStroke = 50;
const pointsPerBatch = 4;

function percentile(samples, fraction) {
    const sorted = [...samples].sort((a, b) => a - b);
    const index = Math.min(
        sorted.length - 1,
        Math.ceil(fraction * sorted.length) - 1,
    );
    return sorted[Math.max(0, index)];
}

function megabytes(bytes) {
    return (bytes / (1024 * 1024)).toFixed(1);
}

const { default: createUguruguEngine } = await import(
    fileURLToPath(modulePath)
);

async function benchDocument(path) {
    const engine = await createUguruguEngine();
    const heapSize = () => engine.HEAPU8.buffer.byteLength;
    const initialHeap = heapSize();

    const bytes = await readFile(path);
    const pointer = engine._malloc(bytes.length);
    engine.HEAPU8.set(bytes, pointer);
    const openStart = performance.now();
    const handle = engine._ugu_document_open(pointer, bytes.length);
    const openMs = performance.now() - openStart;
    engine._free(pointer);
    if (!handle) {
        throw new Error(engine.UTF8ToString(engine._ugu_last_error()));
    }
    const heapAfterOpen = heapSize();

    const width = engine._ugu_document_width(handle);
    const height = engine._ugu_document_height(handle);

    const renderStart = performance.now();
    if (!engine._ugu_render_frame(handle, 0)) {
        throw new Error(engine.UTF8ToString(engine._ugu_last_error()));
    }
    const firstRenderMs = performance.now() - renderStart;
    const heapAfterRender = heapSize();

    engine._ugu_set_brush(handle, 30, 60, 120, 255, 12.0, 0);
    const beginSamples = [];
    const batchSamples = [];
    const commitSamples = [];
    let coldBeginMs = 0;
    for (let stroke = 0; stroke < strokesPerDocument; stroke += 1) {
        const originX = 8 + (stroke * 97) % Math.max(1, width - 16);
        const originY = 8 + (stroke * 61) % Math.max(1, height - 16);
        let timestamp = stroke * 1000;

        const beginStart = performance.now();
        if (
            !engine._ugu_stroke_begin(
                handle,
                0,
                originX,
                originY,
                0.7,
                timestamp,
            )
        ) {
            throw new Error(engine.UTF8ToString(engine._ugu_last_error()));
        }
        engine._ugu_stroke_render(handle);
        const beginMs = performance.now() - beginStart;
        if (stroke === 0) {
            coldBeginMs = beginMs;
        } else {
            beginSamples.push(beginMs);
        }

        for (let batch = 0; batch < batchesPerStroke; batch += 1) {
            const batchStart = performance.now();
            for (let point = 0; point < pointsPerBatch; point += 1) {
                const step = batch * pointsPerBatch + point;
                timestamp += 8;
                const x = Math.min(
                    width - 2,
                    originX + step * 1.5 + Math.sin(step * 0.11) * 24,
                );
                const y = Math.min(
                    height - 2,
                    originY + step * 1.1 + Math.cos(step * 0.13) * 24,
                );
                engine._ugu_stroke_append(handle, x, y, 0.8, timestamp);
            }
            engine._ugu_stroke_render(handle);
            batchSamples.push(performance.now() - batchStart);
        }

        const commitStart = performance.now();
        engine._ugu_stroke_end(handle);
        commitSamples.push(performance.now() - commitStart);
    }
    const heapAfterStrokes = heapSize();

    const serializeStart = performance.now();
    if (!engine._ugu_serialize(handle)) {
        throw new Error(engine.UTF8ToString(engine._ugu_last_error()));
    }
    const serializeMs = performance.now() - serializeStart;
    const serializedBytes = engine._ugu_serialized_size(handle);
    const peakHeap = heapSize();

    engine._ugu_document_close(handle);

    return {
        document: basename(path),
        fileBytes: bytes.length,
        width,
        height,
        openMs,
        firstRenderMs,
        strokeBeginColdMs: coldBeginMs,
        strokeBeginWarmP95Ms: percentile(beginSamples, 0.95),
        batchP50Ms: percentile(batchSamples, 0.5),
        batchP95Ms: percentile(batchSamples, 0.95),
        batchMaxMs: Math.max(...batchSamples),
        commitP95Ms: percentile(commitSamples, 0.95),
        serializeMs,
        serializedBytes,
        initialHeap,
        heapAfterOpen,
        heapAfterRender,
        heapAfterStrokes,
        peakHeap,
    };
}

const paths = process.argv.slice(2);
if (paths.length === 0) {
    console.error("usage: node tools/wasm_engine_bench.mjs <document.ugu> ...");
    process.exit(2);
}

const results = [];
for (const path of paths) {
    const result = await benchDocument(path);
    results.push(result);
    console.log(`\n${result.document} (${result.width}x${result.height}, ${megabytes(result.fileBytes)} MiB file)`);
    console.log(`  open: ${result.openMs.toFixed(0)} ms`);
    console.log(`  first full render: ${result.firstRenderMs.toFixed(0)} ms`);
    console.log(
        `  stroke begin cold/warm p95: ` +
            `${result.strokeBeginColdMs.toFixed(0)} / ` +
            `${result.strokeBeginWarmP95Ms.toFixed(1)} ms`,
    );
    console.log(
        `  stroke batch p50/p95/max: ${result.batchP50Ms.toFixed(1)} / ` +
            `${result.batchP95Ms.toFixed(1)} / ${result.batchMaxMs.toFixed(1)} ms`,
    );
    console.log(`  stroke commit p95: ${result.commitP95Ms.toFixed(1)} ms`);
    console.log(
        `  serialize: ${result.serializeMs.toFixed(0)} ms ` +
            `(${megabytes(result.serializedBytes)} MiB)`,
    );
    console.log(
        `  wasm heap: initial ${megabytes(result.initialHeap)} -> ` +
            `open ${megabytes(result.heapAfterOpen)} -> ` +
            `render ${megabytes(result.heapAfterRender)} -> ` +
            `strokes ${megabytes(result.heapAfterStrokes)} -> ` +
            `peak ${megabytes(result.peakHeap)} MiB`,
    );
}

console.log(`\nJSON: ${JSON.stringify(results)}`);

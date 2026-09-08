// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyattic)

// Drives the built web shell (web/dist) in headless Chromium: the IndexedDB
// recovery loop, drawing and selection, exports, the view transform, the
// document's own properties, and the failure paths behind all of them. Run
// `npm run build` first, then `npm run test:browser`.
//
// One scenario per file under scenarios/. Passing a substring runs only the
// scenarios whose name contains it — `npm run test:browser -- selection`.

import { chromium, failures, resolveChromiumPath, startServer } from "./harness.mjs";
import recoveryAndPngExport from "./scenarios/01-recovery-and-png-export.mjs";
import newDocument from "./scenarios/02-new-document.mjs";
import indexeddbFailure from "./scenarios/03-indexeddb-failure.mjs";
import selectionAndFillTools from "./scenarios/04-selection-and-fill-tools.mjs";
import failedOpen from "./scenarios/05-failed-open.mjs";
import hiddenLayer from "./scenarios/06-hidden-layer.mjs";
import brushAndEraserSizes from "./scenarios/07-brush-and-eraser-sizes.mjs";
import translucentLayers from "./scenarios/08-translucent-layers.mjs";
import webglContextLoss from "./scenarios/09-webgl-context-loss.mjs";
import playbackBackpressure from "./scenarios/10-playback-backpressure.mjs";
import missingEngine from "./scenarios/11-missing-engine.mjs";
import backToBackStrokes from "./scenarios/12-back-to-back-strokes.mjs";
import layerCommands from "./scenarios/13-layer-commands.mjs";
import phoneSizedViewport from "./scenarios/14-phone-sized-viewport.mjs";
import canvasRotation from "./scenarios/15-canvas-rotation.mjs";
import twoFingerGestures from "./scenarios/16-two-finger-gestures.mjs";
import selectionTransform from "./scenarios/17-selection-transform.mjs";
import licenceNotices from "./scenarios/18-licence-notices.mjs";
import documentProperties from "./scenarios/19-document-properties.mjs";
import clipboardAndLayers from "./scenarios/20-clipboard-and-layers.mjs";

import textWobbleImage from "./scenarios/21-text-wobble-image.mjs";

// Order matters only in that the cheapest, most fundamental checks come first:
// a failure in scenario 1 makes every later one meaningless.
const scenarios = [
    ["recovery-and-png-export", recoveryAndPngExport],
    ["new-document", newDocument],
    ["indexeddb-failure", indexeddbFailure],
    ["selection-and-fill-tools", selectionAndFillTools],
    ["failed-open", failedOpen],
    ["hidden-layer", hiddenLayer],
    ["brush-and-eraser-sizes", brushAndEraserSizes],
    ["translucent-layers", translucentLayers],
    ["webgl-context-loss", webglContextLoss],
    ["playback-backpressure", playbackBackpressure],
    ["missing-engine", missingEngine],
    ["back-to-back-strokes", backToBackStrokes],
    ["layer-commands", layerCommands],
    ["phone-sized-viewport", phoneSizedViewport],
    ["canvas-rotation", canvasRotation],
    ["two-finger-gestures", twoFingerGestures],
    ["selection-transform", selectionTransform],
    ["licence-notices", licenceNotices],
    ["document-properties", documentProperties],
    ["clipboard-and-layers", clipboardAndLayers],
    ["text-wobble-image", textWobbleImage],
];

const filter = process.argv.slice(2).find((argument) => !argument.startsWith("--"));
const selected = scenarios.filter(([name]) => !filter || name.includes(filter));
if (selected.length === 0) {
    console.error(`No scenario matches "${filter}"`);
    process.exit(1);
}

const { server, origin } = await startServer();
const browser = await chromium.launch({
    executablePath: resolveChromiumPath(),
    headless: true,
});

for (const [name, run] of selected) {
    console.log(`\n— ${name}`);
    await run({ browser, origin });
}

await browser.close();
server.close();

if (failures.length > 0) {
    console.error(`\n${failures.length} check(s) failed`);
    process.exit(1);
}
console.log(`\nOK: ${selected.length} browser scenarios verified`);

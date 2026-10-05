// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

import { fileURLToPath } from "node:url";
import {
    acceptReplacePrompts,
    check,
    countBrushPixels,
    dragBetween,
    drawStroke,
    engineFault,
    eventually,
    installPixelCounter,
    startServer,
    waitForDocumentLoaded,
} from "../harness.mjs";

const wave = fileURLToPath(new URL("../../../examples/Wave.ugu", import.meta.url));

// The file is copied into the wasm heap before the engine parses it. When that
// allocation fails the open has to fail by name: writing the bytes through a
// null pointer lands on the engine's own static data.
export default async function run({ browser }) {
    // Wave.ugu is a few KiB; strokes and layer names stay far below this.
    const faulty = await startServer({
        engineFault: engineFault({ failMallocAtLeast: 1024 }),
    });
    const context = await browser.newContext();
    const page = await context.newPage();
    acceptReplacePrompts(page);
    await installPixelCounter(page);
    await page.goto(faulty.origin);
    await waitForDocumentLoaded(page);
    await drawStroke(page);
    const drawn = await countBrushPixels(page);

    await page.locator("#open-document").setInputFiles(wave);
    check(
        await eventually(
            page,
            () =>
                /^Open failed.*memory/i.test(
                    document.querySelector("#status")?.textContent ?? "",
                ),
        ),
        "an open whose heap copy cannot be allocated fails as out of memory",
    );
    check(
        (await countBrushPixels(page)) === drawn,
        "the open document is kept when the heap copy fails",
    );

    const box = await page.locator("#display-canvas").boundingBox();
    await dragBetween(
        page,
        { x: box.x + 60, y: box.y + box.height - 60 },
        { x: box.x + 220, y: box.y + box.height - 60 },
    );
    check(
        await eventually(
            page,
            (before) => window.__uguruguBrushCount?.() > before,
            drawn,
        ),
        "the engine keeps drawing after a failed heap copy",
    );
    await context.close();
    faulty.server.close();
}

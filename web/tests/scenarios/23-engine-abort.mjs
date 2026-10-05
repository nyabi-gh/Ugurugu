// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

import {
    check,
    countBrushPixels,
    dragBetween,
    drawStroke,
    engineFault,
    eventually,
    installPixelCounter,
    recoveryRecords,
    startServer,
    waitForDocumentLoaded,
} from "../harness.mjs";

// A wasm abort unwinds the engine mid-operation without running any C++
// cleanup, so whatever it was changing is left half done. From then on the
// engine must not be used — above all, not serialized over the last good
// recovery snapshot.
export default async function run({ browser }) {
    const faulty = await startServer({
        engineFault: engineFault({ abortIn: "_ugu_layer_add" }),
    });
    const context = await browser.newContext();
    const page = await context.newPage();
    await installPixelCounter(page);
    await page.goto(`${faulty.origin}/?autosave=1`);
    await waitForDocumentLoaded(page);
    await drawStroke(page);
    await page.waitForFunction(
        () =>
            document
                .querySelector("#autosave-status")
                ?.textContent.includes("Recovery snapshot saved"),
        undefined,
        { timeout: 20000 },
    );
    const before = await recoveryRecords(page);
    check(before.length > 0, "a recovery snapshot exists before the abort");
    const drawn = await countBrushPixels(page);

    await page.locator("#layer-add").click();
    check(
        await eventually(page, () => {
            const status = document.querySelector("#status")?.textContent ?? "";
            return /engine stopped/i.test(status) && /reload/i.test(status);
        }),
        "an engine abort is reported as fatal, with the way back",
    );

    const box = await page.locator("#display-canvas").boundingBox();
    await dragBetween(
        page,
        { x: box.x + 60, y: box.y + box.height - 60 },
        { x: box.x + 220, y: box.y + box.height - 60 },
    );
    // Three autosave intervals: long enough for a snapshot to have been taken
    // if anything still allowed one.
    await page.waitForTimeout(3000);
    check(
        (await countBrushPixels(page)) === drawn,
        "nothing is drawn on an engine that aborted",
    );
    const after = await recoveryRecords(page);
    check(
        JSON.stringify(after) === JSON.stringify(before),
        "the last good recovery snapshot survives an engine abort",
    );
    await context.close();
    faulty.server.close();
}

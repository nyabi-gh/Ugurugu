// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyattic)

import { writeFile } from "node:fs/promises";
import {
    check,
    countBrushPixels,
    dragBetween,
    drawStroke,
    installPixelCounter,
    scratchFile,
    waitForDocumentLoaded,
} from "../harness.mjs";

// a failed open must not take the open document with it.
export default async function run({ browser, origin }) {
    const context = await browser.newContext();
    const page = await context.newPage();
    await installPixelCounter(page);
    await page.goto(origin);
    await waitForDocumentLoaded(page);
    await drawStroke(page);
    const drawn = await countBrushPixels(page);

    const corrupt = scratchFile("corrupt.ugu");
    await writeFile(corrupt, "this is not a ugu document");
    await page.locator('#open-document').setInputFiles(corrupt);
    await page.waitForFunction(
        () =>
            document
                .querySelector("#status")
                ?.textContent.startsWith("Open failed"),
        undefined,
        { timeout: 20000 },
    );
    check(true, "a corrupt file reports an open failure");
    check(
        await page.locator("#save-document").isEnabled(),
        "the document that was open survives a failed open",
    );
    check(
        (await countBrushPixels(page)) === drawn,
        "a failed open leaves the artwork untouched",
    );

    // Still live: another stroke commits on the document that was kept.
    const box = await page.locator("#display-canvas").boundingBox();
    await dragBetween(
        page,
        { x: box.x + 60, y: box.y + box.height - 60 },
        { x: box.x + 220, y: box.y + box.height - 60 },
    );
    await page.waitForFunction(
        (before) => window.__uguruguBrushCount?.() > before,
        drawn,
        { timeout: 20000 },
    );
    check(true, "drawing still works after a failed open");
    await context.close();
}

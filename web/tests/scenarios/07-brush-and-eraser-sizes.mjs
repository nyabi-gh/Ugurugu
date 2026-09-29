// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

import {
    check,
    waitForDocumentLoaded,
} from "../harness.mjs";

// brush and eraser keep their own size, and a preset default above
// the old 64 ceiling is reachable.
export default async function run({ browser, origin }) {
    const context = await browser.newContext();
    const page = await context.newPage();
    await page.goto(origin);
    await waitForDocumentLoaded(page);

    await page.locator("#brush-preset").selectOption({ label: "Ink Pen" });
    const brushSize = await page.locator("#brush-size").inputValue();
    await page.locator("#tool-eraser").click();
    await page.locator("#eraser-preset").selectOption({ label: "Soft" });
    const eraserSize = await page.locator("#eraser-size").inputValue();
    check(
        eraserSize !== brushSize,
        `the eraser has its own size (brush ${brushSize}, eraser ${eraserSize})`,
    );
    await page.locator("#tool-brush").click();
    check(
        (await page.locator("#brush-size").inputValue()) === brushSize,
        "picking an eraser preset leaves the brush size alone",
    );

    await page
        .locator("#brush-preset")
        .selectOption({ label: "Droplet Spray" });
    check(
        (await page.locator("#brush-size").inputValue()) === "72",
        "a preset default above the old 64 ceiling is reachable",
    );
    await context.close();
}

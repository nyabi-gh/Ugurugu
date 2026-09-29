// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

import { readFile, writeFile } from "node:fs/promises";
import {
    check,
    scratchFile,
    screenPixel,
    waitForDocumentLoaded,
} from "../harness.mjs";

// a translucent document has to reach the screen as straight
// alpha. Blending into the premultipliedAlpha: false drawing buffer used to
// square the alpha, which turned a half-opaque layer nearly black.
export default async function run({ browser, origin }) {
    const context = await browser.newContext({ acceptDownloads: true });
    const page = await context.newPage();
    await page.goto(origin);
    await waitForDocumentLoaded(page);

    const download = page.waitForEvent("download");
    await page.locator("#save-document").click();
    const savedPath = scratchFile("saved.ugu");
    await (await download).saveAs(savedPath);
    const saved = JSON.parse(await readFile(savedPath, "utf8"));
    saved.canvas.background = "#00ffffff";
    const transparentPath = scratchFile("transparent.ugu");
    await writeFile(transparentPath, JSON.stringify(saved));

    await page.locator('#open-document').setInputFiles(transparentPath);
    await page.waitForFunction(
        () =>
            document
                .querySelector("#status")
                ?.textContent.includes("transparent"),
        undefined,
        { timeout: 30000 },
    );

    await page.evaluate(() => {
        const input = document.querySelector('input[type="color"]');
        input.value = "#ff0000";
        input.dispatchEvent(new Event("input", { bubbles: true }));
    });
    await page.locator("#tool-bucket").click();
    const box = await page.locator("#display-canvas").boundingBox();
    const middle = {
        x: Math.round(box.x + box.width / 2),
        y: Math.round(box.y + box.height / 2),
    };
    await page.mouse.click(middle.x, middle.y);
    await page.evaluate(() => {
        const slider = document.querySelector("#layer-opacity");
        slider.value = "50";
        slider.dispatchEvent(new Event("change", { bubbles: true }));
    });
    await page.waitForFunction(
        () => {
            const canvas = document.querySelector("#document-surface");
            const pixel = canvas
                .getContext("2d")
                .getImageData(
                    Math.floor(canvas.width / 2),
                    Math.floor(canvas.height / 2),
                    1,
                    1,
                ).data;
            return pixel[0] === 255 && pixel[3] > 100 && pixel[3] < 155;
        },
        undefined,
        { timeout: 30000 },
    );
    check(true, "a half-opaque red layer reaches the document surface");

    const shown = await screenPixel(page, middle.x, middle.y);
    // Straight alpha over the dark viewport backdrop lands near 145; the
    // squared-alpha bug put it near 56.
    check(
        shown[0] > 120 && shown[0] < 175,
        `the screen shows the layer at its real opacity (red ${shown[0]})`,
    );
    await context.close();
}

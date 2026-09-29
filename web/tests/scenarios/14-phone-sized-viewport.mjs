// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

import {
    check,
    countBrushPixels,
    drawStroke,
    waitForDocumentLoaded,
} from "../harness.mjs";

// a phone-sized window gives the canvas the screen and reaches every panel
// through the dock. The columns the desktop layout puts beside the canvas are
// wider than a phone on their own; laid out that way the canvas came out zero
// wide, and once it was wide enough the bars still ran off the edge.
export default async function run({ browser, origin }) {
    const context = await browser.newContext({
        viewport: { width: 390, height: 844 },
    });
    const page = await context.newPage();
    await page.goto(origin);
    await waitForDocumentLoaded(page);

    const box = await page.locator("#display-canvas").boundingBox();
    check(
        box.width >= 200 && box.height >= 200,
        `phone-sized canvas is ${Math.round(box.width)}x` +
            `${Math.round(box.height)}`,
    );
    const room = await page.evaluate(() => {
        const canvas = document
            .querySelector("#display-canvas")
            .getBoundingClientRect();
        const dock = document.querySelector(".dock").getBoundingClientRect();
        return {
            share: canvas.height / document.documentElement.clientHeight,
            clearsDock: Math.round(canvas.bottom) <= Math.round(dock.top),
        };
    });
    check(
        room.share >= 0.8 && room.clearsDock,
        `the canvas takes ${Math.round(room.share * 100)} percent of the ` +
            `height and stops above the dock`,
    );

    await drawStroke(page);
    const drawn = await countBrushPixels(page);
    check(drawn > 0, `a phone-sized canvas takes a stroke (${drawn} px)`);

    const offscreen = await page.evaluate(() => {
        const width = document.documentElement.clientWidth;
        const height = document.documentElement.clientHeight;
        const out = [];
        for (const el of document.querySelectorAll(
            ".dock button, .floating button",
        )) {
            const rect = el.getBoundingClientRect();
            if (
                rect.right > width ||
                rect.left < 0 ||
                rect.bottom > height ||
                rect.top < 0
            ) {
                out.push(el.id || el.textContent.trim().slice(0, 16));
            }
        }
        return out;
    });
    check(
        offscreen.length === 0,
        `every dock and floating control fits the phone${
            offscreen.length ? ` (off-screen: ${offscreen.join(", ")})` : ""
        }`,
    );

    // The licence notice is the one control the shell cannot afford to lose:
    // with Qt linked statically this is the only route to the licence and the
    // source. On a phone it lives behind the file menu.
    await page.locator("#file-menu").click();
    await page.locator("#notices").click();
    await page.waitForSelector("#notices-close", { timeout: 15000 });
    check(true, "the file sheet reaches the licence notices");
    await page.locator("#notices-close").click();

    for (const [button, control] of [
        ["#dock-layers", "#layer-add"],
        ["#dock-wobble", "#wobble-amount"],
        ["#dock-frames", "#frames-per-second"],
        ["#dock-tool", "#brush-size"],
    ]) {
        await page.locator(button).click();
        await page.waitForSelector(control, {
            state: "visible",
            timeout: 15000,
        });
        await page.locator(button).click();
    }
    check(true, "the dock raises the layer, wobble, frame and tool sheets");

    await context.close();
}

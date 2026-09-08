// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

import {
    brushColor,
    check,
    countBrushPixels,
    hasBrushPixel,
    installPixelCounter,
    waitForDocumentLoaded,
} from "../harness.mjs";

// two touch points translate, pinch, and twist as one transform.
// The document point under the old midpoint must follow the new midpoint.
export default async function run({ browser, origin }) {
    const context = await browser.newContext({
        viewport: { width: 1180, height: 820 },
        hasTouch: true,
    });
    const page = await context.newPage();
    await installPixelCounter(page);
    await page.goto(origin);
    await waitForDocumentLoaded(page);
    await page.locator("#new-document").click();
    await page.locator("#new-document-width").fill("320");
    await page.locator("#new-document-height").fill("160");
    await page.locator("#new-document-confirm").click();
    await page.waitForFunction(
        () => document.querySelector("#document-surface")?.width === 320,
        undefined,
        { timeout: 30000 },
    );
    await page.locator("#tool-lasso").click();

    const canvasBox = await page.locator("#display-canvas").boundingBox();
    const oldCenter = {
        x: canvasBox.x + canvasBox.width / 2,
        y: canvasBox.y + canvasBox.height / 2,
    };
    const nextCenter = { x: oldCenter.x + 40, y: oldCenter.y + 25 };
    const radians = Math.PI / 6;
    const axis = { x: Math.cos(radians) * 70, y: Math.sin(radians) * 70 };
    const session = await context.newCDPSession(page);
    const point = (x, y, id) => ({
        x,
        y,
        id,
        radiusX: 1,
        radiusY: 1,
        force: 1,
    });
    await session.send("Input.dispatchTouchEvent", {
        type: "touchStart",
        touchPoints: [
            point(oldCenter.x - 50, oldCenter.y, 1),
            point(oldCenter.x + 50, oldCenter.y, 2),
        ],
    });
    await session.send("Input.dispatchTouchEvent", {
        type: "touchMove",
        touchPoints: [
            point(nextCenter.x - axis.x, nextCenter.y - axis.y, 1),
            point(nextCenter.x + axis.x, nextCenter.y + axis.y, 2),
        ],
    });
    await session.send("Input.dispatchTouchEvent", {
        type: "touchEnd",
        touchPoints: [],
    });

    check(
        (await page.locator("#rotation-angle").inputValue()) === "30",
        "a two-finger twist rotates the canvas by 30 degrees",
    );
    check(
        (await page.locator("#zoom-fit").textContent()) === "140%",
        "the same two-finger gesture pinches to 140 percent",
    );
    check(
        (await countBrushPixels(page)) === 0,
        "the two-finger gesture does not paint into the document",
    );

    await page.locator("#tool-brush").click();
    await page.mouse.click(nextCenter.x, nextCenter.y);
    await page.waitForFunction(hasBrushPixel, undefined, { timeout: 15000 });
    const inkCenter = await page.evaluate((color) => {
        const canvas = document.querySelector("#document-surface");
        const data = canvas
            .getContext("2d")
            .getImageData(0, 0, canvas.width, canvas.height).data;
        let totalX = 0;
        let totalY = 0;
        let count = 0;
        for (let y = 0; y < canvas.height; y += 1) {
            for (let x = 0; x < canvas.width; x += 1) {
                const index = (y * canvas.width + x) * 4;
                if (
                    data[index] === color.red &&
                    data[index + 1] === color.green &&
                    data[index + 2] === color.blue
                ) {
                    totalX += x;
                    totalY += y;
                    count += 1;
                }
            }
        }
        return { x: totalX / count, y: totalY / count, count };
    }, brushColor);
    check(
        inkCenter.count > 0 &&
            Math.abs(inkCenter.x - 160) < 8 &&
            Math.abs(inkCenter.y - 80) < 8,
        `the touch midpoint keeps document ${inkCenter.x.toFixed(1)},` +
            `${inkCenter.y.toFixed(1)} anchored while it moves`,
    );

    // The check above starts both fingers in one event, which no hand does.
    // Staggering them is what left a dot at the first finger on every pan.
    const beforePinch = await countBrushPixels(page);
    await session.send("Input.dispatchTouchEvent", {
        type: "touchStart",
        touchPoints: [point(oldCenter.x - 30, oldCenter.y, 3)],
    });
    await page.waitForTimeout(80);
    await session.send("Input.dispatchTouchEvent", {
        type: "touchStart",
        touchPoints: [
            point(oldCenter.x - 30, oldCenter.y, 3),
            point(oldCenter.x + 30, oldCenter.y, 4),
        ],
    });
    await session.send("Input.dispatchTouchEvent", {
        type: "touchMove",
        touchPoints: [
            point(oldCenter.x - 80, oldCenter.y, 3),
            point(oldCenter.x + 80, oldCenter.y, 4),
        ],
    });
    await session.send("Input.dispatchTouchEvent", {
        type: "touchEnd",
        touchPoints: [],
    });
    await page.waitForTimeout(600);
    const afterPinch = await countBrushPixels(page);
    check(
        afterPinch === beforePinch,
        `a pinch whose fingers land apart in time paints nothing ` +
            `(${beforePinch} to ${afterPinch} px)`,
    );

    await session.send("Input.dispatchTouchEvent", {
        type: "touchStart",
        touchPoints: [point(oldCenter.x - 60, oldCenter.y + 60, 5)],
    });
    await session.send("Input.dispatchTouchEvent", {
        type: "touchMove",
        touchPoints: [point(oldCenter.x + 20, oldCenter.y + 60, 5)],
    });
    await session.send("Input.dispatchTouchEvent", {
        type: "touchEnd",
        touchPoints: [],
    });
    await page.waitForFunction(
        (baseline) => window.__uguruguBrushCount() > baseline,
        afterPinch,
        { timeout: 15000 },
    );
    check(true, "one finger dragged past the slop still draws");

    // A twist arriving a degree at a time is a hand failing to hold an angle,
    // not a request to rotate.
    await page.locator("#rotation-reset").click();
    const twist = async (degrees) => {
        const radians = (degrees * Math.PI) / 180;
        await session.send("Input.dispatchTouchEvent", {
            type: "touchMove",
            touchPoints: [
                point(
                    oldCenter.x - Math.cos(radians) * 60,
                    oldCenter.y - Math.sin(radians) * 60,
                    6,
                ),
                point(
                    oldCenter.x + Math.cos(radians) * 60,
                    oldCenter.y + Math.sin(radians) * 60,
                    7,
                ),
            ],
        });
    };
    await session.send("Input.dispatchTouchEvent", {
        type: "touchStart",
        touchPoints: [
            point(oldCenter.x - 60, oldCenter.y, 6),
            point(oldCenter.x + 60, oldCenter.y, 7),
        ],
    });
    for (let degrees = 1; degrees <= 3; degrees += 1) {
        await twist(degrees);
    }
    const heldStill = await page.locator("#rotation-angle").inputValue();
    check(heldStill === "0", `a 3 degree wobble leaves the canvas at 0`);

    for (let degrees = 4; degrees <= 20; degrees += 1) {
        await twist(degrees);
    }
    await session.send("Input.dispatchTouchEvent", {
        type: "touchEnd",
        touchPoints: [],
    });
    const engaged = Number(
        await page.locator("#rotation-angle").inputValue(),
    );
    check(
        engaged >= 13 && engaged <= 16,
        `twisting past the slop rotates by the twist less the slop ` +
            `(${engaged} degrees of 20)`,
    );
    await context.close();
}

// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyattic)

import { readFile } from "node:fs/promises";
import { join } from "node:path";
import { tmpdir } from "node:os";
import {
    check,
    countBrushPixels,
    installPixelCounter,
    waitForDocumentLoaded,
} from "../harness.mjs";

export default async function run({ browser, origin }) {
    const context = await browser.newContext({
        viewport: { width: 1440, height: 1000 },
    });
    const page = await context.newPage();
    await installPixelCounter(page);
    await page.goto(origin);
    await waitForDocumentLoaded(page);
    const pixelDigest = () =>
        page
            .locator("#document-surface")
            .evaluate((canvas) => canvas.toDataURL());
    const save = async () => {
        const waiting = page.waitForEvent("download");
        await page.locator("#save-document").click();
        const file = await waiting;
        const bytes = await readFile(await file.path());
        return { bytes, json: JSON.parse(bytes) };
    };

    await page.locator("#wobble-scope").selectOption("layer");
    await page.locator("#wobble-amount").fill("0");
    await page.locator("#wobble-amount").dispatchEvent("change");
    await page.waitForFunction(
        () => !document.querySelector("#wobble-follow").disabled,
    );
    let saved = await save();
    check(
        saved.json.layers[0].wobble === 0 &&
            saved.json.animation.wobble === 1.6,
        "layer wobble overrides only the selected layer",
    );
    await page.locator("#layer-add").click();
    await page.waitForFunction(
        () => document.querySelectorAll("li .name").length === 2,
    );
    await page.waitForFunction(
        () => document.querySelector("#wobble-amount").value === "1.6",
    );
    check(
        await page.locator("#wobble-follow").isDisabled(),
        "a new layer follows drawing settings",
    );
    await page.locator("li .name").filter({ hasText: "Layer 1" }).click();
    await page.waitForFunction(
        () => document.querySelector("#wobble-amount").value === "0",
    );
    await page.locator("#wobble-follow").click();
    await page.waitForFunction(
        () => document.querySelector("#wobble-follow").disabled,
    );
    await page.locator("#undo").click();
    await page.waitForFunction(
        () => document.querySelector("#wobble-amount").value === "0",
    );
    check(
        true,
        "following the drawing can be undone to restore the layer override",
    );

    const blank = await pixelDigest();
    const canvasWidth = (await page.locator("#display-canvas").boundingBox())
        .width;
    await page.locator("#tool-text").click();
    check(
        (await page.locator("#display-canvas").boundingBox()).width ===
            canvasWidth,
        "switching to text preserves the canvas viewport",
    );
    await page.locator("#text-content").fill("Ug 한글\n日本語");
    await page.locator("#text-size").fill("52");
    await page.locator("#text-filled").check();
    await page.waitForFunction(
        () => !document.querySelector("#text-apply").disabled,
    );
    if (process.argv.includes("--screenshots")) {
        await page.screenshot({
            path: join(tmpdir(), "ugurugu-web-text-desktop.png"),
        });
    }
    check(
        (await pixelDigest()) === blank,
        "the text preview leaves the committed picture unchanged",
    );
    // Apply then Save within one event turn: serialization must wait for text.
    const textDownload = page.waitForEvent("download");
    await page.evaluate(() => {
        document.querySelector("#text-apply").click();
        document.querySelector("#save-document").click();
    });
    const textFile = await textDownload;
    const textJson = JSON.parse(await readFile(await textFile.path()));
    check(
        textJson.layers[0].strokes.length > 5,
        "multilingual text is saved as ordinary strokes even on immediate Save",
    );
    await page.waitForFunction(() => window.__uguruguBrushCount() > 100);
    const drawn = await pixelDigest();
    check((await countBrushPixels(page)) > 100, "text produces visible pixels");
    await page.locator("#undo").click();
    await page.waitForFunction(() => window.__uguruguBrushCount() === 0);
    check(
        (await pixelDigest()) === blank,
        "one undo removes the complete text including its fill",
    );
    await page.locator("#redo").click();
    await page.waitForFunction(() => window.__uguruguBrushCount() > 100);
    check(
        (await pixelDigest()) === drawn,
        "redo restores the same text pixels",
    );

    // The image is half transparent red, with a transparent right half.
    const imageUrl = await page.evaluate(() => {
        const canvas = document.createElement("canvas");
        canvas.width = 64;
        canvas.height = 32;
        const ctx = canvas.getContext("2d");
        ctx.fillStyle = "rgba(255,0,0,0.5)";
        ctx.fillRect(0, 0, 32, 32);
        return canvas.toDataURL("image/png");
    });
    const png = Buffer.from(imageUrl.split(",")[1], "base64");
    await page
        .locator("#import-image")
        .setInputFiles({
            name: "Reference.png",
            mimeType: "image/png",
            buffer: png,
        });
    await page.waitForFunction(
        () => document.querySelectorAll("li .name").length === 3,
    );
    const withImage = await pixelDigest();
    check(
        withImage !== drawn,
        "image import adds visible pixels on a new layer",
    );
    const colors = await page
        .locator("#document-surface")
        .evaluate((canvas) => {
            const ctx = canvas.getContext("2d");
            return [
                Array.from(ctx.getImageData(490, 384, 1, 1).data),
                Array.from(ctx.getImageData(530, 384, 1, 1).data),
            ];
        });
    check(
        colors[0][0] === 255 &&
            colors[0][1] >= 126 &&
            colors[0][1] <= 129 &&
            colors[1][1] === 255,
        "the centered image preserves translucent and transparent pixels",
    );
    saved = await save();
    const imageLayer = saved.json.layers.find(
        (layer) => layer.name === "Reference",
    );
    check(
        imageLayer?.strokes[0].mode === "image",
        "the original image is retained as an image operation",
    );
    await page.locator("#undo").click();
    await page.waitForFunction(
        () => document.querySelectorAll("li .name").length === 2,
    );
    check(
        (await pixelDigest()) === drawn,
        "one undo removes the imported image layer",
    );
    await page.locator("#redo").click();
    await page.waitForFunction(
        () => document.querySelectorAll("li .name").length === 3,
    );
    check(
        (await pixelDigest()) === withImage,
        "redo restores the image exactly",
    );
    await page
        .locator("#open-document")
        .setInputFiles({
            name: "Roundtrip.ugu",
            mimeType: "application/octet-stream",
            buffer: saved.bytes,
        });
    await page.waitForFunction(() =>
        document.body.textContent.includes("Roundtrip.ugu —"),
    );
    await page.waitForFunction(
        (expected) =>
            document.querySelector("#document-surface").toDataURL() ===
            expected,
        withImage,
    );
    check(
        (await pixelDigest()) === withImage,
        "text, layer wobble and image render identically after reopening",
    );

    const oversized = Buffer.from(png);
    oversized.writeUInt32BE(5000, 16);
    await page
        .locator("#import-image")
        .setInputFiles({
            name: "TooLarge.png",
            mimeType: "image/png",
            buffer: oversized,
        });
    await page.waitForFunction(() =>
        document.body.textContent.includes("Images can be up to"),
    );
    check(
        (await pixelDigest()) === withImage,
        "oversized images are rejected before decoding without changing the drawing",
    );
    await page
        .locator("#import-image")
        .setInputFiles({
            name: "Broken.jpg",
            mimeType: "image/jpeg",
            buffer: Buffer.from("broken"),
        });
    await page.waitForFunction(() =>
        document.body.textContent.includes("Choose a valid PNG"),
    );
    check(
        (await pixelDigest()) === withImage,
        "corrupt images leave the drawing intact",
    );
    await page.locator("#tool-text").click();
    await page.locator("#text-content").fill(" ");
    await page.waitForFunction(
        () => document.querySelector("#text-apply").disabled,
    );
    check(true, "empty text cannot create a history entry");
    await page.locator("#text-cancel").click();
    check(
        (await pixelDigest()) === withImage,
        "canceling text preserves the document",
    );
    // Exercise browser decoders as well as the transparent PNG path.
    for (const format of ["jpeg", "webp"]) {
        const url = await page.evaluate((format) => {
            const canvas = document.createElement("canvas");
            canvas.width = 20;
            canvas.height = 10;
            canvas.getContext("2d").fillRect(0, 0, 20, 10);
            return canvas.toDataURL(`image/${format}`);
        }, format);
        const before = await page.locator("li .name").count();
        await page
            .locator("#import-image")
            .setInputFiles({
                name: `Sample.${format}`,
                mimeType: `image/${format}`,
                buffer: Buffer.from(url.split(",")[1], "base64"),
            });
        await page.waitForFunction(
            (count) =>
                document.querySelectorAll("li .name").length === count + 1,
            before,
        );
        check(true, `${format} imports through the browser decoder`);
    }
    await context.close();

    const mobile = await browser.newContext({
        viewport: { width: 390, height: 844 },
        hasTouch: true,
        isMobile: true,
    });
    const phone = await mobile.newPage();
    await installPixelCounter(phone);
    await phone.goto(origin);
    await waitForDocumentLoaded(phone);
    await phone.locator("#dock-tool").click();
    await phone.locator("#tool-text").click();
    await phone.locator("#text-content").fill("Mobile");
    await phone.waitForFunction(
        () => !document.querySelector("#text-apply").disabled,
    );
    check(
        await phone.locator("#text-apply").evaluate((button) => {
            const bounds = button.getBoundingClientRect();
            const dock = document.querySelector(".dock").getBoundingClientRect();
            return bounds.top >= 0 && bounds.bottom <= dock.top &&
                document.elementFromPoint(
                    bounds.left + bounds.width / 2,
                    bounds.top + bounds.height / 2,
                ) === button;
        }),
        "the phone text apply button is reachable without scrolling",
    );
    if (process.argv.includes("--screenshots")) {
        await phone.screenshot({
            path: join(tmpdir(), "ugurugu-web-text-phone.png"),
        });
    }
    await phone.locator("#text-apply").click();
    await phone.waitForFunction(() => window.__uguruguBrushCount() > 100);
    check(true, "the phone tool sheet can apply text");
    await phone.locator("#dock-wobble").click();
    await phone.locator("#wobble-scope").selectOption("layer");
    await phone.locator("#wobble-amount").fill("0");
    await phone.locator("#wobble-amount").dispatchEvent("change");
    await phone.waitForFunction(
        () => !document.querySelector("#wobble-follow").disabled,
    );
    check(true, "the phone wobble sheet edits the active layer");
    await phone.locator("#dock-wobble").click();
    await phone.locator("#file-menu").click();
    const choosing = phone.waitForEvent("filechooser");
    await phone.locator("#import-image-button").click();
    const chooser = await choosing;
    check(
        (await phone.locator("#import-image").count()) === 1,
        "the file input survives closing the mobile menu",
    );
    await chooser.setFiles({
        name: "Phone.png",
        mimeType: "image/png",
        buffer: png,
    });
    await phone.waitForFunction(() =>
        document.body.textContent.includes("Phone.png added as a new layer"),
    );
    check(
        true,
        "the phone file menu imports an image after the picker returns",
    );
    await mobile.close();
}

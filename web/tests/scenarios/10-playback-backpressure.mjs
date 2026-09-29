// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

import {
    check,
    installWorkerDelay,
    isPlaying,
    waitForDocumentLoaded,
} from "../harness.mjs";

// playback must not queue renders it cannot keep up with. The
// worker is delayed here to stand in for a document whose full render costs
// more than one frame interval.
export default async function run({ browser, origin }) {
    const context = await browser.newContext();
    const page = await context.newPage();
    await installWorkerDelay(page);
    await page.goto(origin);
    await waitForDocumentLoaded(page);
    await page.evaluate(() => {
        window.__slowEngine = 250;
    });
    await page.locator(".play").click();
    check(await isPlaying(page), "playback started against a slow engine");
    await page.waitForTimeout(4000);
    await page.locator(".play").click();
    const stoppedAt = Date.now();
    await page.waitForFunction(
        () => window.__sent === window.__received,
        undefined,
        { timeout: 120000 },
    );
    const drain = Date.now() - stoppedAt;
    check(
        drain < 3000,
        `stopping playback leaves no render backlog (drained in ${drain} ms)`,
    );
    await context.close();
}

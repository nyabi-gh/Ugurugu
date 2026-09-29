// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

import {
    check,
    drainWorker,
    inkColumnRuns,
    installWorkerDelay,
    quickStroke,
    waitForDocumentLoaded,
} from "../harness.mjs";

// two strokes drawn back to back, faster than the engine answers,
// have to stay two strokes. The point batch used to be read when the queued
// append finally ran rather than when the pointer produced it, so a second
// stroke starting first either emptied the buffer under the open stroke or
// handed it its own points.
export default async function run({ browser, origin }) {
    const context = await browser.newContext();
    const page = await context.newPage();
    await installWorkerDelay(page);
    await page.goto(origin);
    await waitForDocumentLoaded(page);
    await page.evaluate(() => {
        window.__slowEngine = 400;
    });

    const box = await page.locator("#display-canvas").boundingBox();
    await quickStroke(page, box, 0.1, 0.35, 0.5);
    await quickStroke(page, box, 0.65, 0.9, 0.5);
    await drainWorker(page);
    await page.waitForFunction(
        () => document.querySelector("#undo")?.disabled === false,
        undefined,
        { timeout: 30000 },
    );

    const runs = await inkColumnRuns(page);
    check(
        runs.length === 2,
        `two quick strokes stay two strokes (${runs.length} ink run(s))`,
    );
    const widths = runs.map((run) => run.end - run.start);
    check(
        widths.every((width) => width > 20),
        `neither quick stroke lost its tail (widths ${widths.join(", ")})`,
    );
    await context.close();
}

// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

import {
    check,
    startServer,
} from "../harness.mjs";

// an engine artifact that never arrives has to say so.
export default async function run({ browser, origin }) {
    const blocked = await startServer({ withoutEngine: true });
    const context = await browser.newContext();
    const page = await context.newPage();
    await page.goto(blocked.origin);
    await page.waitForFunction(
        () =>
            /failed|could not/i.test(
                document.querySelector("#status")?.textContent ?? "",
            ),
        undefined,
        { timeout: 30000 },
    );
    check(true, "a missing engine artifact surfaces instead of hanging");
    await context.close();
    blocked.server.close();
}

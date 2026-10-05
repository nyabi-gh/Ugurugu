// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

import {
    check,
    drawStroke,
    eventually,
    recoveryRecords,
    seedRecoveryRecord,
    waitForDocumentLoaded,
} from "../harness.mjs";

async function waitForRecord(page, predicate, timeout = 20000) {
    const deadline = Date.now() + timeout;
    while (Date.now() < deadline) {
        const records = await recoveryRecords(page);
        if (records.some(predicate)) {
            return records;
        }
        await page.waitForTimeout(250);
    }
    return recoveryRecords(page);
}

// Recovery is offered work the artist has not decided about yet, and it
// belongs to the session that wrote it. Neither a new session's own
// snapshots, a failed restore, a discard of something else, nor a second tab
// may take it away.
export default async function run({ browser, origin }) {
    {
        const context = await browser.newContext();
        const page = await context.newPage();
        await page.goto(`${origin}/?autosave=1`);
        await waitForDocumentLoaded(page);
        await drawStroke(page);
        const [first] = await waitForRecord(page, () => true);

        await page.reload();
        await waitForDocumentLoaded(page);
        check(
            await eventually(page, () => !!document.querySelector("#recovery-restore")),
            "the earlier session is offered after a reload",
        );
        await drawStroke(page);
        const records = await waitForRecord(
            page,
            (record) => record.savedAt > (first?.savedAt ?? 0),
        );
        const second = records.find((record) => record.savedAt > first.savedAt);
        check(
            records.some((record) => record.savedAt === first.savedAt),
            "an unanswered offer is not overwritten by the new session's snapshot",
        );

        await page.locator("#recovery-discard").click();
        await page.waitForTimeout(500);
        const afterDiscard = await recoveryRecords(page);
        check(
            !afterDiscard.some((record) => record.savedAt === first.savedAt),
            "discard deletes the offered record",
        );
        check(
            !!second && afterDiscard.some((record) => record.savedAt === second.savedAt),
            "discard leaves this session's own snapshot alone",
        );
        await context.close();
    }

    {
        const context = await browser.newContext();
        const page = await context.newPage();
        await page.goto(`${origin}/?autosave=1`);
        await waitForDocumentLoaded(page);
        await seedRecoveryRecord(page, "slot", {
            name: "Broken.ugu",
            bytes: [1, 2, 3, 4],
            savedAt: Date.now(),
        });
        await page.reload();
        await page.locator("#recovery-restore").waitFor({ timeout: 30000 });
        await page.locator("#recovery-restore").click();
        check(
            await eventually(page, () =>
                /Open failed/.test(document.querySelector("#status")?.textContent ?? ""),
            ),
            "a broken recovery record fails to open by name",
        );
        check(
            (await page.locator("#recovery-restore").count()) === 1,
            "a failed restore keeps the offer",
        );
        check(
            (await recoveryRecords(page)).some((record) => record.name === "Broken.ugu"),
            "a failed restore keeps the record",
        );
        await context.close();
    }

    {
        const context = await browser.newContext();
        const first = await context.newPage();
        await first.goto(`${origin}/?autosave=1`);
        await waitForDocumentLoaded(first);
        await drawStroke(first);
        const [firstRecord] = await waitForRecord(first, () => true);

        const second = await context.newPage();
        await second.goto(`${origin}/?autosave=1`);
        await waitForDocumentLoaded(second);
        await second.waitForTimeout(2000);
        check(
            (await second.locator("#recovery-restore").count()) === 0,
            "a second tab does not offer the first tab's live document",
        );
        await drawStroke(second);
        const records = await waitForRecord(
            second,
            (record) => record.savedAt > (firstRecord?.savedAt ?? 0),
        );
        check(
            records.some((record) => record.savedAt === firstRecord?.savedAt) &&
                records.length >= 2,
            "a second tab's snapshot does not replace the first tab's",
        );
        await context.close();
    }
}

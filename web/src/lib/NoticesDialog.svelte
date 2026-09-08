<!--
SPDX-License-Identifier: GPL-3.0-or-later
Copyright (C) 2026 Nyabi (nyabi-gh)
-->
<script lang="ts">
    // The licence notice the browser build owes its users. The desktop
    // packages install THIRD_PARTY_NOTICES.md and the licence texts next to
    // the binary; on the web the upload is the whole distribution, so the
    // notice has to be reachable from inside the app itself.
    import type { DocumentMeta } from "./EngineClient";

    interface Props {
        meta: DocumentMeta | null;
        onclose: () => void;
    }

    const { meta, onclose }: Props = $props();

    let dialog: HTMLDivElement | undefined = $state();

    $effect(() => {
        // Focus has to enter the dialog, or Escape and Tab stay with whatever
        // was behind it.
        dialog?.querySelector("button")?.focus();
    });

    const sourceUrl = "https://github.com/nyabi-gh/Ugurugu";
    const qtObligationsUrl =
        "https://www.qt.io/licensing/open-source-lgpl-obligations";

    // Resolved against the page rather than the origin: itch.io serves the
    // build from a per-project CDN subdirectory, where an absolute path would
    // leave the project entirely.
    function licenseHref(name: string): string {
        return new URL(`licenses/${name}`, document.baseURI).href;
    }

    const notices = [
        {
            title: "OpenType.js 2.0.0",
            body: "Copyright © Frederik De Bleser and contributors. Text outlines are read using OpenType.js under the MIT License.",
            file: "OpenType-LICENSE.txt",
            fileLabel: "OpenType.js MIT text",
        },
        {
            title: "Ugurugu",
            body:
                "Copyright © 2026 Nyabi (nyabi-gh). Licensed under the GNU " +
                "General Public License, version 3 or later. The app icon " +
                "artwork is copyright © seuppi and is distributed under the " +
                "same licence.",
            file: "GPL-3.0.txt",
            fileLabel: "GPL-3.0 text",
        },
        {
            title: "Qt 6.11.1 (Core and Gui)",
            body:
                "Copyright © The Qt Company Ltd. and other contributors. " +
                "Used under the GNU Lesser General Public License, version " +
                "3. This build links Qt statically into the engine, so the " +
                "relinking right is served by the complete source above: " +
                "BUILDING.md names the Qt and Emscripten versions and the " +
                "wasm-release preset that rebuilds the engine against a " +
                "modified Qt of the same version.",
            file: "LGPL-3.0.txt",
            fileLabel: "LGPL-3.0 text",
            link: qtObligationsUrl,
            linkLabel: "Qt LGPL obligations",
        },
        {
            title: "Pretendard JP",
            body:
                "Copyright © 2021 Kil Hyung-jin. Licensed under the SIL Open " +
                "Font License 1.1.",
            file: "Pretendard-OFL.txt",
            fileLabel: "OFL 1.1 text",
        },
        {
            title: "Svelte 5",
            body:
                "Copyright © 2016–2025 Svelte Contributors. Licensed under " +
                "the MIT License.",
            file: "Svelte-LICENSE.txt",
            fileLabel: "MIT text",
        },
        {
            title: "Emscripten 4.0.7",
            body:
                "Copyright © 2010–2014 Emscripten authors. Available under " +
                "both the MIT License and the University of Illinois/NCSA " +
                "Open Source License.",
            file: "Emscripten-LICENSE.txt",
            fileLabel: "MIT and NCSA text",
        },
    ];

    function onKeyDown(event: KeyboardEvent) {
        if (event.key === "Escape") {
            event.stopPropagation();
            onclose();
        }
    }
</script>

<div class="backdrop" role="presentation" onkeydown={onKeyDown}>
    <div
        class="dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby="notices-title"
        bind:this={dialog}
    >
        <h2 id="notices-title">About Ugurugu</h2>

        <p class="lead">
            A drawing app for wobbling lines. This is the browser build; it is
            free software and you may study, share and change it.
        </p>

        <p class="build">
            <!--
              Named in the notice on purpose: a bug report from a published
              build is far easier to place when the reporter can read these
              two numbers off the screen.
            -->
            {#if meta}
                Engine ABI {meta.abiVersion} · document schema v{meta.schemaVersion}
            {:else}
                Engine not loaded
            {/if}
        </p>

        <p class="source">
            Complete source code:
            <a
                id="notices-source"
                href={sourceUrl}
                target="_blank"
                rel="noopener noreferrer">{sourceUrl}</a
            >
        </p>

        <ul class="notices">
            {#each notices as notice (notice.title)}
                <li>
                    <h3>{notice.title}</h3>
                    <p>{notice.body}</p>
                    <p class="links">
                        <a
                            href={licenseHref(notice.file)}
                            target="_blank"
                            rel="noopener noreferrer">{notice.fileLabel}</a
                        >
                        {#if notice.link}
                            <a
                                href={notice.link}
                                target="_blank"
                                rel="noopener noreferrer">{notice.linkLabel}</a
                            >
                        {/if}
                    </p>
                </li>
            {/each}
        </ul>

        <div class="actions">
            <a
                class="all-notices"
                href={licenseHref("THIRD_PARTY_NOTICES.txt")}
                target="_blank"
                rel="noopener noreferrer">All third-party notices</a
            >
            <button id="notices-close" type="button" onclick={onclose}>
                Close
            </button>
        </div>
    </div>
</div>

<style>
    .backdrop {
        position: fixed;
        inset: 0;
        display: grid;
        place-items: center;
        padding: 1rem;
        background: rgb(0 0 0 / 55%);
        z-index: 10;
    }

    .dialog {
        display: flex;
        flex-direction: column;
        gap: 0.6rem;
        inline-size: min(34rem, 100%);
        max-block-size: min(36rem, 100%);
        overflow-y: auto;
        padding: 1.25rem;
        border-radius: 0.5rem;
        background: #26292f;
        box-shadow: 0 1rem 2rem rgb(0 0 0 / 45%);
    }

    h2 {
        margin: 0;
        font-size: 1rem;
    }

    h3 {
        margin: 0 0 0.15rem;
        font-size: 0.8125rem;
    }

    p {
        margin: 0;
        font-size: 0.8125rem;
        line-height: 1.45;
    }

    .lead {
        color: #c8ccd2;
    }

    .build,
    .source {
        color: #9aa0aa;
        font-size: 0.75rem;
    }

    .notices {
        display: flex;
        flex-direction: column;
        gap: 0.7rem;
        margin: 0.2rem 0 0;
        padding: 0.7rem 0 0;
        border-block-start: 1px solid #3f434b;
        list-style: none;
    }

    .notices p {
        color: #9aa0aa;
        font-size: 0.75rem;
    }

    .links {
        display: flex;
        flex-wrap: wrap;
        gap: 0.75rem;
        margin-block-start: 0.2rem;
    }

    a {
        color: #ffc94a;
    }

    .actions {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 0.5rem;
        padding-block-start: 0.7rem;
        border-block-start: 1px solid #3f434b;
    }

    .all-notices {
        font-size: 0.75rem;
    }

    button {
        padding: 0.35rem 0.9rem;
        border: 1px solid #43474f;
        border-radius: 0.25rem;
        background: #31353d;
        color: inherit;
        cursor: pointer;
    }
</style>

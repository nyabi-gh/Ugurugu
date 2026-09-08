<!--
SPDX-License-Identifier: GPL-3.0-or-later
Copyright (C) 2026 Nyabi (nyabi-gh)
-->
<script lang="ts">
    import type { Snippet } from "svelte";

    interface Props {
        title: string;
        // Set when the content carries its own heading, so the sheet keeps the
        // accessible name without printing it twice.
        selfTitled?: boolean;
        onclose: () => void;
        children: Snippet;
    }

    const { title, selfTitled = false, onclose, children }: Props = $props();

    let panel: HTMLDivElement | undefined = $state();

    $effect(() => {
        panel
            ?.querySelector<HTMLElement>(
                "button, input, select, [tabindex]:not([tabindex='-1'])",
            )
            ?.focus();
    });

    function onKeyDown(event: KeyboardEvent) {
        if (event.key === "Escape") {
            event.stopPropagation();
            onclose();
        }
    }
</script>

<svelte:window onkeydown={onKeyDown} />

<div class="scrim" role="presentation" onclick={onclose}></div>
<div
    class="sheet"
    bind:this={panel}
    role="dialog"
    aria-modal="true"
    aria-label={title}
>
    <header class:compact={selfTitled}>
        <h2 class:visually-hidden={selfTitled}>{title}</h2>
        <button class="close" aria-label="Close {title}" onclick={onclose}>
            ✕
        </button>
    </header>
    <div class="body">
        {@render children()}
    </div>
</div>

<style>
    /* Lifted clear of whatever owns the foot of the page, so that stays usable
       while a sheet is up. */
    .scrim {
        position: fixed;
        inset: 0;
        inset-block-end: var(--sheet-lift, 0px);
        background: rgb(0 0 0 / 45%);
        z-index: 8;
    }

    .sheet {
        position: fixed;
        inset-inline: 0;
        inset-block-end: var(--sheet-lift, 0px);
        z-index: 9;
        display: flex;
        flex-direction: column;
        max-block-size: 62vh;
        border-start-start-radius: 0.9rem;
        border-start-end-radius: 0.9rem;
        border-block-start: 1px solid var(--line);
        background: var(--ink-900);
        box-shadow: 0 -0.75rem 2rem rgb(0 0 0 / 45%);
        animation: rise 160ms ease-out;
    }

    @media (prefers-reduced-motion: reduce) {
        .sheet {
            animation: none;
        }
    }

    @keyframes rise {
        from {
            transform: translateY(1.5rem);
            opacity: 0;
        }
    }

    header {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 0.75rem;
        padding: 0.6rem 0.9rem 0.4rem;
    }

    /* The hidden heading leaves the flow, so the close button has to be told
       where to sit. */
    header.compact {
        justify-content: flex-end;
        padding-block: 0.15rem 0;
    }

    .visually-hidden {
        position: absolute;
        inline-size: 1px;
        block-size: 1px;
        overflow: hidden;
        clip-path: inset(50%);
    }

    h2 {
        margin: 0;
        font-size: 0.75rem;
        font-weight: 700;
        letter-spacing: 0.14em;
        text-transform: uppercase;
        color: var(--paper-dim);
    }

    .close {
        min-inline-size: 2.5rem;
        min-block-size: 2.5rem;
        border: 0;
        background: none;
        color: var(--paper);
        font-size: 1rem;
        cursor: pointer;
    }

    .body {
        overflow-y: auto;
        overscroll-behavior: contain;
    }
</style>

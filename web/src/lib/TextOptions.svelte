<!--
SPDX-License-Identifier: GPL-3.0-or-later
Copyright (C) 2026 Nyabi (nyabi-gh)
-->
<script lang="ts">
    import type { TextDraft } from "./TextGeometry";
    let {
        draft,
        ready,
        message,
        canvasWidth,
        canvasHeight,
        onapply,
        oncancel,
    }: {
        draft: TextDraft;
        ready: boolean;
        message: string;
        canvasWidth: number;
        canvasHeight: number;
        onapply: () => void;
        oncancel: () => void;
    } = $props();
</script>

<section aria-label="Text options">
    <h2>Text</h2>
    <p>Click the canvas to position the text, then apply.</p>
    <label
        >Text <textarea
            id="text-content"
            maxlength="256"
            rows="4"
            bind:value={draft.text}></textarea></label
    >
    <p class="font">Pretendard JP</p>
    <label
        >Size (px) <input
            id="text-size"
            type="number"
            min="8"
            max="512"
            bind:value={draft.size}
        /></label
    >
    <label
        >Outline (px) <input
            id="text-width"
            type="number"
            min="1"
            max="128"
            bind:value={draft.width}
        /></label
    >
    <label class="check"
        ><input id="text-filled" type="checkbox" bind:checked={draft.filled} /> Fill
        letters</label
    >
    <div class="coordinates">
        <label
            >X <input
                id="text-x"
                type="number"
                min="0"
                max={canvasWidth}
                bind:value={draft.x}
            /></label
        >
        <label
            >Y <input
                id="text-y"
                type="number"
                min="0"
                max={canvasHeight}
                bind:value={draft.y}
            /></label
        >
    </div>
    <p role="status">{message}</p>
    <div class="actions">
        <button id="text-apply" disabled={!ready} onclick={onapply}
            >Apply text</button
        >
        <button id="text-cancel" onclick={oncancel}>Cancel</button>
    </div>
</section>

<style>
    section {
        box-sizing: content-box;
        flex: none;
        inline-size: 13.5rem;
        overflow-y: auto;
        background: var(--ink-850);
        border-inline-end: 1px solid var(--line);
        display: flex;
        flex-direction: column;
        gap: 0.6rem;
        padding: 0.9rem 0.85rem;
    }
    h2,
    p {
        margin: 0;
    }
    h2 {
        font-size: 0.9rem;
    }
    p {
        color: var(--paper-dim);
        font-size: 0.75rem;
    }
    label {
        display: flex;
        flex-direction: column;
        gap: 0.25rem;
        font-size: 0.75rem;
    }
    input,
    textarea,
    button {
        box-sizing: border-box;
        min-inline-size: 0;
        inline-size: 100%;
        min-block-size: 2.75rem;
        padding: 0.4rem;
        background: var(--ink-800);
        color: var(--paper);
        border: 1px solid var(--line);
        border-radius: 6px;
        font: inherit;
    }
    textarea {
        resize: vertical;
    }
    .check {
        flex-direction: row;
        align-items: center;
        min-block-size: 2.75rem;
    }
    .check input {
        inline-size: 1.2rem;
        min-block-size: 1.2rem;
    }
    .coordinates {
        display: grid;
        grid-template-columns: 1fr 1fr;
        gap: 0.5rem;
    }
    button {
        cursor: pointer;
    }
    button:disabled {
        opacity: 0.5;
        cursor: default;
    }
    #text-apply {
        border-color: var(--accent);
        color: var(--accent);
    }
    .actions {
        position: sticky;
        bottom: 0;
        display: grid;
        grid-template-columns: 1fr 1fr;
        gap: 0.4rem;
        padding-block: 0.4rem;
        background: var(--ink-850);
    }
    @media (max-width: 48rem) {
        section {
            box-sizing: border-box;
            inline-size: 100%;
            overflow: visible;
        }
    }
    :focus-visible {
        outline: 2px solid var(--accent);
        outline-offset: 2px;
    }
</style>

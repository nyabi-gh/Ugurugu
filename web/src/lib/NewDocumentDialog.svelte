<!--
SPDX-License-Identifier: GPL-3.0-or-later
Copyright (C) 2026 Nyabi (nyabi-gh)
-->
<script lang="ts">
    import { untrack } from "svelte";
    import { clampCanvasEdge } from "./MemoryPolicy";
    import type { MemoryProfile } from "./MemoryPolicy";

    interface Props {
        profile: MemoryProfile;
        oncreate: (width: number, height: number) => void;
        oncancel: () => void;
    }

    const { profile, oncreate, oncancel }: Props = $props();

    let width = $state(untrack(() => profile.defaultCanvasWidth));
    let height = $state(untrack(() => profile.defaultCanvasHeight));
    let dialog: HTMLDivElement | undefined = $state();

    const estimatedBytes = $derived(
        clampCanvasEdge(width, profile) * clampCanvasEdge(height, profile) * 4,
    );

    function confirm(event: Event) {
        event.preventDefault();
        oncreate(clampCanvasEdge(width, profile), clampCanvasEdge(height, profile));
    }

    function onKeyDown(event: KeyboardEvent) {
        if (event.key === "Escape") {
            event.stopPropagation();
            oncancel();
        }
    }

    $effect(() => {
        dialog?.querySelector("input")?.focus();
    });
</script>

<div
    class="backdrop"
    role="presentation"
    onkeydown={onKeyDown}
>
    <div
        class="dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby="new-document-title"
        bind:this={dialog}
    >
        <h2 id="new-document-title">New document</h2>
        <!--
          novalidate on purpose: max below is the affordance, but a typed
          out-of-range value should clamp to the web policy rather than block
          submission with nothing happening.
        -->
        <form novalidate onsubmit={confirm}>
            <label>
                Width
                <input
                    id="new-document-width"
                    type="number"
                    min="1"
                    max={profile.maximumCanvasEdge}
                    bind:value={width}
                />
            </label>
            <label>
                Height
                <input
                    id="new-document-height"
                    type="number"
                    min="1"
                    max={profile.maximumCanvasEdge}
                    bind:value={height}
                />
            </label>
            <p class="hint">
                Web limit {profile.maximumCanvasEdge}&times;{profile.maximumCanvasEdge}
                &mdash; about {(estimatedBytes / (1024 * 1024)).toFixed(1)} MiB per frame
            </p>
            <div class="actions">
                <button type="button" onclick={oncancel}>Cancel</button>
                <button id="new-document-confirm" type="submit">Create</button>
            </div>
        </form>
    </div>
</div>

<style>
    .backdrop {
        position: fixed;
        inset: 0;
        display: grid;
        place-items: center;
        background: rgb(0 0 0 / 55%);
        z-index: 10;
    }

    .dialog {
        min-width: 18rem;
        padding: 1.25rem;
        border-radius: 0.5rem;
        background: #26292f;
        box-shadow: 0 1rem 2rem rgb(0 0 0 / 45%);
    }

    h2 {
        margin: 0 0 0.75rem;
        font-size: 1rem;
    }

    form {
        display: flex;
        flex-direction: column;
        gap: 0.6rem;
    }

    label {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 0.75rem;
        font-size: 0.85rem;
    }

    input {
        width: 7rem;
        padding: 0.3rem 0.4rem;
        border: 1px solid #43474f;
        border-radius: 0.25rem;
        background: #1d1f24;
        color: inherit;
    }

    .hint {
        margin: 0;
        color: #9aa0aa;
        font-size: 0.75rem;
    }

    .actions {
        display: flex;
        justify-content: flex-end;
        gap: 0.5rem;
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

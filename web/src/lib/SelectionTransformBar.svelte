<!--
SPDX-License-Identifier: GPL-3.0-or-later
Copyright (C) 2026 Nyabi (nyabi-gh)
-->
<script lang="ts">
    // Mirrors the desktop's selection transform commands: move mode plus the
    // Scale (%) and Rotate (degrees) prompts and the two flips. Everything
    // accumulates into one floating transform, so a whole session of tweaks
    // commits as a single undo entry.
    let {
        moveMode,
        pending,
        onmovetoggle,
        onscale,
        onrotate,
        onflip,
        onapply,
        oncancel,
    }: {
        moveMode: boolean;
        pending: boolean;
        onmovetoggle: () => void;
        onscale: (percent: number) => void;
        onrotate: (degrees: number) => void;
        onflip: (horizontal: boolean) => void;
        onapply: () => void;
        oncancel: () => void;
    } = $props();

    // Same defaults and ranges as MainWindow::scaleSelection and
    // MainWindow::rotateSelection.
    const minimumScale = 10;
    const maximumScale = 400;
    const maximumRotation = 360;
    let scalePercent = $state(125);
    let rotationDegrees = $state(90);

    function clamp(value: number, low: number, high: number) {
        return Math.min(high, Math.max(low, value));
    }

    function applyScale() {
        if (!Number.isFinite(scalePercent)) {
            return;
        }
        scalePercent = clamp(scalePercent, minimumScale, maximumScale);
        onscale(scalePercent);
    }

    function applyRotation() {
        if (!Number.isFinite(rotationDegrees)) {
            return;
        }
        rotationDegrees = clamp(
            rotationDegrees,
            -maximumRotation,
            maximumRotation,
        );
        onrotate(rotationDegrees);
    }

    // Enter inside these fields runs the field's own command. The global Enter
    // that commits a floating transform skips text entry, so without this the
    // key would do nothing while the caret is here.
    function onFieldKey(event: KeyboardEvent, run: () => void) {
        if (event.key !== "Enter") {
            return;
        }
        event.preventDefault();
        run();
    }
</script>

<div
    class="transform-bar"
    id="selection-transform"
    role="group"
    aria-label="Selection transform"
>
    <button
        id="selection-move"
        class:active={moveMode}
        aria-pressed={moveMode}
        title="Move mode (M) — drag inside the selection to move it"
        onclick={onmovetoggle}
    >
        Move
    </button>

    <span class="divider" aria-hidden="true"></span>

    <label class="field">
        <span class="field-label">Scale</span>
        <input
            id="selection-scale"
            type="number"
            min={minimumScale}
            max={maximumScale}
            step="1"
            bind:value={scalePercent}
            aria-label="Selection scale in percent"
            onkeydown={(event) => onFieldKey(event, applyScale)}
        />
        <span class="unit" aria-hidden="true">%</span>
    </label>
    <button
        id="selection-scale-apply"
        title="Scale the selection by this percentage"
        onclick={applyScale}
    >
        Apply
    </button>

    <label class="field">
        <span class="field-label">Rotate</span>
        <input
            id="selection-rotate"
            type="number"
            min={-maximumRotation}
            max={maximumRotation}
            step="1"
            bind:value={rotationDegrees}
            aria-label="Selection rotation in degrees"
            onkeydown={(event) => onFieldKey(event, applyRotation)}
        />
        <span class="unit" aria-hidden="true">°</span>
    </label>
    <button
        id="selection-rotate-apply"
        title="Rotate the selection by this angle"
        onclick={applyRotation}
    >
        Apply
    </button>

    <button
        id="selection-flip-horizontal"
        title="Flip the selection horizontally"
        aria-label="Flip the selection horizontally"
        onclick={() => onflip(true)}
    >
        ⇹
    </button>
    <button
        id="selection-flip-vertical"
        title="Flip the selection vertically"
        aria-label="Flip the selection vertically"
        onclick={() => onflip(false)}
    >
        ⇵
    </button>

    <span class="divider" aria-hidden="true"></span>

    <button
        id="selection-transform-apply"
        class="commit"
        title="Apply the transform (Enter)"
        disabled={!pending}
        onclick={onapply}
    >
        Apply move
    </button>
    <button
        id="selection-transform-cancel"
        title="Cancel the transform (Esc)"
        disabled={!pending}
        onclick={oncancel}
    >
        Cancel
    </button>
</div>

<style>
    .transform-bar {
        position: absolute;
        inset-block-start: 0.6rem;
        inset-inline-start: 50%;
        transform: translateX(-50%);
        display: flex;
        flex-wrap: wrap;
        align-items: center;
        gap: 0.3rem;
        max-inline-size: calc(100% - 1.2rem);
        padding: 0.3rem 0.45rem;
        border: 1px solid var(--line);
        border-radius: 12px;
        background: color-mix(in srgb, var(--ink-900) 92%, transparent);
        box-shadow: 0 6px 18px rgb(0 0 0 / 32%);
    }

    button {
        padding: 0.25rem 0.5rem;
        border: 1px solid transparent;
        border-radius: 8px;
        background: var(--ink-750);
        color: var(--paper-dim);
        font-size: 0.75rem;
        cursor: pointer;
    }

    button:hover:not(:disabled) {
        color: var(--paper);
    }

    button:disabled {
        opacity: 0.45;
        cursor: default;
    }

    button:focus-visible {
        outline: 2px solid var(--accent);
        outline-offset: 2px;
    }

    button.active {
        border-color: var(--accent);
        background: var(--accent-bed);
        color: var(--accent);
    }

    button.commit:not(:disabled) {
        color: var(--accent);
    }

    .field {
        display: flex;
        align-items: center;
        gap: 0.2rem;
        color: var(--paper-faint);
        font-size: 0.6875rem;
    }

    .field-label {
        text-transform: uppercase;
        letter-spacing: 0.06em;
    }

    input {
        inline-size: 3.6rem;
        padding: 0.2rem 0.3rem;
        border: 1px solid var(--line);
        border-radius: 7px;
        background: var(--ink-800);
        color: var(--paper);
        font: inherit;
        font-size: 0.75rem;
    }

    input:focus-visible {
        outline: 2px solid var(--accent);
        outline-offset: 1px;
    }

    .unit {
        color: var(--paper-faint);
    }

    .divider {
        inline-size: 1px;
        block-size: 1.1rem;
        background: var(--line);
    }
</style>

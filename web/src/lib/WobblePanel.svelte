<script lang="ts">
    import { untrack } from "svelte";
    import type { LayerInfo, WobbleSettings } from "./EngineClient";

    let {
        wobble,
        frameCount,
        layer,
        onchange,
        onfollow,
    }: {
        wobble: WobbleSettings;
        frameCount: number;
        layer: LayerInfo | undefined;
        onchange: (next: WobbleSettings, layerId: string | null) => void;
        onfollow: (layerId: string) => void;
    } = $props();

    let scope = $state("drawing");
    const shown = $derived(
        scope === "layer" ? (layer?.wobble ?? wobble) : wobble,
    );
    const unavailable = $derived(scope === "layer" && (!layer || layer.group));

    const styles = [
        { value: 0, label: "Classic" },
        { value: 1, label: "Smooth" },
        { value: 2, label: "Stepped" },
    ];

    // The engine owns these values; the draft only exists so a slider can show
    // where it is before the change is committed on release.
    let draft = $state<WobbleSettings>({ ...untrack(() => wobble) });
    $effect(() => {
        draft = { ...shown };
    });

    // Every style but Classic draws one pose per frame, so it cannot ask for
    // more poses than the animation has frames.
    const poseCeiling = $derived(
        draft.style === 0 ? 60 : Math.max(1, frameCount),
    );

    function apply(patch: Partial<WobbleSettings>) {
        const next = { ...draft, ...patch };
        next.poseCount = Math.min(
            next.poseCount,
            next.style === 0 ? 60 : frameCount,
        );
        draft = next;
        if (!unavailable) onchange(next, scope === "layer" ? layer!.id : null);
    }

    function percent(value: number) {
        return Math.round(value * 100);
    }
</script>

<section class="wobble">
    <h2>Wobble</h2>
    <label class="field">
        <span class="field-label">Scope</span>
        <select id="wobble-scope" bind:value={scope}>
            <option value="drawing">Whole drawing</option>
            <option value="layer">Active layer</option>
        </select>
    </label>
    {#if scope === "layer"}
        <p class="scope-status">
            {unavailable ? "Choose a paint layer." : layer?.name}
        </p>
        <button
            id="wobble-follow"
            disabled={unavailable || !layer?.wobble}
            onclick={() => layer && onfollow(layer.id)}
            >Follow drawing settings</button
        >
    {/if}
    <fieldset disabled={unavailable}>
        <label class="field">
            <span class="field-label">
                Amount <em>{draft.amount.toFixed(1)}</em>
            </span>
            <input
                id="wobble-amount"
                type="range"
                min="0"
                max="12"
                step="0.1"
                bind:value={draft.amount}
                onchange={() => apply({ amount: draft.amount })}
            />
        </label>

        <div class="field">
            <span class="field-label">Style</span>
            <div class="segment" role="group" aria-label="Motion style">
                {#each styles as style (style.value)}
                    <button
                        id={`wobble-style-${style.value}`}
                        class:on={draft.style === style.value}
                        aria-pressed={draft.style === style.value}
                        onclick={() => apply({ style: style.value })}
                    >
                        {style.label}
                    </button>
                {/each}
            </div>
        </div>

        <details>
            <summary id="wobble-more">More</summary>

            <label class="field">
                <span class="field-label">
                    Poses <em>{draft.poseCount}</em>
                </span>
                <input
                    id="wobble-pose-count"
                    type="range"
                    min="1"
                    max={poseCeiling}
                    step="1"
                    bind:value={draft.poseCount}
                    onchange={() => apply({ poseCount: draft.poseCount })}
                />
            </label>

            <label class="field">
                <span class="field-label">
                    Detail <em>{draft.detail}</em>
                </span>
                <input
                    id="wobble-detail"
                    type="range"
                    min="1"
                    max="24"
                    step="1"
                    bind:value={draft.detail}
                    onchange={() => apply({ detail: draft.detail })}
                />
            </label>

            <label class="field">
                <span class="field-label">
                    Linked <em>{percent(draft.linked)}%</em>
                </span>
                <input
                    id="wobble-linked"
                    type="range"
                    min="0"
                    max="100"
                    step="1"
                    value={percent(draft.linked)}
                    oninput={(event) =>
                        (draft.linked =
                            Number(
                                (event.currentTarget as HTMLInputElement).value,
                            ) / 100)}
                    onchange={() => apply({ linked: draft.linked })}
                />
            </label>

            <label class="field">
                <span class="field-label">
                    Randomness <em>{percent(draft.randomness)}%</em>
                </span>
                <input
                    id="wobble-randomness"
                    type="range"
                    min="0"
                    max="100"
                    step="1"
                    value={percent(draft.randomness)}
                    oninput={(event) =>
                        (draft.randomness =
                            Number(
                                (event.currentTarget as HTMLInputElement).value,
                            ) / 100)}
                    onchange={() => apply({ randomness: draft.randomness })}
                />
            </label>

            <label class="check">
                <input
                    id="wobble-broken"
                    type="checkbox"
                    checked={draft.brokenLine}
                    onchange={(event) =>
                        apply({
                            brokenLine: (
                                event.currentTarget as HTMLInputElement
                            ).checked,
                        })}
                />
                Broken line
            </label>

            <label class="field" class:disabled={!draft.brokenLine}>
                <span class="field-label">
                    Break amount <em>{percent(draft.breakAmount)}%</em>
                </span>
                <input
                    id="wobble-break-amount"
                    type="range"
                    min="0"
                    max="100"
                    step="1"
                    disabled={!draft.brokenLine}
                    value={percent(draft.breakAmount)}
                    oninput={(event) =>
                        (draft.breakAmount =
                            Number(
                                (event.currentTarget as HTMLInputElement).value,
                            ) / 100)}
                    onchange={() => apply({ breakAmount: draft.breakAmount })}
                />
            </label>

            <label class="field" class:disabled={!draft.brokenLine}>
                <span class="field-label">
                    Break range <em>{draft.breakRange.toFixed(0)} px</em>
                </span>
                <input
                    id="wobble-break-range"
                    type="range"
                    min="2"
                    max="256"
                    step="1"
                    disabled={!draft.brokenLine}
                    bind:value={draft.breakRange}
                    onchange={() => apply({ breakRange: draft.breakRange })}
                />
            </label>
        </details>
    </fieldset>
</section>

<style>
    fieldset {
        border: 0;
        padding: 0;
        margin: 0;
        min-inline-size: 0;
        display: flex;
        flex-direction: column;
        gap: 0.4rem;
    }
    fieldset:disabled {
        opacity: 0.5;
    }
    select,
    #wobble-follow {
        min-block-size: 2.75rem;
        background: var(--ink-800);
        color: var(--paper);
        border: 1px solid var(--line);
        border-radius: 6px;
        padding: 0.35rem;
    }
    .scope-status {
        margin: 0;
        font-size: 0.75rem;
        overflow-wrap: anywhere;
    }

    .wobble {
        flex: none;
        display: flex;
        flex-direction: column;
        gap: 0.4rem;
        padding: 0.6rem 0.75rem;
        border-block-end: 1px solid var(--line);
    }

    h2 {
        margin: 0;
        font-size: 0.625rem;
        font-weight: 700;
        letter-spacing: 0.13em;
        text-transform: uppercase;
        color: var(--paper-dim);
    }

    .field {
        display: flex;
        flex-direction: column;
        gap: 0.25rem;
    }

    .field.disabled {
        opacity: 0.45;
    }

    .field-label {
        display: flex;
        align-items: baseline;
        justify-content: space-between;
        font-size: 0.625rem;
        font-weight: 700;
        letter-spacing: 0.1em;
        text-transform: uppercase;
        color: var(--paper-dim);
    }

    .field-label em {
        font-family: var(--mono);
        font-size: 0.6875rem;
        font-style: normal;
        font-variant-numeric: tabular-nums;
        letter-spacing: 0;
        text-transform: none;
        color: var(--paper);
    }

    input[type="range"] {
        inline-size: 100%;
        accent-color: var(--accent);
    }

    .segment {
        display: flex;
        gap: 0.15rem;
    }

    .segment button {
        flex: 1;
        padding: 0.25rem 0;
        border: 1px solid var(--line);
        border-radius: 6px;
        background: var(--ink-800);
        color: var(--paper-dim);
        font-size: 0.6875rem;
        cursor: pointer;
    }

    .segment button.on {
        border-color: var(--accent);
        background: var(--accent-bed);
        color: var(--accent);
    }

    .segment button:focus-visible,
    summary:focus-visible {
        outline: 2px solid var(--accent);
        outline-offset: 1px;
    }

    details {
        display: flex;
        flex-direction: column;
    }

    summary {
        font-size: 0.625rem;
        font-weight: 700;
        letter-spacing: 0.1em;
        text-transform: uppercase;
        color: var(--paper-dim);
        cursor: pointer;
    }

    details[open] summary {
        margin-block-end: 0.4rem;
    }

    details .field,
    details .check {
        margin-block-end: 0.4rem;
    }

    .check {
        display: flex;
        align-items: center;
        gap: 0.4rem;
        font-size: 0.75rem;
        color: var(--paper-dim);
    }

    .check input {
        accent-color: var(--accent);
    }
</style>

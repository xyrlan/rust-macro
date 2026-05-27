<script lang="ts">
  import { phase, disarm, begin, restoreWindow, discard, complete } from "../stores/recording";
  import * as api from "../api";
  import { reportError } from "../stores/toast";
  import { loadAll } from "../stores/macros";
  import HotkeyPicker from "./HotkeyPicker.svelte";
  import type { Trigger, PlaybackMode, StepDto } from "../types";
  import { stepLabel } from "../types";

  // Form state for the preview phase.
  let name = $state("");
  let trigger = $state<Trigger>({ type: "hotkey", key: "f1", modifiers: ["ctrl"] });
  let playback = $state<PlaybackMode>({ type: "once" });
  let repeatN = $state(3);
  let saving = $state(false);

  $effect(() => {
    // When we enter preview phase, restore the window and reset the form.
    if ($phase.tag === "preview") {
      void restoreWindow();
      name = "";
      trigger = { type: "hotkey", key: "f1", modifiers: ["ctrl"] };
      playback = { type: "once" };
      repeatN = 3;
      saving = false;
    }
  });

  function changePlayback(e: Event) {
    const v = (e.target as HTMLSelectElement).value;
    switch (v) {
      case "once": playback = { type: "once" }; break;
      case "repeat": playback = { type: "repeat", value: repeatN }; break;
      case "loop": playback = { type: "loop" }; break;
      case "toggle": playback = { type: "toggle" }; break;
    }
  }

  function changeRepeatN(e: Event) {
    repeatN = Math.max(1, Number((e.target as HTMLInputElement).value));
    if (playback.type === "repeat") playback = { type: "repeat", value: repeatN };
  }

  async function save() {
    if ($phase.tag !== "preview") return;
    if (name.trim() === "") return;
    saving = true;
    try {
      await api.createMacro(name.trim(), trigger, playback, $phase.steps);
      await loadAll();
      complete();
    } catch (e) {
      reportError(e);
    } finally {
      saving = false;
    }
  }

  function backdropClick(e: MouseEvent) {
    if (e.target === e.currentTarget) {
      if ($phase.tag === "armed") disarm();
      else if ($phase.tag === "preview") discard();
    }
  }

  function stepSummary(s: StepDto): string {
    switch (s.type) {
      case "key_press": return `KeyPress ${s.key} hold ${s.hold_ms}ms`;
      case "key_down": return `KeyDown ${s.key}`;
      case "key_up": return `KeyUp ${s.key}`;
      case "mouse_click": return `MouseClick ${s.button} hold ${s.hold_ms}ms`;
      case "mouse_move": return `MouseMove (${s.to.x},${s.to.y}) ${s.mode}`;
      case "mouse_scroll": return `MouseScroll ${s.delta}`;
      case "wait":
        return s.min_ms === s.max_ms
          ? `Wait ${s.min_ms}ms`
          : `Wait ${s.min_ms}-${s.max_ms}ms`;
    }
  }
</script>

{#if $phase.tag === "armed"}
  <div class="backdrop" onclick={backdropClick} role="presentation">
    <div class="modal" role="dialog" aria-labelledby="rec-armed-title">
      <h3 id="rec-armed-title">Record a new macro</h3>
      <p>
        Press <strong>F10</strong> to stop. The window will minimize while you record.
      </p>
      <div class="actions">
        <button onclick={disarm}>Cancel</button>
        <button class="primary" onclick={() => void begin()}>Start</button>
      </div>
    </div>
  </div>
{:else if $phase.tag === "preview"}
  <div class="backdrop" onclick={backdropClick} role="presentation">
    <div class="modal preview" role="dialog" aria-labelledby="rec-preview-title">
      <h3 id="rec-preview-title">Recording finished — {$phase.steps.length} steps captured</h3>

      <div class="step-list">
        {#each $phase.steps as s, i}
          <div class="step-line"><span class="num">#{i + 1}</span> {stepSummary(s)}</div>
        {/each}
        {#if $phase.steps.length === 0}
          <div class="empty">No steps captured.</div>
        {/if}
      </div>

      <div class="field">
        <label for="rec-name">Name</label>
        <input id="rec-name" bind:value={name} />
      </div>

      <div class="field">
        <label>Hotkey</label>
        <HotkeyPicker value={trigger} onChange={(t) => (trigger = t)} />
      </div>

      <div class="field">
        <label for="rec-mode">Playback mode</label>
        <select id="rec-mode" value={playback.type} onchange={changePlayback}>
          <option value="once">Once</option>
          <option value="repeat">Repeat (N)</option>
          <option value="loop">Loop</option>
          <option value="toggle">Toggle</option>
        </select>
        {#if playback.type === "repeat"}
          <input
            class="repeat-n"
            type="number"
            min="1"
            value={repeatN}
            oninput={changeRepeatN}
          />
        {/if}
      </div>

      <div class="actions">
        <button onclick={discard}>Discard</button>
        <button
          class="primary"
          disabled={saving || name.trim() === "" || $phase.steps.length === 0}
          onclick={save}
        >
          {saving ? "Saving…" : "Save"}
        </button>
      </div>
    </div>
  </div>
{/if}

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.6);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 600;
  }
  .modal {
    background: var(--bg-elevated);
    border: 1px solid var(--border);
    border-radius: 8px;
    padding: 1.5rem;
    width: 100%;
    max-width: 460px;
    box-shadow: 0 16px 48px rgba(0, 0, 0, 0.5);
  }
  .modal.preview { max-width: 560px; }
  h3 { margin: 0 0 1rem 0; }
  .step-list {
    max-height: 240px;
    overflow-y: auto;
    background: var(--bg-input);
    border: 1px solid var(--border);
    border-radius: 4px;
    padding: 0.5rem 0.75rem;
    margin-bottom: 1rem;
    font-family: ui-monospace, "Cascadia Code", "Consolas", monospace;
    font-size: 0.85rem;
  }
  .step-line { padding: 0.1rem 0; }
  .num {
    display: inline-block;
    width: 2.5rem;
    color: var(--text-muted);
  }
  .empty { color: var(--text-muted); text-align: center; padding: 1rem; }
  .field { margin-bottom: 1rem; }
  .field > label {
    display: block;
    font-size: 0.85rem;
    color: var(--text-muted);
    text-transform: uppercase;
    letter-spacing: 0.05em;
    margin-bottom: 0.35rem;
  }
  .field input, .field select { width: 100%; }
  .repeat-n {
    margin-top: 0.5rem;
    width: 100px !important;
  }
  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 0.5rem;
    margin-top: 1.5rem;
  }
</style>

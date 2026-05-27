<script lang="ts">
  import { onMount } from "svelte";
  import type { MacroDto, StepDto, Trigger, PlaybackMode } from "../types";
  import { STEP_DEFAULTS, stepLabel } from "../types";
  import * as api from "../api";
  import { reportError } from "../stores/toast";
  import { loadAll, snapshot } from "../stores/macros";
  import HotkeyPicker from "./HotkeyPicker.svelte";
  import StepRow from "./StepRow.svelte";

  let { macroId, onBack }: { macroId: string; onBack: () => void } = $props();

  let macro = $state<MacroDto | null>(null);
  let steps = $state<StepDto[]>([]);
  let initialSnapshot = $state<string>("");
  let loading = $state(true);
  let saving = $state(false);
  let addType = $state<StepDto["type"]>("key_press");

  // Local edit state for metadata
  let name = $state("");
  let trigger = $state<Trigger>({ type: "hotkey", key: "f1", modifiers: ["ctrl"] });
  let playback = $state<PlaybackMode>({ type: "once" });
  let repeatN = $state(3);

  onMount(async () => {
    const m = snapshot().find((x) => x.id === macroId);
    if (!m) {
      reportError(new Error("Macro not found in list"));
      onBack();
      return;
    }
    macro = m;
    name = m.name;
    trigger = m.trigger;
    playback = m.playback;
    if (m.playback.type === "repeat") repeatN = m.playback.value;

    try {
      steps = await api.loadMacroSteps(macroId);
      initialSnapshot = JSON.stringify({ name, trigger, playback, steps });
    } catch (e) {
      reportError(e);
      onBack();
    } finally {
      loading = false;
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

  function moveStep(i: number, delta: number) {
    const j = i + delta;
    if (j < 0 || j >= steps.length) return;
    const next = [...steps];
    [next[i], next[j]] = [next[j], next[i]];
    steps = next;
  }

  function removeStep(i: number) {
    steps = steps.filter((_, idx) => idx !== i);
  }

  function updateStep(i: number, s: StepDto) {
    const next = [...steps];
    next[i] = s;
    steps = next;
  }

  function addStep() {
    steps = [...steps, STEP_DEFAULTS[addType]()];
  }

  function isDirty(): boolean {
    return JSON.stringify({ name, trigger, playback, steps }) !== initialSnapshot;
  }

  async function save() {
    if (!macro) return;
    if (name.trim() === "") return;
    saving = true;
    try {
      await api.updateMacroFull(macro.id, name.trim(), trigger, playback, steps);
      await loadAll();
      onBack();
    } catch (e) {
      reportError(e);
    } finally {
      saving = false;
    }
  }

  function discard() {
    if (isDirty()) {
      if (!confirm("Discard unsaved changes?")) return;
    }
    onBack();
  }
</script>

{#if loading}
  <main class="loading">
    <p>Loading editor…</p>
  </main>
{:else if macro}
  <main class="editor">
    <header>
      <button class="back" onclick={discard}>← Back to list</button>
      <div class="spacer"></div>
      <button onclick={discard}>Discard</button>
      <button
        class="primary"
        disabled={saving || name.trim() === ""}
        onclick={save}
      >{saving ? "Saving…" : "Save"}</button>
    </header>

    <section class="metadata">
      <h2>Metadata</h2>
      <div class="field">
        <label for="ed-name">Name</label>
        <input id="ed-name" bind:value={name} />
      </div>
      <div class="field">
        <label>Hotkey</label>
        <HotkeyPicker value={trigger} onChange={(t) => (trigger = t)} />
      </div>
      <div class="field">
        <label for="ed-mode">Playback mode</label>
        <select id="ed-mode" value={playback.type} onchange={changePlayback}>
          <option value="once">Once</option>
          <option value="repeat">Repeat (N)</option>
          <option value="loop">Loop</option>
          <option value="toggle">Toggle</option>
        </select>
        {#if playback.type === "repeat"}
          <input class="repeat-n" type="number" min="1" value={repeatN} oninput={changeRepeatN} />
        {/if}
      </div>
    </section>

    <section class="steps">
      <h2>Steps ({steps.length})</h2>
      {#if steps.length === 0}
        <p class="empty">No steps. Use "+ Add step" below to add one.</p>
      {:else}
        {#each steps as s, i}
          <StepRow
            step={s}
            index={i}
            canMoveUp={i > 0}
            canMoveDown={i < steps.length - 1}
            onChange={(ns) => updateStep(i, ns)}
            onMoveUp={() => moveStep(i, -1)}
            onMoveDown={() => moveStep(i, 1)}
            onRemove={() => removeStep(i)}
          />
        {/each}
      {/if}
      <div class="add-step">
        <select bind:value={addType}>
          <option value="key_press">{stepLabel("key_press")}</option>
          <option value="key_down">{stepLabel("key_down")}</option>
          <option value="key_up">{stepLabel("key_up")}</option>
          <option value="mouse_click">{stepLabel("mouse_click")}</option>
          <option value="mouse_move">{stepLabel("mouse_move")}</option>
          <option value="mouse_scroll">{stepLabel("mouse_scroll")}</option>
          <option value="wait">{stepLabel("wait")}</option>
        </select>
        <button onclick={addStep}>+ Add step</button>
      </div>
    </section>
  </main>
{/if}

<style>
  main.loading, main.editor {
    max-width: 960px;
    margin: 0 auto;
    padding: 1.5rem;
  }
  header {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    margin-bottom: 1.5rem;
  }
  .back { background: transparent; }
  .spacer { flex: 1; }
  section { margin-bottom: 2rem; }
  h2 { font-size: 1rem; color: var(--text-muted); text-transform: uppercase; letter-spacing: 0.05em; margin: 0 0 0.75rem 0; }
  .field { margin-bottom: 0.75rem; }
  .field > label {
    display: block;
    font-size: 0.85rem;
    color: var(--text-muted);
    margin-bottom: 0.25rem;
  }
  .field input, .field select { width: 100%; max-width: 360px; }
  .repeat-n { margin-top: 0.4rem; width: 100px !important; }
  .empty { color: var(--text-muted); padding: 1rem 0; }
  .add-step {
    display: flex;
    gap: 0.5rem;
    margin-top: 1rem;
  }
  .add-step select { max-width: 200px; }
</style>

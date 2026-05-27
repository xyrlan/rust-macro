<script lang="ts">
  import type { MacroDto, Trigger, PlaybackMode } from "../types";
  import { updateMetadata } from "../stores/macros";
  import HotkeyPicker from "./HotkeyPicker.svelte";

  let {
    macro,
    onClose,
  }: {
    macro: MacroDto;
    onClose: () => void;
  } = $props();

  let name = $state(macro.name);
  let trigger = $state<Trigger>(macro.trigger);
  let playback = $state<PlaybackMode>(macro.playback);
  let repeatN = $state(
    macro.playback.type === "repeat" ? macro.playback.value : 1,
  );
  let saving = $state(false);

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
    if (playback.type === "repeat") {
      playback = { type: "repeat", value: repeatN };
    }
  }

  async function save() {
    if (name.trim() === "") return;
    saving = true;
    await updateMetadata(macro.id, name.trim(), trigger, playback);
    saving = false;
    onClose();
  }

  function backdropClick(e: MouseEvent) {
    if (e.target === e.currentTarget) onClose();
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") onClose();
  }
</script>

<svelte:window onkeydown={onKeydown} />

<div class="backdrop" onclick={backdropClick} role="presentation">
  <div class="modal" role="dialog" aria-labelledby="edit-title">
    <h3 id="edit-title">Edit metadata</h3>

    <div class="field">
      <label for="edit-name">Name</label>
      <input id="edit-name" bind:value={name} />
    </div>

    <div class="field">
      <label>Hotkey</label>
      <HotkeyPicker
        value={trigger}
        onChange={(t) => (trigger = t)}
      />
    </div>

    <div class="field">
      <label for="edit-mode">Playback mode</label>
      <select id="edit-mode" value={playback.type} onchange={changePlayback}>
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
      <button onclick={onClose}>Cancel</button>
      <button class="primary" disabled={saving || name.trim() === ""} onclick={save}>
        {saving ? "Saving…" : "Save"}
      </button>
    </div>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.6);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 500;
  }
  .modal {
    background: var(--bg-elevated);
    border: 1px solid var(--border);
    border-radius: 8px;
    padding: 1.5rem;
    width: 100%;
    max-width: 420px;
    box-shadow: 0 16px 48px rgba(0, 0, 0, 0.5);
  }
  h3 {
    margin: 0 0 1.25rem 0;
  }
  .field {
    margin-bottom: 1rem;
  }
  .field > label {
    display: block;
    font-size: 0.85rem;
    color: var(--text-muted);
    text-transform: uppercase;
    letter-spacing: 0.05em;
    margin-bottom: 0.35rem;
  }
  .field input,
  .field select {
    width: 100%;
  }
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

<script lang="ts">
  import { onMount } from "svelte";
  import { settings, load as loadSettings, save as saveSettings } from "../stores/settings";
  import type { KeyCode } from "../types";

  let { onBack }: { onBack: () => void } = $props();

  let stopKey = $state<KeyCode>("f10");
  let storageOverride = $state<string>("");
  let saving = $state(false);

  onMount(async () => {
    await loadSettings();
    const s = $settings;
    stopKey = s.stop_key;
    storageOverride = s.storage_root_override ?? "";
  });

  const STOP_KEY_OPTIONS: KeyCode[] = [
    "f1", "f2", "f3", "f4", "f5", "f6", "f7", "f8", "f9", "f10", "f11", "f12",
    "escape", "pause",
  ];

  async function save() {
    saving = true;
    await saveSettings({
      stop_key: stopKey,
      storage_root_override: storageOverride.trim() === "" ? null : storageOverride.trim(),
    });
    saving = false;
  }
</script>

<main>
  <header>
    <button class="back" onclick={onBack}>← Back</button>
    <div class="spacer"></div>
    <button class="primary" disabled={saving} onclick={save}>{saving ? "Saving…" : "Save"}</button>
  </header>

  <h2>Settings</h2>

  <div class="field">
    <label for="stop-key">Recording stop key</label>
    <select id="stop-key" bind:value={stopKey}>
      {#each STOP_KEY_OPTIONS as k}<option value={k}>{k.toUpperCase()}</option>{/each}
    </select>
    <p class="hint">Pressed during a recording to stop it. Default: F10.</p>
  </div>

  <div class="field">
    <label for="storage-root">Storage root override</label>
    <input id="storage-root" bind:value={storageOverride} placeholder="(default: %AppData%\rust-macro)" />
    <p class="hint">
      Leave empty for the default. Changing this does NOT move existing macros —
      restart the app after changing.
    </p>
  </div>
</main>

<style>
  main { max-width: 720px; margin: 0 auto; padding: 1.5rem; }
  header { display: flex; gap: 0.5rem; align-items: center; margin-bottom: 1.5rem; }
  .back { background: transparent; }
  .spacer { flex: 1; }
  .field { margin-bottom: 1rem; }
  .field > label { display: block; font-size: 0.85rem; color: var(--text-muted); margin-bottom: 0.35rem; text-transform: uppercase; letter-spacing: 0.05em; }
  .field input, .field select { width: 100%; max-width: 360px; }
  .hint { color: var(--text-muted); font-size: 0.8rem; margin: 0.25rem 0 0 0; }
</style>

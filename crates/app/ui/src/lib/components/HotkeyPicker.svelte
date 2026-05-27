<script lang="ts">
  import type { Trigger, KeyCode, Modifier } from "../types";

  let { value, onChange }: { value: Trigger; onChange: (t: Trigger) => void } = $props();

  import { inputLabel } from "../types";

  // Subset of keys we expose in the dropdown. Live capture lands in Plan 3b.
  const KEY_OPTIONS: KeyCode[] = [
    "f1", "f2", "f3", "f4", "f5", "f6", "f7", "f8", "f9", "f10", "f11", "f12",
    "a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l", "m",
    "n", "o", "p", "q", "r", "s", "t", "u", "v", "w", "x", "y", "z",
    "num0", "num1", "num2", "num3", "num4",
    "num5", "num6", "num7", "num8", "num9",
    "space", "enter", "tab", "escape",
    "up", "down", "left", "right",
  ];
  const MODIFIERS: Modifier[] = ["ctrl", "shift", "alt", "win"];

  function toggle(mod: Modifier) {
    if (value.type !== "hotkey") return;
    const has = value.modifiers.includes(mod);
    const modifiers = has
      ? value.modifiers.filter((m) => m !== mod)
      : [...value.modifiers, mod];
    onChange({ ...value, modifiers });
  }

  function changeKey(e: Event) {
    const key = (e.target as HTMLSelectElement).value as KeyCode;
    if (value.type !== "hotkey") return;
    onChange({ ...value, key });
  }
</script>

<div class="modifiers">
  {#each MODIFIERS as mod}
    <label>
      <input
        type="checkbox"
        checked={value.type === "hotkey" && value.modifiers.includes(mod)}
        onchange={() => toggle(mod)}
      />
      {inputLabel(mod)}
    </label>
  {/each}
</div>
<select onchange={changeKey} value={value.type === "hotkey" ? value.key : "f1"}>
  {#each KEY_OPTIONS as k}
    <option value={k}>{inputLabel(k)}</option>
  {/each}
</select>

<style>
  .modifiers {
    display: flex;
    gap: 0.75rem;
    margin-bottom: 0.5rem;
  }
  label {
    cursor: pointer;
    user-select: none;
  }
  select {
    width: 100%;
  }
</style>

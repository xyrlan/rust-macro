<script lang="ts">
  import type { Trigger, KeyCode, Modifier } from "../types";
  import { inputLabel } from "../types";

  let { value, onChange }: { value: Trigger; onChange: (t: Trigger) => void } = $props();

  // Subset of keys we expose in the dropdown fallback. Live capture covers
  // most everyday combos; the dropdown is for users who want a key the
  // browser can't see (e.g. Print Screen, Win key alone — though Esc is
  // RESERVED for cancel during capture).
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

  let listening = $state(false);
  let liveModifiers = $state<Modifier[]>([]);
  let liveKey = $state<KeyCode | null>(null);
  let timeoutHandle: ReturnType<typeof setTimeout> | null = null;

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

  // ---- Capture mode ----

  function startCapture() {
    listening = true;
    liveModifiers = [];
    liveKey = null;
    window.addEventListener("keydown", onKeyDown, { capture: true });
    window.addEventListener("keyup", onKeyUp, { capture: true });
    timeoutHandle = setTimeout(() => stopCapture(false), 5000);
  }

  function stopCapture(commit: boolean) {
    listening = false;
    if (timeoutHandle) { clearTimeout(timeoutHandle); timeoutHandle = null; }
    window.removeEventListener("keydown", onKeyDown, true);
    window.removeEventListener("keyup", onKeyUp, true);
    if (commit && liveKey) {
      onChange({ type: "hotkey", key: liveKey, modifiers: liveModifiers });
    }
    liveModifiers = [];
    liveKey = null;
  }

  // Map a browser KeyboardEvent.code -> KeyCode (snake_case). Returns null
  // for codes we don't expose. Keep in sync with rm_macro_model::KeyCode.
  function codeToKeyCode(code: string): KeyCode | null {
    if (/^Key[A-Z]$/.test(code)) return code.slice(3).toLowerCase() as KeyCode;
    if (/^Digit[0-9]$/.test(code)) return ("num" + code.slice(5)) as KeyCode;
    if (/^F([1-9]|1[0-2])$/.test(code)) return code.toLowerCase() as KeyCode;
    if (/^Numpad[0-9]$/.test(code)) return ("num" + code.slice(6)) as KeyCode;
    switch (code) {
      case "Space": return "space";
      case "Enter": return "enter";
      case "Tab": return "tab";
      case "Backspace": return "backspace";
      case "CapsLock": return "caps_lock";
      case "ArrowUp": return "up";
      case "ArrowDown": return "down";
      case "ArrowLeft": return "left";
      case "ArrowRight": return "right";
      case "Insert": return "insert";
      case "Delete": return "delete";
      case "Home": return "home";
      case "End": return "end";
      case "PageUp": return "page_up";
      case "PageDown": return "page_down";
      case "Minus": return "minus";
      case "Equal": return "equals";
      case "BracketLeft": return "l_bracket";
      case "BracketRight": return "r_bracket";
      case "Backslash": return "backslash";
      case "Semicolon": return "semicolon";
      case "Quote": return "apostrophe";
      case "Backquote": return "backtick";
      case "Comma": return "comma";
      case "Period": return "period";
      case "Slash": return "slash";
      // Modifiers handled separately
      default: return null;
    }
  }

  function isModifierCode(code: string): boolean {
    return ["ShiftLeft", "ShiftRight", "ControlLeft", "ControlRight",
            "AltLeft", "AltRight", "MetaLeft", "MetaRight"].includes(code);
  }

  function modifiersFromEvent(e: KeyboardEvent): Modifier[] {
    const mods: Modifier[] = [];
    if (e.ctrlKey) mods.push("ctrl");
    if (e.shiftKey) mods.push("shift");
    if (e.altKey) mods.push("alt");
    if (e.metaKey) mods.push("win");
    return mods;
  }

  function onKeyDown(e: KeyboardEvent) {
    e.preventDefault();
    e.stopPropagation();
    if (e.code === "Escape") { stopCapture(false); return; }
    if (isModifierCode(e.code)) {
      liveModifiers = modifiersFromEvent(e);
      return;
    }
    const k = codeToKeyCode(e.code);
    if (k) {
      liveKey = k;
      liveModifiers = modifiersFromEvent(e);
    }
  }

  function onKeyUp(e: KeyboardEvent) {
    e.preventDefault();
    e.stopPropagation();
    // Commit on the keyup of a non-modifier key.
    if (liveKey && !isModifierCode(e.code)) {
      stopCapture(true);
    }
  }

  function liveLabel(): string {
    if (!liveKey) return liveModifiers.map(inputLabel).join("+") || "...";
    return [...liveModifiers, liveKey].map(inputLabel).join("+");
  }
</script>

{#if listening}
  <div class="listening">
    <span class="banner">Press your hotkey combo: <code>{liveLabel()}</code></span>
    <button onclick={() => stopCapture(false)}>Cancel</button>
  </div>
  <p class="hint">Esc to cancel. Modifier-only combos are not allowed.</p>
{:else}
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
  <div class="key-row">
    <select onchange={changeKey} value={value.type === "hotkey" ? value.key : "f1"}>
      {#each KEY_OPTIONS as k}
        <option value={k}>{inputLabel(k)}</option>
      {/each}
    </select>
    <button onclick={startCapture} title="Press a key combo to bind">🎯 Capture</button>
  </div>
{/if}

<style>
  .modifiers {
    display: flex;
    gap: 0.75rem;
    margin-bottom: 0.5rem;
  }
  label { cursor: pointer; user-select: none; }
  .key-row {
    display: flex;
    gap: 0.5rem;
  }
  .key-row select { flex: 1; }
  .listening {
    display: flex;
    gap: 0.5rem;
    align-items: center;
    padding: 0.5rem 0.75rem;
    background: rgba(37, 99, 235, 0.12);
    border: 1px solid var(--accent);
    border-radius: 4px;
  }
  .banner { flex: 1; }
  .hint {
    margin: 0.25rem 0 0 0;
    color: var(--text-muted);
    font-size: 0.8rem;
  }
</style>

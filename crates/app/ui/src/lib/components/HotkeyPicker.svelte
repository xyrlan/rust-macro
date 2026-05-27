<script lang="ts">
  import type { Trigger, KeyCode, Modifier, MouseButton } from "../types";
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
  const MOUSE_BUTTON_OPTIONS: MouseButton[] = ["left", "right", "middle", "x1", "x2"];

  let listening = $state(false);
  let liveModifiers = $state<Modifier[]>([]);
  let liveKey = $state<KeyCode | null>(null);
  let liveButton = $state<MouseButton | null>(null);
  let timeoutHandle: ReturnType<typeof setTimeout> | null = null;

  // ---- Mode toggle ----

  function setMode(mode: "hotkey" | "mouse_button") {
    if (value.type === mode) return;
    if (mode === "hotkey") {
      onChange({ type: "hotkey", key: "f1", modifiers: value.modifiers });
    } else {
      onChange({ type: "mouse_button", button: "x1", modifiers: value.modifiers });
    }
  }

  // ---- Modifier checkboxes (variant-agnostic) ----

  function toggle(mod: Modifier) {
    const has = value.modifiers.includes(mod);
    const modifiers = has
      ? value.modifiers.filter((m) => m !== mod)
      : [...value.modifiers, mod];
    onChange({ ...value, modifiers });
  }

  // ---- Key select ----

  function changeKey(e: Event) {
    if (value.type !== "hotkey") return;
    const key = (e.target as HTMLSelectElement).value as KeyCode;
    onChange({ ...value, key });
  }

  // ---- Mouse button select ----

  function changeMouseButton(e: Event) {
    if (value.type !== "mouse_button") return;
    const button = (e.target as HTMLSelectElement).value as MouseButton;
    onChange({ ...value, button });
  }

  // ---- Keyboard Capture mode ----

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

  // ---- Mouse Capture mode ----

  function startMouseCapture() {
    listening = true;
    liveModifiers = [];
    liveButton = null;
    // Wrap in setTimeout(0) so the click event that triggered this button
    // has fully settled before we start listening for mousedown events.
    // Without this, the capture button's own click would immediately commit.
    setTimeout(() => {
      window.addEventListener("mousedown", onMouseDown, { capture: true });
      window.addEventListener("keydown", onCaptureEscape, { capture: true });
      timeoutHandle = setTimeout(() => stopMouseCapture(false), 5000);
    }, 0);
  }

  function stopMouseCapture(commit: boolean) {
    listening = false;
    if (timeoutHandle) { clearTimeout(timeoutHandle); timeoutHandle = null; }
    window.removeEventListener("mousedown", onMouseDown, true);
    window.removeEventListener("keydown", onCaptureEscape, true);
    if (commit && liveButton) {
      onChange({ type: "mouse_button", button: liveButton, modifiers: liveModifiers });
    }
    liveModifiers = [];
    liveButton = null;
  }

  function onMouseDown(e: MouseEvent) {
    e.preventDefault();
    e.stopPropagation();
    // Browser MouseEvent.button: 0=left, 1=middle, 2=right, 3=x1, 4=x2
    const map: Record<number, MouseButton> = {
      0: "left", 1: "middle", 2: "right", 3: "x1", 4: "x2"
    };
    const btn = map[e.button];
    if (btn) {
      liveButton = btn;
      liveModifiers = modifiersFromMouseEvent(e);
      stopMouseCapture(true);
    }
  }

  function onCaptureEscape(e: KeyboardEvent) {
    if (e.code === "Escape") {
      e.preventDefault();
      stopMouseCapture(false);
    }
  }

  function modifiersFromMouseEvent(e: MouseEvent): Modifier[] {
    const mods: Modifier[] = [];
    if (e.ctrlKey) mods.push("ctrl");
    if (e.shiftKey) mods.push("shift");
    if (e.altKey) mods.push("alt");
    if (e.metaKey) mods.push("win");
    return mods;
  }

  // ---- Live label ----

  function liveLabel(): string {
    const mods = liveModifiers.map(inputLabel).join("+");
    let tail = "...";
    if (value.type === "hotkey" && liveKey) tail = inputLabel(liveKey);
    if (value.type === "mouse_button" && liveButton) tail = `Mouse:${inputLabel(liveButton)}`;
    return mods ? `${mods}+${tail}` : tail;
  }
</script>

{#if listening}
  <div class="listening">
    <span class="banner">
      {value.type === "hotkey" ? "Press your hotkey combo:" : "Click your mouse button:"}
      <code>{liveLabel()}</code>
    </span>
    <button onclick={() => value.type === "hotkey" ? stopCapture(false) : stopMouseCapture(false)}>Cancel</button>
  </div>
  <p class="hint">Esc to cancel.{value.type === "hotkey" ? " Modifier-only combos are not allowed." : ""}</p>
{:else}
  <div class="mode-row">
    <label><input type="radio" name="trigger-mode" checked={value.type === "hotkey"} onchange={() => setMode("hotkey")} /> Keyboard</label>
    <label><input type="radio" name="trigger-mode" checked={value.type === "mouse_button"} onchange={() => setMode("mouse_button")} /> Mouse</label>
  </div>
  <div class="modifiers">
    {#each MODIFIERS as mod}
      <label>
        <input
          type="checkbox"
          checked={value.modifiers.includes(mod)}
          onchange={() => toggle(mod)}
        />
        {inputLabel(mod)}
      </label>
    {/each}
  </div>
  {#if value.type === "hotkey"}
    <div class="key-row">
      <select onchange={changeKey} value={value.key}>
        {#each KEY_OPTIONS as k}<option value={k}>{inputLabel(k)}</option>{/each}
      </select>
      <button onclick={startCapture} title="Press a key combo to bind">🎯 Capture</button>
    </div>
  {:else}
    <div class="key-row">
      <select onchange={changeMouseButton} value={value.button}>
        {#each MOUSE_BUTTON_OPTIONS as b}<option value={b}>{inputLabel(b)}</option>{/each}
      </select>
      <button onclick={startMouseCapture} title="Click a button to bind">🎯 Capture</button>
    </div>
  {/if}
{/if}

<style>
  .mode-row {
    display: flex;
    gap: 1rem;
    margin-bottom: 0.5rem;
  }
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

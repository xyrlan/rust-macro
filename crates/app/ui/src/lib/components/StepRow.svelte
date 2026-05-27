<script lang="ts">
  import type { StepDto, KeyCode, MouseButton, MoveModeDto } from "../types";
  import { inputLabel } from "../types";

  let {
    step,
    index,
    canMoveUp,
    canMoveDown,
    onChange,
    onMoveUp,
    onMoveDown,
    onRemove,
  }: {
    step: StepDto;
    index: number;
    canMoveUp: boolean;
    canMoveDown: boolean;
    onChange: (s: StepDto) => void;
    onMoveUp: () => void;
    onMoveDown: () => void;
    onRemove: () => void;
  } = $props();

  // Same KEY_OPTIONS subset as HotkeyPicker — extend if you need more keys.
  const KEY_OPTIONS: KeyCode[] = [
    "f1", "f2", "f3", "f4", "f5", "f6", "f7", "f8", "f9", "f10", "f11", "f12",
    "a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l", "m",
    "n", "o", "p", "q", "r", "s", "t", "u", "v", "w", "x", "y", "z",
    "num0", "num1", "num2", "num3", "num4",
    "num5", "num6", "num7", "num8", "num9",
    "space", "enter", "tab", "escape", "backspace", "caps_lock",
    "up", "down", "left", "right",
    "l_shift", "r_shift", "l_ctrl", "r_ctrl", "l_alt", "r_alt", "l_win", "r_win",
  ];
  const MOUSE_BUTTONS: MouseButton[] = ["left", "right", "middle", "x1", "x2"];
  const MOVE_MODES: MoveModeDto[] = ["absolute", "relative"];

  function update(patch: Partial<StepDto>) {
    onChange({ ...step, ...patch } as StepDto);
  }

  function intInput(value: number, set: (n: number) => void) {
    return (e: Event) => set(Math.max(0, Number((e.target as HTMLInputElement).value) | 0));
  }
</script>

<div class="row">
  <div class="num">#{index + 1}</div>
  <div class="move">
    <button onclick={onMoveUp} disabled={!canMoveUp} title="Move up">↑</button>
    <button onclick={onMoveDown} disabled={!canMoveDown} title="Move down">↓</button>
  </div>
  <div class="type-label">{step.type.split("_").map(p => p[0].toUpperCase() + p.slice(1)).join(" ")}</div>
  <div class="params">
    {#if step.type === "key_press"}
      <label>key
        <select value={step.key} onchange={(e) => update({ key: (e.target as HTMLSelectElement).value as KeyCode })}>
          {#each KEY_OPTIONS as k}<option value={k}>{inputLabel(k)}</option>{/each}
        </select>
      </label>
      <label>hold_ms
        <input type="number" min="0" value={step.hold_ms} oninput={intInput(step.hold_ms, n => update({ hold_ms: n }))} />
      </label>
    {:else if step.type === "key_down" || step.type === "key_up"}
      <label>key
        <select value={step.key} onchange={(e) => update({ key: (e.target as HTMLSelectElement).value as KeyCode })}>
          {#each KEY_OPTIONS as k}<option value={k}>{inputLabel(k)}</option>{/each}
        </select>
      </label>
    {:else if step.type === "mouse_click"}
      <label>button
        <select value={step.button} onchange={(e) => update({ button: (e.target as HTMLSelectElement).value as MouseButton })}>
          {#each MOUSE_BUTTONS as b}<option value={b}>{inputLabel(b)}</option>{/each}
        </select>
      </label>
      <label>hold_ms
        <input type="number" min="0" value={step.hold_ms} oninput={intInput(step.hold_ms, n => update({ hold_ms: n }))} />
      </label>
    {:else if step.type === "mouse_move"}
      <label>x
        <input type="number" value={step.to.x} oninput={(e) => update({ to: { ...step.to, x: Number((e.target as HTMLInputElement).value) | 0 } })} />
      </label>
      <label>y
        <input type="number" value={step.to.y} oninput={(e) => update({ to: { ...step.to, y: Number((e.target as HTMLInputElement).value) | 0 } })} />
      </label>
      <label>mode
        <select value={step.mode} onchange={(e) => update({ mode: (e.target as HTMLSelectElement).value as MoveModeDto })}>
          {#each MOVE_MODES as m}<option value={m}>{inputLabel(m)}</option>{/each}
        </select>
      </label>
    {:else if step.type === "mouse_scroll"}
      <label>delta
        <input type="number" value={step.delta} oninput={(e) => update({ delta: Number((e.target as HTMLInputElement).value) | 0 })} />
      </label>
    {:else if step.type === "wait"}
      <label>min_ms
        <input type="number" min="0" value={step.min_ms} oninput={intInput(step.min_ms, n => update({ min_ms: n }))} />
      </label>
      <label>max_ms
        <input type="number" min="0" value={step.max_ms} oninput={intInput(step.max_ms, n => update({ max_ms: n }))} />
      </label>
    {/if}
  </div>
  <button class="danger remove" onclick={onRemove} title="Delete step">✕</button>
</div>

<style>
  .row {
    display: grid;
    grid-template-columns: 2.5rem auto 9rem 1fr auto;
    gap: 0.5rem;
    align-items: center;
    padding: 0.4rem 0.5rem;
    border-bottom: 1px solid var(--border);
  }
  .num { color: var(--text-muted); }
  .move { display: flex; gap: 0.25rem; }
  .move button { padding: 0.2rem 0.4rem; }
  .type-label {
    font-family: ui-monospace, "Cascadia Code", "Consolas", monospace;
    font-size: 0.85rem;
    color: var(--text-muted);
  }
  .params {
    display: flex;
    gap: 0.6rem;
    flex-wrap: wrap;
    align-items: center;
  }
  .params label {
    display: flex;
    flex-direction: column;
    gap: 0.15rem;
    font-size: 0.75rem;
    color: var(--text-muted);
  }
  .params input, .params select {
    padding: 0.2rem 0.4rem;
    font-size: 0.85rem;
    min-width: 4rem;
  }
  .remove { padding: 0.25rem 0.5rem; }
</style>

<script lang="ts">
  import type { MacroDto } from "../types";
  import { triggerLabel } from "../types";

  let {
    macro,
    onPlay,
    onEdit,
    onDelete,
  }: {
    macro: MacroDto;
    onPlay: (id: string) => void;
    onEdit: (id: string) => void;
    onDelete: (id: string) => void;
  } = $props();

  function modeLabel(macro: MacroDto): string {
    switch (macro.playback.type) {
      case "once": return "Once";
      case "repeat": return `Repeat(${macro.playback.value})`;
      case "loop": return "Loop";
      case "toggle": return "Toggle";
    }
  }

  function confirmDelete() {
    if (confirm(`Delete macro "${macro.name}"? This cannot be undone.`)) {
      onDelete(macro.id);
    }
  }
</script>

<tr>
  <td>{macro.name}</td>
  <td><code>{triggerLabel(macro.trigger)}</code></td>
  <td>{modeLabel(macro)}</td>
  <td class="num">{macro.step_count}</td>
  <td class="actions">
    <button onclick={() => onPlay(macro.id)} title="Play">▶</button>
    <button onclick={() => onEdit(macro.id)} title="Edit">✎</button>
    <button onclick={confirmDelete} class="danger" title="Delete">✕</button>
  </td>
</tr>

<style>
  td {
    padding: 0.6rem 0.5rem;
    border-bottom: 1px solid var(--border);
  }
  .num {
    text-align: right;
    font-variant-numeric: tabular-nums;
  }
  .actions {
    text-align: right;
    white-space: nowrap;
  }
  .actions button {
    margin-left: 0.25rem;
    padding: 0.25rem 0.5rem;
  }
</style>

<script lang="ts">
  import { macros, loading, remove } from "../stores/macros";
  import MacroRow from "./MacroRow.svelte";

  let {
    onPlay,
    onEdit,
    onRecord,
    onSettings,
  }: {
    onPlay: (id: string) => void;
    onEdit: (id: string) => void;
    onRecord: () => void;
    onSettings: () => void;
  } = $props();

  function handleDelete(id: string) {
    void remove(id);
  }
</script>

<section>
  <div class="header">
    <h2>Macros</h2>
    <div class="header-actions">
      <button class="icon" onclick={onSettings} title="Settings">⚙</button>
      <button class="primary" onclick={onRecord}>+ Record new</button>
    </div>
  </div>

  {#if $loading}
    <p class="empty">Loading…</p>
  {:else if $macros.length === 0}
    <p class="empty">
      No macros yet. Click "+ Record new" to capture one.
    </p>
  {:else}
    <table>
      <thead>
        <tr>
          <th>Name</th>
          <th>Hotkey</th>
          <th>Mode</th>
          <th class="num">Steps</th>
          <th class="actions">Actions</th>
        </tr>
      </thead>
      <tbody>
        {#each $macros as macro (macro.id)}
          <MacroRow
            {macro}
            {onPlay}
            {onEdit}
            onDelete={handleDelete}
          />
        {/each}
      </tbody>
    </table>
  {/if}
</section>

<style>
  .header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: 1rem;
  }
  h2 {
    margin: 0;
    font-size: 1.25rem;
  }
  .empty {
    color: var(--text-muted);
    padding: 2rem 0;
    text-align: center;
  }
  table { width: 100%; border-collapse: collapse; }
  th {
    text-align: left;
    padding: 0.5rem;
    border-bottom: 1px solid var(--border);
    color: var(--text-muted);
    font-weight: 500;
    font-size: 0.85rem;
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }
  .num { text-align: right; }
  .actions { text-align: right; }
  .header-actions { display: flex; gap: 0.5rem; align-items: center; }
  .icon { background: transparent; padding: 0.4rem 0.5rem; font-size: 1.1rem; }
</style>

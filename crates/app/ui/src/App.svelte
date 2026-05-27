<script lang="ts">
  import { onMount } from "svelte";
  import { loadAll, snapshot } from "./lib/stores/macros";
  import MacroTable from "./lib/components/MacroTable.svelte";
  import EditMetadataModal from "./lib/components/EditMetadataModal.svelte";
  import ToastHost from "./lib/components/ToastHost.svelte";
  import type { MacroDto } from "./lib/types";

  let editing = $state<MacroDto | null>(null);

  function handlePlay(_id: string) {
    // Wired up in Task 13.
  }

  function handleEdit(id: string) {
    const m = snapshot().find((x) => x.id === id);
    if (m) editing = m;
  }

  onMount(() => {
    void loadAll();
  });
</script>

<main>
  <header>
    <h1>rust-macro</h1>
  </header>
  <MacroTable onPlay={handlePlay} onEdit={handleEdit} />
  {#if editing}
    <EditMetadataModal macro={editing} onClose={() => (editing = null)} />
  {/if}
  <ToastHost />
</main>

<style>
  main {
    max-width: 960px;
    margin: 0 auto;
    padding: 2rem 1.5rem;
  }
  header {
    margin-bottom: 1.5rem;
  }
  h1 {
    margin: 0;
    font-size: 1.5rem;
    font-weight: 600;
  }
</style>

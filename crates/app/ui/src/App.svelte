<script lang="ts">
  import { onMount } from "svelte";
  import { loadAll } from "./lib/stores/macros";
  import MacroTable from "./lib/components/MacroTable.svelte";
  import ToastHost from "./lib/components/ToastHost.svelte";

  // EditMetadataModal hook-up lands in Task 11. For now, edit shows a toast.
  function handlePlay(_id: string) {
    // Wired up in Task 13.
  }
  function handleEdit(_id: string) {
    // Wired up in Task 11.
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

<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { loadAll, snapshot } from "./lib/stores/macros";
  import { play, startListening, stopListening } from "./lib/stores/playback";
  import MacroTable from "./lib/components/MacroTable.svelte";
  import EditMetadataModal from "./lib/components/EditMetadataModal.svelte";
  import PlaybackBanner from "./lib/components/PlaybackBanner.svelte";
  import ToastHost from "./lib/components/ToastHost.svelte";
  import type { MacroDto } from "./lib/types";

  let editing = $state<MacroDto | null>(null);

  function handlePlay(id: string) {
    void play(id);
  }

  function handleEdit(id: string) {
    const m = snapshot().find((x) => x.id === id);
    if (m) editing = m;
  }

  onMount(() => {
    void loadAll();
    void startListening();
  });

  onDestroy(() => {
    void stopListening();
  });
</script>

<main>
  <header>
    <h1>rust-macro</h1>
  </header>
  <MacroTable onPlay={handlePlay} onEdit={handleEdit} />
  <PlaybackBanner />
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

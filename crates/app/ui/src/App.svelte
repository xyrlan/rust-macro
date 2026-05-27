<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { loadAll } from "./lib/stores/macros";
  import { play, startListening as startPlaybackListening, stopListening as stopPlaybackListening } from "./lib/stores/playback";
  import { arm as armRecording, startListening as startRecordingListening, stopListening as stopRecordingListening } from "./lib/stores/recording";
  import MacroTable from "./lib/components/MacroTable.svelte";
  import StepEditor from "./lib/components/StepEditor.svelte";
  import SettingsView from "./lib/components/SettingsView.svelte";
  import RecordingModal from "./lib/components/RecordingModal.svelte";
  import PlaybackBanner from "./lib/components/PlaybackBanner.svelte";
  import ToastHost from "./lib/components/ToastHost.svelte";

  type View =
    | { tag: "list" }
    | { tag: "editor"; macroId: string }
    | { tag: "settings" };
  let view = $state<View>({ tag: "list" });

  function handlePlay(id: string) { void play(id); }
  function handleEdit(id: string) { view = { tag: "editor", macroId: id }; }
  function handleRecord() { armRecording(); }
  function handleSettings() { view = { tag: "settings" }; }
  function backToList() { view = { tag: "list" }; }

  onMount(() => {
    void loadAll();
    void startPlaybackListening();
    void startRecordingListening();
  });

  onDestroy(() => {
    void stopPlaybackListening();
    void stopRecordingListening();
  });
</script>

{#if view.tag === "list"}
  <main>
    <header>
      <h1>rust-macro</h1>
    </header>
    <MacroTable onPlay={handlePlay} onEdit={handleEdit} onRecord={handleRecord} onSettings={handleSettings} />
    <PlaybackBanner />
    <RecordingModal />
    <ToastHost />
  </main>
{:else if view.tag === "editor"}
  <StepEditor macroId={view.macroId} onBack={backToList} />
  <ToastHost />
{:else if view.tag === "settings"}
  <SettingsView onBack={backToList} />
  <ToastHost />
{/if}

<style>
  main {
    max-width: 960px;
    margin: 0 auto;
    padding: 2rem 1.5rem;
  }
  header { margin-bottom: 1.5rem; }
  h1 { margin: 0; font-size: 1.5rem; font-weight: 600; }
</style>

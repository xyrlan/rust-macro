<script lang="ts">
  import { toasts } from "../stores/toast";
  import Toast from "./Toast.svelte";
</script>

<div class="host" aria-live="polite">
  {#each $toasts as entry (entry.id)}
    <Toast {entry} />
  {/each}
</div>

<style>
  .host {
    position: fixed;
    top: 1rem;
    right: 1rem;
    z-index: 1000;
    display: flex;
    flex-direction: column;
    pointer-events: none;
  }
  .host :global(.toast) {
    pointer-events: auto;
  }
</style>

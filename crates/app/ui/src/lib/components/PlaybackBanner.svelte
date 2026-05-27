<script lang="ts">
  import { active, stop } from "../stores/playback";

  let elapsedMs = $state(0);
  let timer: ReturnType<typeof setInterval> | null = null;

  $effect(() => {
    const current = $active;
    if (current) {
      elapsedMs = Date.now() - current.startedAt;
      timer = setInterval(() => {
        elapsedMs = Date.now() - current.startedAt;
      }, 250);
    } else {
      if (timer) {
        clearInterval(timer);
        timer = null;
      }
      elapsedMs = 0;
    }
    return () => {
      if (timer) clearInterval(timer);
    };
  });

  function formatElapsed(ms: number): string {
    const s = Math.floor(ms / 1000);
    return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
  }
</script>

{#if $active}
  <div class="banner" role="status">
    <span class="icon">▶</span>
    <span class="text">
      Playing <strong>{$active.macroName}</strong>
      · {formatElapsed(elapsedMs)}
    </span>
    <button class="danger" onclick={() => void stop()}>■ Stop</button>
  </div>
{/if}

<style>
  .banner {
    position: sticky;
    bottom: 0;
    background: rgba(34, 197, 94, 0.1);
    border: 1px solid var(--success);
    border-left-width: 4px;
    padding: 0.75rem 1rem;
    border-radius: 4px;
    display: flex;
    align-items: center;
    gap: 0.75rem;
    margin-top: 1.5rem;
  }
  .icon {
    color: var(--success);
    font-size: 1.1rem;
  }
  .text {
    flex: 1;
    color: var(--text);
  }
  strong {
    color: var(--text);
  }
</style>

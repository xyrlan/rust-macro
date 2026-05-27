<script lang="ts">
  import { driver, restartNow } from "../stores/driver";

  let shown = $state(false);
  let lastPending = $state(false);

  $effect(() => {
    // Detect transition false → true; show modal once per transition.
    if ($driver.pending_reboot && !lastPending) {
      shown = true;
    }
    lastPending = $driver.pending_reboot;
  });

  function close() { shown = false; }
  async function later() {
    close();
    // We don't clear the marker — the banner stays up.
  }
  async function now() {
    await restartNow();
    close();
  }
</script>

{#if shown}
  <div class="backdrop" role="presentation">
    <div class="modal" role="dialog" aria-labelledby="reboot-title">
      <h3 id="reboot-title">Restart required</h3>
      <p>
        The Interception driver was installed. Windows must restart before
        the driver becomes active and rust-macro can capture input.
      </p>
      <p class="small">Restart will begin in 10 seconds after you click "Restart now".</p>
      <div class="actions">
        <button onclick={later}>Restart later</button>
        <button class="primary" onclick={() => void now()}>Restart now</button>
      </div>
    </div>
  </div>
{/if}

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.6);
    display: flex; align-items: center; justify-content: center;
    z-index: 700;
  }
  .modal {
    background: var(--bg-elevated);
    border: 1px solid var(--border);
    border-radius: 8px;
    padding: 1.5rem;
    max-width: 480px;
    box-shadow: 0 16px 48px rgba(0, 0, 0, 0.5);
  }
  h3 { margin: 0 0 0.75rem 0; }
  p { margin: 0 0 0.75rem 0; }
  .small { color: var(--text-muted); font-size: 0.85rem; }
  .actions { display: flex; justify-content: flex-end; gap: 0.5rem; margin-top: 1.25rem; }
</style>

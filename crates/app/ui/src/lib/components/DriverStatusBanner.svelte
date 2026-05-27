<script lang="ts">
  import { driver, install, dismissPending } from "../stores/driver";

  let installing = $state(false);
  async function onInstall() {
    installing = true;
    try { await install(); } finally { installing = false; }
  }
</script>

{#if $driver.pending_reboot}
  <div class="banner banner-warning">
    <span>⚠ Driver install pending — restart Windows to activate Interception.</span>
    <button onclick={() => void dismissPending()} title="Hide this banner">✕</button>
  </div>
{:else if $driver.status === "not_installed"}
  <div class="banner banner-error">
    <span>❌ Interception driver not installed. Hotkeys and playback won't work.</span>
    <button class="primary" disabled={installing} onclick={() => void onInstall()}>
      {installing ? "Installing…" : "Install (admin)"}
    </button>
  </div>
{:else if $driver.status === "installed_not_running"}
  <div class="banner banner-warning">
    <span>⚠ Interception driver installed but not running. Restart Windows to activate.</span>
  </div>
{/if}

<style>
  .banner {
    display: flex;
    gap: 0.75rem;
    align-items: center;
    padding: 0.5rem 1rem;
    font-size: 0.9rem;
    border-bottom: 1px solid var(--border);
  }
  .banner span { flex: 1; }
  .banner-error {
    background: rgba(220, 38, 38, 0.15);
    color: #fca5a5;
  }
  .banner-warning {
    background: rgba(202, 138, 4, 0.15);
    color: #fde68a;
  }
  button { padding: 0.25rem 0.6rem; }
</style>

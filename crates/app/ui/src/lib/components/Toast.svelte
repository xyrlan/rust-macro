<script lang="ts">
  import type { ToastEntry } from "../stores/toast";
  import { dismiss } from "../stores/toast";

  let { entry }: { entry: ToastEntry } = $props();

  const colors: Record<ToastEntry["level"], string> = {
    info: "var(--text-muted)",
    success: "var(--success)",
    warning: "var(--warning)",
    error: "var(--danger)",
  };
</script>

<div
  class="toast"
  style:border-left-color={colors[entry.level]}
  role="status"
>
  <span class="message">{entry.message}</span>
  <button class="close" onclick={() => dismiss(entry.id)} aria-label="Dismiss">×</button>
</div>

<style>
  .toast {
    background: var(--bg-elevated);
    border: 1px solid var(--border);
    border-left-width: 4px;
    padding: 0.75rem 1rem;
    border-radius: 4px;
    margin-bottom: 0.5rem;
    display: flex;
    align-items: center;
    gap: 0.75rem;
    min-width: 280px;
    max-width: 420px;
    box-shadow: 0 4px 12px rgba(0, 0, 0, 0.4);
  }
  .message {
    flex: 1;
    line-height: 1.4;
  }
  .close {
    background: transparent;
    border: none;
    color: var(--text-muted);
    font-size: 1.25rem;
    line-height: 1;
    padding: 0 0.25rem;
    cursor: pointer;
  }
  .close:hover {
    color: var(--text);
  }
</style>

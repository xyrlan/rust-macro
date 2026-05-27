import { writable, get } from "svelte/store";
import type { WireError } from "../types";
import { isWireError } from "../types";

export type ToastLevel = "info" | "success" | "warning" | "error";

export type ToastEntry = {
  id: number;
  level: ToastLevel;
  message: string;
  persistent: boolean;
};

let nextId = 1;
export const toasts = writable<ToastEntry[]>([]);

export function pushToast(
  level: ToastLevel,
  message: string,
  persistent = false,
): number {
  const id = nextId++;
  toasts.update((list) => [...list, { id, level, message, persistent }]);
  if (!persistent) {
    setTimeout(() => dismiss(id), 4000);
  }
  return id;
}

export function dismiss(id: number): void {
  toasts.update((list) => list.filter((t) => t.id !== id));
}

export function clear(): void {
  toasts.set([]);
}

/** Map a thrown command error to a toast. Errors that aren't WireError
 *  surface as an "Other" red toast with the raw message. */
export function reportError(e: unknown): void {
  if (isWireError(e)) {
    handleWireError(e);
    return;
  }
  const message = e instanceof Error ? e.message : String(e);
  pushToast("error", message);
}

function handleWireError(e: WireError): void {
  switch (e.kind) {
    case "DriverNotInstalled":
      pushToast("error", "Interception driver not installed. (Install flow lands in 3b.)", true);
      break;
    case "DriverNotRunning":
      pushToast("error", "Interception driver installed but not running. Reboot may be required.", true);
      break;
    case "PlaybackActive":
      pushToast("warning", "Already playing — stop it first.");
      break;
    case "MacroNotFound":
      pushToast("info", "That macro no longer exists; refreshing the list.");
      break;
    default:
      pushToast("error", `${e.kind}: ${e.message}`);
  }
}

// Test-only export — Vitest tests reset state between cases.
export function _testReset(): void {
  nextId = 1;
  toasts.set([]);
}

// Avoid unused warning in production builds when get isn't used elsewhere.
export const _peek = () => get(toasts);

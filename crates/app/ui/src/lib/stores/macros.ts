import { writable, get } from "svelte/store";
import type { MacroDto, Trigger, PlaybackMode } from "../types";
import * as api from "../api";
import { reportError } from "./toast";

export const macros = writable<MacroDto[]>([]);
export const loading = writable<boolean>(false);

export async function loadAll(): Promise<void> {
  loading.set(true);
  try {
    const list = await api.loadMacros();
    macros.set(list);
  } catch (e) {
    reportError(e);
  } finally {
    loading.set(false);
  }
}

export async function remove(id: string): Promise<void> {
  try {
    await api.deleteMacro(id);
    macros.update((list) => list.filter((m) => m.id !== id));
  } catch (e) {
    reportError(e);
    // The macro may have been deleted externally — reload to converge.
    await loadAll();
  }
}

export async function updateMetadata(
  id: string,
  name: string,
  trigger: Trigger,
  playback: PlaybackMode,
): Promise<void> {
  try {
    const updated = await api.updateMacroMetadata(id, name, trigger, playback);
    macros.update((list) => list.map((m) => (m.id === id ? updated : m)));
  } catch (e) {
    reportError(e);
  }
}

// Helper for downstream stores/components — read the current macros snapshot
// without a subscribe roundtrip.
export function snapshot(): MacroDto[] {
  return get(macros);
}

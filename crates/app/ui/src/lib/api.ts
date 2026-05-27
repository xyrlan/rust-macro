// Tauri 2 converts JS argument keys from camelCase to snake_case automatically
// when calling Rust commands. Single-word args (`id`, `name`) are unaffected;
// multi-word args added later (e.g. `macroId`) must use camelCase here and
// snake_case in the Rust signature. Keep this in mind when adding commands.

import { invoke } from "@tauri-apps/api/core";
import type { MacroDto, Trigger, PlaybackMode } from "./types";

export async function loadMacros(): Promise<MacroDto[]> {
  return invoke<MacroDto[]>("load_macros");
}

export async function deleteMacro(id: string): Promise<void> {
  await invoke("delete_macro", { id });
}

// Stubs for commands added in later tasks. Frontend uses them in Task 11+
// once they're implemented in the backend.
export async function updateMacroMetadata(
  id: string,
  name: string,
  trigger: Trigger,
  playback: PlaybackMode,
): Promise<MacroDto> {
  return invoke<MacroDto>("update_macro_metadata", { id, name, trigger, playback });
}

export async function playMacro(id: string): Promise<void> {
  await invoke("play_macro", { id });
}

export async function stopPlayback(): Promise<void> {
  await invoke("stop_playback");
}

// Tauri 2 converts JS argument keys from camelCase to snake_case automatically
// when calling Rust commands. Single-word args (`id`, `name`) are unaffected;
// multi-word args added later (e.g. `macroId`) must use camelCase here and
// snake_case in the Rust signature. Keep this in mind when adding commands.

import { invoke } from "@tauri-apps/api/core";
import type { MacroDto, Trigger, PlaybackMode, StepDto, SettingsDto } from "./types";

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

export async function createMacro(
  name: string,
  trigger: Trigger,
  playback: PlaybackMode,
  steps: StepDto[],
): Promise<MacroDto> {
  return invoke<MacroDto>("create_macro", { name, trigger, playback, steps });
}

export async function updateMacroFull(
  id: string,
  name: string,
  trigger: Trigger,
  playback: PlaybackMode,
  steps: StepDto[],
): Promise<MacroDto> {
  return invoke<MacroDto>("update_macro_full", { id, name, trigger, playback, steps });
}

export async function loadMacroSteps(id: string): Promise<StepDto[]> {
  return invoke<StepDto[]>("load_macro_steps", { id });
}

export async function startRecording(): Promise<void> {
  await invoke("start_recording");
}

export async function stopRecording(): Promise<void> {
  await invoke("stop_recording");
}

export async function loadSettings(): Promise<SettingsDto> {
  return invoke<SettingsDto>("load_settings");
}

export async function saveSettings(settings: SettingsDto): Promise<void> {
  await invoke("save_settings", { settings });
}

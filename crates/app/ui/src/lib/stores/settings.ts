import { writable } from "svelte/store";
import type { SettingsDto, KeyCode } from "../types";
import * as api from "../api";
import { reportError, pushToast } from "./toast";

const DEFAULT_SETTINGS: SettingsDto = {
  stop_key: "f10",
  storage_root_override: null,
};

export const settings = writable<SettingsDto>(DEFAULT_SETTINGS);

export async function load(): Promise<void> {
  try {
    const s = await api.loadSettings();
    settings.set(s);
  } catch (e) {
    reportError(e);
  }
}

export async function save(s: SettingsDto): Promise<void> {
  try {
    await api.saveSettings(s);
    settings.set(s);
    pushToast("info", "Settings saved.");
  } catch (e) {
    reportError(e);
  }
}

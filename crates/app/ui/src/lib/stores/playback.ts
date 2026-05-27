import { writable } from "svelte/store";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { WireError } from "../types";
import * as api from "../api";
import { reportError, pushToast } from "./toast";

export type ActivePlayback = {
  macroId: string;
  macroName: string;
  startedAt: number;
};

export const active = writable<ActivePlayback | null>(null);

type StartedPayload = { macro_id: string; macro_name: string };
type FinishedOutcome =
  | { status: "ok" }
  | { status: "stopped" }
  | { status: "failed"; error: WireError };

type FinishedPayload = { macro_id: string; outcome: FinishedOutcome };

let unlisteners: UnlistenFn[] = [];

export async function startListening(): Promise<void> {
  // Idempotent — calling twice is harmless because we tear down first.
  await stopListening();

  unlisteners.push(
    await listen<StartedPayload>("playback_started", (event) => {
      active.set({
        macroId: event.payload.macro_id,
        macroName: event.payload.macro_name,
        startedAt: Date.now(),
      });
    }),
  );

  unlisteners.push(
    await listen<FinishedPayload>("playback_finished", (event) => {
      const { outcome } = event.payload;
      switch (outcome.status) {
        case "ok":
          pushToast("success", "Playback finished.");
          break;
        case "stopped":
          pushToast("info", "Playback stopped.");
          break;
        case "failed":
          pushToast("error", `Playback failed: ${outcome.error.message}`);
          break;
      }
      active.set(null);
    }),
  );
}

export async function stopListening(): Promise<void> {
  for (const u of unlisteners) u();
  unlisteners = [];
}

export async function play(id: string): Promise<void> {
  try {
    await api.playMacro(id);
  } catch (e) {
    reportError(e);
  }
}

export async function stop(): Promise<void> {
  try {
    await api.stopPlayback();
  } catch (e) {
    reportError(e);
  }
}

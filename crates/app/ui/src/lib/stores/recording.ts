import { writable } from "svelte/store";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { StepDto, WireError } from "../types";
import * as api from "../api";
import { reportError, pushToast } from "./toast";

/** Phases of the recording UI:
 *   - `idle`: no recording in progress
 *   - `armed`: user opened the start modal, hasn't clicked Start yet
 *   - `recording`: backend returned OK from start_recording and emitted
 *     recording_started; window minimized; F10 will stop
 *   - `preview`: recording_finished arrived; user is in the Save/Discard modal
 */
export type RecordingPhase =
  | { tag: "idle" }
  | { tag: "armed" }
  | { tag: "recording" }
  | { tag: "preview"; steps: StepDto[] };

export const phase = writable<RecordingPhase>({ tag: "idle" });

type FinishedOutcome =
  | { status: "ok"; steps: StepDto[] }
  | { status: "failed"; error: WireError };

type FinishedPayload = { outcome: FinishedOutcome };

let unlisteners: UnlistenFn[] = [];

export async function startListening(): Promise<void> {
  await stopListening();

  unlisteners.push(
    await listen("recording_started", () => {
      phase.set({ tag: "recording" });
    }),
  );

  unlisteners.push(
    await listen<FinishedPayload>("recording_finished", (event) => {
      const o = event.payload.outcome;
      if (o.status === "ok") {
        phase.set({ tag: "preview", steps: o.steps });
        if (o.steps.length === 0) {
          pushToast("info", "Recording captured 0 steps.");
        }
      } else {
        pushToast("error", `Recording failed: ${o.error.message}`);
        phase.set({ tag: "idle" });
      }
    }),
  );
}

export async function stopListening(): Promise<void> {
  for (const u of unlisteners) u();
  unlisteners = [];
}

/** Open the start modal. */
export function arm(): void {
  phase.set({ tag: "armed" });
}

/** Cancel the start modal (user clicked Cancel before recording started). */
export function disarm(): void {
  phase.set({ tag: "idle" });
}

/** Begin recording: minimize window + call backend. */
export async function begin(): Promise<void> {
  try {
    const w = await import("@tauri-apps/api/window");
    await w.getCurrentWindow().minimize();
    await api.startRecording();
    // `recording_started` event sets phase to "recording".
  } catch (e) {
    reportError(e);
    phase.set({ tag: "idle" });
  }
}

/** Explicitly stop the recording from the frontend (rare — F10 is primary). */
export async function stop(): Promise<void> {
  try {
    await api.stopRecording();
  } catch (e) {
    reportError(e);
  }
}

/** Restore the window after recording_finished arrives. */
export async function restoreWindow(): Promise<void> {
  try {
    const w = await import("@tauri-apps/api/window");
    const win = w.getCurrentWindow();
    await win.unminimize();
    await win.setFocus();
  } catch (e) {
    // Non-critical — user can click the window to focus it.
    console.warn("recording: window restore failed", e);
  }
}

/** Discard the captured steps without saving. */
export function discard(): void {
  phase.set({ tag: "idle" });
}

/** Finalize: caller already saved via api.createMacro; transition to idle. */
export function complete(): void {
  phase.set({ tag: "idle" });
}

import { writable } from "svelte/store";
import type { DriverStateDto } from "../types";
import * as api from "../api";
import { reportError, pushToast } from "./toast";

const DEFAULT: DriverStateDto = { status: "not_installed", pending_reboot: false };

export const driver = writable<DriverStateDto>(DEFAULT);

/** Refresh from the backend. Call on boot + after any install/uninstall. */
export async function refresh(): Promise<void> {
  try {
    const s = await api.driverStatus();
    driver.set(s);
  } catch (e) {
    reportError(e);
  }
}

/** Start install flow — UAC prompt, then writes pending marker. */
export async function install(): Promise<void> {
  try {
    await api.installDriver();
    await refresh();
    pushToast("info", "Driver installed. Restart to activate.");
  } catch (e) {
    reportError(e);
  }
}

export async function uninstall(): Promise<void> {
  try {
    await api.uninstallDriver();
    await refresh();
    pushToast("info", "Driver uninstalled. Restart to complete.");
  } catch (e) {
    reportError(e);
  }
}

export async function dismissPending(): Promise<void> {
  try {
    await api.clearPendingReboot();
    await refresh();
  } catch (e) {
    reportError(e);
  }
}

export async function restartNow(): Promise<void> {
  try {
    await api.rebootWindows();
    pushToast("info", "Restarting in 10 seconds…");
  } catch (e) {
    reportError(e);
  }
}

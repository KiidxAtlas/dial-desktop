// Keeping Dial current: the app (signed Tauri updates) and the engine (`dial update`).
// Checks run at start and every few hours; nothing installs without the user asking.

import { reactive } from "vue";
import type { Update } from "@tauri-apps/plugin-updater";
import { engine, showToast, snapshot, updatesApi } from "./bridge";
import type { EngineUpdateView } from "./types";

const RECHECK_MS = 6 * 60 * 60 * 1000;

export const updates = reactive({
  /** The app's own version. */
  appVersion: "",
  /** Whether this build can update itself (release builds only). */
  appEnabled: false,
  /** A newer app release: its version and notes. */
  app: null as { version: string; notes: string } | null,
  appChecking: false,
  /** 0..1 while downloading, `null` when the size is unknown; `undefined` when idle. */
  appProgress: undefined as number | null | undefined,
  appError: "",

  engine: null as EngineUpdateView | null,
  engineChecking: false,
  /** What the engine button is doing. */
  engineBusy: "" as "" | "updating" | "installing",
  /** Output of the last engine install or update, for the Updates page. */
  engineLog: "",
  engineError: "",

  /** The banner was closed for the updates it showed. */
  dismissed: "",
});

let pendingApp: Update | null = null;

/** Whether the engine reports a newer release (or predates checks, so updating adds them). */
export function engineUpdateAvailable(view = updates.engine): boolean {
  return !!view && view.state === "checked" && view.status.update_available;
}

/** Text that identifies what the banner is announcing, so closing it hides only these. */
export function bannerKey(): string {
  const parts = [];
  if (updates.app) parts.push(`app ${updates.app.version}`);
  if (engineUpdateAvailable()) parts.push(`engine ${updates.engine?.state === "checked" ? updates.engine.status.latest : ""}`);
  return parts.join(" · ");
}

export async function checkApp() {
  if (!updates.appEnabled || updates.appChecking) return;
  updates.appChecking = true;
  updates.appError = "";
  try {
    pendingApp = await updatesApi.checkApp();
    updates.app = pendingApp ? { version: pendingApp.version, notes: pendingApp.body ?? "" } : null;
  } catch (error) {
    updates.appError = String(error);
  } finally {
    updates.appChecking = false;
  }
}

/** Download and install the app update, then restart into it. */
export async function installApp() {
  if (!pendingApp || updates.appProgress !== undefined) return;
  updates.appError = "";
  updates.appProgress = 0;
  try {
    await updatesApi.installApp(pendingApp, (fraction) => (updates.appProgress = fraction));
  } catch (error) {
    updates.appError = String(error);
    updates.appProgress = undefined;
  }
}

export async function checkEngine() {
  if (updates.engineChecking || snapshot.value?.connection.state === "preview") return;
  updates.engineChecking = true;
  try {
    updates.engine = await updatesApi.checkEngine();
  } catch (error) {
    updates.engine = { state: "failed", error: String(error) };
  } finally {
    updates.engineChecking = false;
  }
}

/** Run `dial update`, then reconnect so the new engine is used (after a running turn, never during). */
export async function updateEngine() {
  if (updates.engineBusy) return;
  updates.engineBusy = "updating";
  updates.engineError = "";
  try {
    updates.engineLog = await updatesApi.updateEngine();
    await checkEngine();
    if (snapshot.value?.turnRunning) showToast("Dial engine updated. Reconnect after this turn to use it.");
    else {
      showToast("Dial engine updated.");
      if (snapshot.value?.workspace) await engine.reconnect();
    }
  } catch (error) {
    updates.engineError = String(error);
  } finally {
    updates.engineBusy = "";
  }
}

/** Install the engine with Dial's signed installer; the app connects to it when done. */
export async function installEngine() {
  if (updates.engineBusy) return;
  updates.engineBusy = "installing";
  updates.engineError = "";
  try {
    updates.engineLog = await updatesApi.installEngine();
    showToast("Dial engine installed.");
    await checkEngine();
  } catch (error) {
    updates.engineError = String(error);
    showToast("Could not install the Dial engine; see Settings › Updates.");
  } finally {
    updates.engineBusy = "";
  }
}

let started = false;

/** Check now and every few hours. Safe to call more than once. */
export async function startUpdateChecks() {
  if (started) return;
  started = true;
  updates.appVersion = await updatesApi.appVersion().catch(() => "");
  updates.appEnabled = await updatesApi.appEnabled().catch(() => false);
  const run = () => Promise.all([checkApp(), checkEngine()]);
  await run();
  setInterval(run, RECHECK_MS);
}

// Keeping Dial current: the app (signed Tauri updates) and the engine (`dial update`).
// Checks run at start and every few hours; nothing installs without the user asking.

import { reactive } from "vue";
import { ask } from "@tauri-apps/plugin-dialog";
import type { Update } from "@tauri-apps/plugin-updater";
import { engine, showToast, snapshot, updatesApi } from "./bridge";
import type { EngineProblem, EngineUpdateView } from "./types";
import { createAppInstaller, restartBlocked } from "./appInstaller";

const RECHECK_MS = 6 * 60 * 60 * 1000;
const installer = createAppInstaller<Update>(() => restartBlocked(snapshot.value), updatesApi);

/** Both connection failures and update checks can offer a separate, signed engine. */
export function needsEngineInstall(state: EngineProblem | EngineUpdateView["state"] | undefined) {
  return state === "missing" || state === "unlaunchable";
}

export const updates = reactive({
  /** The app's own version. */
  appVersion: "",
  /** Whether this build can update itself (release builds only). */
  appEnabled: false,
  /** A newer app release: its version and notes. */
  app: null as { version: string; notes: string } | null,
  appChecking: false,
  appPhase: installer.phase,
  appStatus: installer.status,
  appAction: installer.label,
  appError: "",

  engine: null as EngineUpdateView | null,
  engineChecking: false,
  engineChanged: false,
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
  if (!updates.appEnabled || updates.appChecking || updates.appPhase !== "idle" || installer.installed.value) return;
  updates.appChecking = true;
  updates.appError = "";
  try {
    const old = pendingApp;
    pendingApp = await updatesApi.checkApp();
    if (old) void old.close().catch(() => {});
    if (pendingApp?.version !== old?.version) installer.error.value = "";
    updates.app = pendingApp ? { version: pendingApp.version, notes: pendingApp.body ?? "" } : null;
  } catch (error) {
    updates.appError = String(error);
  } finally {
    updates.appChecking = false;
  }
}

/** Download and install the app update, then restart into it. */
export async function installApp() {
  if (!pendingApp || updates.appChecking || updates.engineBusy) return;
  await installer.run(pendingApp);
}

let engineCheckId = 0;

export async function checkEngine() {
  if (updates.engineBusy || updates.engineChecking || snapshot.value?.connection.state === "preview") return;
  await refreshEngineStatus();
}

async function refreshEngineStatus() {
  const id = ++engineCheckId;
  updates.engineChecking = true;
  try {
    const result = await updatesApi.checkEngine();
    if (id === engineCheckId) updates.engine = result;
  } catch (error) {
    if (id === engineCheckId) updates.engine = { state: "failed", error: String(error) };
  } finally {
    if (id === engineCheckId) updates.engineChecking = false;
  }
}

async function finishEngineChange(message: string) {
  updates.engineChanged = true;
  // Supersede any check of the previous engine that was still in flight.
  await refreshEngineStatus();
  if (snapshot.value?.workspace) {
    if (snapshot.value.turnRunning || !(await engine.reconnect())) {
      showToast(`${message} Use Reconnect this session in Settings › Updates when idle.`);
      return;
    }
  }
  showToast(message);
}

/** Run `dial update`, then reconnect so the new engine is used (after a running turn, never during). */
export async function updateEngine() {
  if (updates.engineBusy || updates.appPhase !== "idle") return;
  if (updates.engine?.state === "checked" && !updates.engine.status.can_update) return installEngine();
  updates.engineBusy = "updating";
  updates.engineError = "";
  updates.engineLog = "";
  try {
    updates.engineLog = await updatesApi.updateEngine();
    await finishEngineChange("Engine update command finished.");
  } catch (error) {
    updates.engineError = String(error);
    showToast("Could not update the Dial engine; see Settings › Updates.");
  } finally {
    updates.engineBusy = "";
  }
}

/** Install a separately managed copy only after the user agrees to switch engines. */
export async function installEngine() {
  if (updates.engineBusy || updates.appPhase !== "idle") return;
  updates.engineBusy = "installing";
  updates.engineError = "";
  updates.engineLog = "";
  try {
    const confirmed = await ask("Desktop will install a signed Dial release in its own folder and select it for new connections. Your existing engine will not be replaced. Desktop requests no PATH changes, but older Windows installers may add the folder to your user PATH. An explicit engine override still wins on the next launch.", {
      title: "Install a separate Dial engine?", okLabel: "Install Desktop copy", cancelLabel: "Cancel",
    });
    if (!confirmed) return;
    updates.engineLog = await updatesApi.installEngine();
    await finishEngineChange("Dial engine installed for Desktop.");
  } catch (error) {
    updates.engineError = String(error);
    showToast(snapshot.value?.workspace ? "Could not install the Dial engine; see Settings › Updates." : "Could not install the Dial engine. You can retry here.");
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

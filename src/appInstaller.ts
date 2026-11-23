import { computed, ref } from "vue";
import type { Snapshot } from "./types";

/** The selected turn alone isn't enough: restarting also ends background sessions. */
export function restartBlocked(snap: Snapshot | null) {
  return !snap || snap.turnRunning || snap.sessions.some((s) => s.running);
}

type Phase = "idle" | "downloading" | "waiting" | "installing" | "restarting";
export type RestartDecision = { state: "busy" | "reserved" } | { state: "confirmTerminals"; ids: number[] };

/** Keep the signed download separate from the destructive step (install itself exits on Windows). */
export function createAppInstaller<T>(blocked: () => boolean, api: {
  downloadApp(update: T, progress: (fraction: number | null) => void): Promise<void>;
  tryReserveRestart(confirmed: number[]): Promise<RestartDecision>;
  confirmTerminals(count: number): Promise<boolean>;
  releaseRestart(): Promise<void>;
  installApp(update: T): Promise<void>;
  restartApp(): Promise<void>;
}) {
  const phase = ref<Phase>("idle");
  const error = ref("");
  const progress = ref<number | null>(null);
  const installed = ref(false);
  const label = computed(() => {
    if (installed.value) return blocked() ? "Restart after turns finish" : "Retry restart";
    if (blocked()) return "Install and restart after turns finish";
    return error.value ? "Retry install and restart" : "Install and restart";
  });
  const status = computed(() => {
    switch (phase.value) {
      case "downloading": return progress.value === null ? "Downloading…" : `Downloading… ${Math.round(progress.value * 100)}%`;
      case "waiting": return "Waiting to restart: finish active turns and stop voice mode if it is on.";
      case "installing": return "Installing Dial Desktop…";
      case "restarting": return "Restarting Dial Desktop…";
      default: return error.value;
    }
  });
  let needsRelease = false;
  async function run(update: T) {
    if (phase.value !== "idle") return;
    error.value = "";
    let reserved = false;
    try {
      if (needsRelease) {
        phase.value = "waiting";
        await api.releaseRestart();
        needsRelease = false;
      }
      if (!installed.value) {
        progress.value = null;
        phase.value = "downloading";
        await api.downloadApp(update, (p) => { progress.value = p; });
      }
      phase.value = "waiting";
      let confirmed: number[] = [];
      while (!reserved) {
        const decision = await api.tryReserveRestart(confirmed);
        if (decision.state === "confirmTerminals") {
          if (!await api.confirmTerminals(decision.ids.length)) {
            phase.value = "idle";
            return;
          }
          confirmed = decision.ids;
        } else if (decision.state === "busy") {
          // Queued turns aren't necessarily in a snapshot (or associated with a session id yet).
          await new Promise(resolve => setTimeout(resolve, 500));
        } else if (decision.state === "reserved") reserved = true;
        else throw Error("Unrecognized answer to restart request");
      }
      if (!installed.value) {
        phase.value = "installing";
        await api.installApp(update);
        installed.value = true;
      }
      phase.value = "restarting";
      await api.restartApp();
    } catch (cause) {
      error.value = `Could not ${installed.value ? "restart" : "update"} Dial Desktop: ${cause}`;
      if (reserved) {
        try { await api.releaseRestart(); }
        catch (releaseError) {
          needsRelease = true;
          error.value += ` Could not unlock the session: ${releaseError}`;
        }
      }
      phase.value = "idle";
    }
  }
  return { phase, installed, error, label, status, run };
}

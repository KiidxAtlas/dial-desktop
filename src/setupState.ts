// What a new user still has to set up: the welcome checklist (engine, folder, provider) and the
// "Finish setup" card shown in an empty session while no model is ready to answer.
import type { EngineUpdateView, ModelOption } from "./types";

export type StepState = "done" | "next" | "later" | "checking" | "problem";

export interface SetupStep {
  id: "engine" | "folder" | "provider";
  title: string;
  detail: string;
  state: StepState;
}

/** The welcome screen's checklist for the engine check `engine` (`null` until it ran). */
export function welcomeSteps(engine: EngineUpdateView | null, checking: boolean): SetupStep[] {
  let state: StepState;
  let detail: string;
  if (!engine) {
    state = "checking";
    detail = checking ? "Looking for the Dial engine…" : "Not checked yet.";
  } else if (engine.state === "missing") {
    state = "next";
    detail = "Not found on this computer. Install a copy for Desktop below.";
  } else if (engine.state === "failed") {
    state = "problem";
    detail = `Could not check the engine: ${engine.error}`;
  } else {
    state = "done";
    detail = engine.state === "checked" ? `Dial ${engine.status.current} is installed.` : "Installed.";
  }
  const ready = state === "done";
  return [
    { id: "engine", title: "Dial engine", detail, state },
    { id: "folder", title: "Open a project folder", detail: "Dial works on the code in that folder.", state: ready || state === "problem" ? "next" : "later" },
    { id: "provider", title: "Connect a model", detail: "Then sign in to a provider or add an API key in Settings › Providers, or download a local model.", state: "later" },
  ];
}

type Selection = Pick<ModelOption, 'provider' | 'modelId'> | null;
/** Inventory is evidence of configuration, not a successful inference request. */
export function selectedRoute(selected: Selection, models: ModelOption[]) {
  if (!selected) return 'No model selected';
  if (!selected.provider || !models.length) return 'Model inventory unavailable';
  const route = models.find(m => m.modelId === selected.modelId && m.provider === selected.provider);
  if (route?.local) return 'Downloaded locally';
  if (route?.listed) return 'Listed by provider';
  if (route?.configured) return 'Configured';
  return 'Setup not confirmed';
}
export interface RequestObservation { provider: string; model: string; succeeded: boolean }
export function lastRequestLabel(selected: Selection, request?: RequestObservation | null): string {
  if (!request || !selected?.provider || request.provider !== selected.provider || request.model !== selected.modelId) return 'No matching request result available';
  return request.succeeded ? 'Last observed request succeeded' : 'Last observed request did not complete successfully';
}
export function needsModelSetup(live: boolean, selected: Selection, models: ModelOption[]): boolean {
  return live && (!selected || selectedRoute(selected, models) === 'Setup not confirmed');
}

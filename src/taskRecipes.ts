// Editable starting points shared by the empty conversation and the command palette.
import { snapshot } from "./bridge";
import { draftFor } from "./drafts";
import type { Snapshot } from "./types";
import { ui } from "./ui";
import { addToPrompt } from "./workspace";

export const taskRecipes = [
  {
    id: "map-codebase",
    title: "Map this codebase",
    detail: "Explain the structure and main flows",
    prompt: "Map the main modules and trace one representative flow through the codebase. Cite the files to start with and note any assumptions. Do not edit files.",
  },
  {
    id: "fix-test",
    title: "Fix a failing test",
    detail: "Reproduce the failure, fix it, and verify the fix",
    prompt: "Reproduce the reported test failure with the narrowest available test. If no failure is specified, use the project's documented test command to find one. Identify the root cause, make a focused fix, and rerun the failing test. Report what passed and what remains unverified.",
  },
  {
    id: "review-changes",
    title: "Review my changes",
    detail: "Check uncommitted work for problems",
    prompt: "Review my uncommitted changes for bugs, regressions, and missing tests. List actionable findings in severity order with file and line references. Do not edit files.",
  },
  {
    id: "add-tests",
    title: "Add tests",
    detail: "Cover an important behavior and an edge case",
    prompt: "Find an important untested behavior in this project and add focused tests using the existing test conventions. Cover a representative case and a meaningful edge case. Run the tests and report the result.",
  },
];

export function canUseTaskRecipes(snap: Snapshot) {
  return !snap.turnRunning && (snap.canSend || snap.connection.state === "preview");
}

/** Prepare a prompt, never send it or change the engine's permissions. */
export function insertTaskRecipe(prompt: string) {
  const snap = snapshot.value;
  if (!snap?.workspace || !canUseTaskRecipes(snap)) return;
  if (draftFor({ workspace: snap.workspace, sessionId: snap.sessionId }).sending) return;
  ui.settingsOpen = false;
  addToPrompt(prompt);
}

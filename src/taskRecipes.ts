// Editable starting points shared by the empty conversation and the command palette.
import { snapshot } from "./bridge";
import { draftFor } from "./drafts";
import type { Snapshot } from "./types";
import { ui } from "./ui";
import { addToPrompt } from "./workspace";
import { appearance } from "./appearance";

interface TaskRecipe { id: string; title: string; detail: string; prompt: string; needs?: "cards" | "diagrams" }

const ALL: TaskRecipe[] = [
  {
    id: "explain-visually",
    title: "Explain with a diagram",
    detail: "Turn a topic into a visual explanation",
    prompt: "Explain the topic below in plain language. Show how the pieces fit together in a small Mermaid flowchart in a fenced mermaid block. Follow it with the key takeaways.\n\nTopic: ",
    needs: "diagrams",
  },
  {
    id: "make-interactive",
    title: "Make it interactive",
    detail: "Build a guide, calculator, comparison, explorable diagram, or decision helper",
    prompt: 'Make an interactive card about the topic below. Return one fenced dial-app block of JSON with a "title", a "kind", and up to four "followUps" like {"label":"Go deeper","ask":"Explain … in more detail"}. Pick the kind that fits best:\n- "guide": {"steps":[{"title":"Install","text":"Markdown","checklist":["Node 20 installed"]}]}\n- "calculator": {"inputs":[{"id":"users","label":"Users","type":"slider","min":0,"max":1000,"step":10,"value":100,"unit":"k"}],"outputs":[{"id":"cost","label":"Monthly cost","formula":"users * 0.4","unit":"$","decimals":0}]}. Input types are slider, number, select (with "options":[{"label":"…","value":1}]) and toggle. Formulas use + - * / % ^, comparisons, a ? b : c, and min max abs round floor ceil sqrt pow clamp log ln exp; they may use input ids and earlier output ids.\n- "compare": {"tabs":[{"label":"Option A","text":"Markdown"}]} with 2-8 tabs.\n- "explore": {"diagram":"flowchart LR\\n  A[Client] --> B[Server]","intro":"Markdown","nodes":{"A":{"text":"Markdown","ask":"optional prompt"},"B":{"text":"Markdown"}}}, keyed by Mermaid node id. Use a flowchart or stateDiagram-v2 (other diagram types have no clickable nodes) and give every node and subgraph an entry, so none is left unexplained.\n- "screens": branching steps {"screens":[{"id":"start","text":"Markdown","choices":[{"label":"Next","to":"end"}]},{"id":"end","text":"Done"}]}; a choice may instead be {"label":"…","ask":"prompt"}.\nText fields are Markdown and may include code, tables, and mermaid blocks. Keep labels short. JSON only: no scripts or HTML.\n\nTopic: ',
    needs: "cards",
  },
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

/** The recipes in effect: one whose answer the Appearance settings would not draw is left out. */
export function taskRecipes(): TaskRecipe[] {
  return ALL.filter((r) => !r.needs || appearance[r.needs]);
}

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

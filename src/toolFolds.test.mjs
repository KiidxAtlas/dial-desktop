import assert from "node:assert/strict";
import test from "node:test";
import { phaseOf, progressFraction, runActivity, runFiles, runHeadline, runImages, runSummary, toolRuns } from "./toolFolds.ts";

const tool = (key, label, state = "succeeded", check = false, extra = {}) =>
  ({ kind: "tool", key, label, target: key, state, check, changedPaths: [], images: [], output: null, ...extra });

const notice = (lane = null) => ({ kind: "notice", message: "note", lane, noticeKind: null });
const received = (sender = "task-1") => ({ kind: "received", sender, text: "done", source: "live" });

test("one run per burst of calls; answers, dividers, alerts and questions to the user break it", () => {
  const entries = [
    tool("a", "Read"), tool("b", "Search"), tool("c", "Edit"), tool("d", "Run"), tool("e", "Generate image"),
    { kind: "assistant" }, tool("f", "Web"),
    notice("divider"), tool("g", "Read"), tool("h", "Ask"), tool("i", "Read"),
    notice("alert"), tool("j", "Read"),
  ];
  assert.deepEqual([...toolRuns(entries).values()].map(({ start, end }) => [start, end]), [
    [0, 4], [6, 6], [8, 8], [9, 9], [10, 10], [12, 12],
  ]);
});

test("received messages and the status lines between a burst's rows fold into it", () => {
  const entries = [
    notice(), tool("a", "Read"), received(), notice(), tool("b", "Run"), notice("context"), received("peer"),
    notice(), { kind: "assistant" }, received(), received(), { kind: "user" }, received(), notice(), tool("c", "Ask"),
  ];
  const runs = [...toolRuns(entries, "s1").values()];
  // Status lines before a burst (0), after it (7) or next to a question (13) stay in view.
  assert.deepEqual(runs.map(({ start, end }) => [start, end]), [[1, 6], [9, 10], [12, 12], [14, 14]]);
  assert.deepEqual(runs[0].items.map((item) => item.index), [1, 2, 3, 4, 5, 6]);
  assert.deepEqual(runs[0].tools.map((t) => t.key), ["a", "b"]);
  assert.equal(runSummary(runs[0]), "Read 1 file, received 2 messages, ran 1 command");
  assert.equal(runs[0].key, "fold:a");
  assert.equal(runSummary(runs[1]), "Received 2 messages");
  assert.equal(runs[1].key, "fold:s1:received:9");
  assert.deepEqual(runActivity(runs[1]), { current: undefined, running: false, failed: 0, unknown: 0 });
});

test("a live row keeps its identity, shows the newest active call, and retains failures", () => {
  const first = tool("a", "Read", "running");
  const second = tool("b", "Search", "running");
  const failed = tool("c", "Find", "failed");
  const unknown = tool("d", "Read", "unknown");
  const singleton = toolRuns([first]).get(0);
  const run = toolRuns([first, second, failed, unknown]).get(0);
  assert.equal(run.key, singleton.key);
  assert.deepEqual(runActivity(run), { current: second, running: true, failed: 1, unknown: 1 });
  first.state = second.state = "succeeded";
  assert.deepEqual(runActivity(run), { current: unknown, running: false, failed: 1, unknown: 1 });
});

test("the summary counts distinct files and calls in the order the work happened", () => {
  const run = toolRuns([
    tool("src/a.rs", "Read"), tool("src/b.rs", "Read"), tool("src/a.rs", "Read"), tool("x", "Search"),
    tool("e1", "Edit", "succeeded", false, { changedPaths: ["src/a.rs"] }),
    tool("e2", "Write", "succeeded", false, { changedPaths: ["src/a.rs"] }),
    tool("cargo test", "Custom", "failed", true), tool("m", "browser_click"),
  ]).get(0);
  assert.equal(runSummary(run), "Read 2 files, 1 search, edited 1 file, ran 1 command, used 1 tool");
  assert.equal(runHeadline(run), "Edited 1 file, ran 1 command · 3 more");
  assert.equal(runHeadline(run, 9), "Edited 1 file, ran 1 command, used 1 tool, 1 search, read 2 files");
});

test("progress lines that state a count read as a fraction", () => {
  assert.equal(progressFraction("8 of 12 tests passed"), 8 / 12);
  assert.equal(progressFraction("step 3/10"), 0.3);
  assert.equal(progressFraction("Compiling server"), null);
  assert.equal(progressFraction("5 of 0"), null);
  assert.equal(progressFraction(null), null);
});

test("changed files carry the line counts their calls reported", () => {
  const diff = "--- a/r.rs\n+++ b/r.rs\n@@ -1 +1,2 @@\n-old\n+new\n+more";
  const run = toolRuns([
    tool("e1", "Edit", "succeeded", false, { changedPaths: ["l.rs"], output: { caption: "result", text: "Created l.rs (+86)" } }),
    tool("e2", "Edit", "succeeded", false, { changedPaths: ["r.rs"], output: { caption: "diff", text: diff } }),
    tool("e3", "Edit", "succeeded", false, { changedPaths: ["l.rs"], output: { caption: "result", text: "l.rs (+9 -2)" } }),
    tool("e4", "Run", "succeeded", false, { changedPaths: ["x", "y"], output: { caption: "result", text: "(+1)" } }),
    tool("g", "Generate image", "succeeded", false, { images: ["/p/a.png"] }),
    tool("v", "Read", "succeeded", false, { images: ["/p/a.png", "/p/b.png"] }),
  ]).get(0);
  assert.deepEqual(runFiles(run), [
    { path: "l.rs", added: 95, removed: 2 },
    { path: "r.rs", added: 2, removed: 1 },
    { path: "x", added: null, removed: null },
    { path: "y", added: null, removed: null },
  ]);
  assert.deepEqual(runImages(run), ["/p/a.png", "/p/b.png"]);
});

test("each call names its phase for the live line; checks verify, unknown tools work", () => {
  const phases = [["Read", "Reading"], ["grep", "Searching"], ["Write", "Writing"], ["Edit", "Editing"],
    ["bash", "Running commands"], ["Web search", "Searching the web"], ["todo", "Making the plan"],
    ["browser_click", "Working"]];
  for (const [label, phase] of phases) assert.equal(phaseOf({ label, check: false }), phase);
  assert.equal(phaseOf({ label: "Custom", check: true }), "Verifying");
});

import assert from "node:assert/strict";
import test from "node:test";
import { runActivity, runFiles, runImages, runSummary, toolRuns } from "./toolFolds.ts";

const tool = (key, label, state = "succeeded", check = false, extra = {}) =>
  ({ kind: "tool", key, label, target: key, state, check, changedPaths: [], images: [], output: null, ...extra });

test("one run per burst of calls; messages, notices and questions to the user break it", () => {
  const entries = [
    tool("a", "Read"), tool("b", "Search"), tool("c", "Edit"), tool("d", "Run"), tool("e", "Generate image"),
    { kind: "assistant" }, tool("f", "Web"),
    { kind: "notice" }, tool("g", "Read"), tool("h", "Ask"), tool("i", "Read"),
  ];
  assert.deepEqual([...toolRuns(entries).values()].map(({ start, end }) => [start, end]), [
    [0, 4], [6, 6], [8, 8], [9, 9], [10, 10],
  ]);
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

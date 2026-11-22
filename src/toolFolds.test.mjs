import assert from "node:assert/strict";
import test from "node:test";
import { runActivity, toolRuns } from "./toolFolds.ts";

const tool = (key, label, state = "succeeded", check = false) => ({ kind: "tool", key, label, state, check });

test("groups consecutive calls by category without crossing messages or unrelated calls", () => {
  const entries = [
    tool("a", "Read"), tool("b", "Search"), tool("c", "Find"),
    tool("d", "Write"), tool("e", "Edit"),
    tool("f", "bg"), tool("g", "Verify"), tool("h", "Check"), tool("i", "Custom", "succeeded", true),
    tool("j", "Web"), tool("k", "web_search"),
    { kind: "assistant" }, tool("l", "Web"), tool("m", "Ask"), tool("n", "Web"),
  ];
  assert.deepEqual([...toolRuns(entries).values()].map(({ start, end, label }) => [start, end, label]), [
    [0, 2, "Read & search"], [3, 4, "Write & edit"], [5, 8, "Run & check"],
    [9, 10, "Web search"], [12, 12, "Web search"], [14, 14, "Web search"],
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

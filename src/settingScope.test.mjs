import test from "node:test";
import assert from "node:assert/strict";
import { changedCounts, changedTitle, hasSources, resetScope, saveScope, sourceBadge } from "./settingScope.ts";

test("saves and resets follow the layer that set the value, else the picked scope", () => {
  for (const [source, selected, save, reset] of [
    ["project", "global", "project", "project"],
    ["project", "project", "project", "project"],
    ["global", "project", "project", "global"],
    ["global", "global", "global", "global"],
    [undefined, "project", "project", "project"],
    [null, "global", "global", "global"],
  ]) {
    assert.equal(saveScope(source, selected), save, `save ${source} ${selected}`);
    assert.equal(resetScope(source, selected), reset, `reset ${source} ${selected}`);
  }
});

test("older engines report no layers, so counts and badges stay as before", () => {
  const old = [{ modified: true }, { modified: false }];
  assert.equal(hasSources(old), false);
  assert.deepEqual(changedCounts(old), { total: 1, project: 0, global: 0 });
  assert.equal(changedTitle(changedCounts(old), false), "Show every setting changed from its default");
  assert.equal(sourceBadge(undefined, "global"), null);
});

test("changed counts split by layer and badges name it", () => {
  const rows = [
    { modified: true, source: "project" },
    { modified: true, source: "global" },
    { modified: true, source: "global" },
    { modified: false, source: "project" },
  ];
  assert.equal(hasSources(rows), true);
  const counts = changedCounts(rows);
  assert.deepEqual(counts, { total: 3, project: 1, global: 2 });
  assert.match(changedTitle(counts, true), /1 in this project, 2 everywhere/);
  assert.equal(sourceBadge("project", "global").label, "This project");
  assert.match(sourceBadge("project", "global").hint, /^Overridden by this project/);
  assert.equal(sourceBadge("project", "project").hint, "");
  assert.equal(sourceBadge("global", "global").label, "Everywhere");
});

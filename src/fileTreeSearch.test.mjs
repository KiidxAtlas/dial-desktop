import test from "node:test";
import assert from "node:assert/strict";
import { effectScope } from "vue";
import { useFileTreeSearch } from "./fileTreeSearch.ts";

function setup(t) {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  const requests = [];
  const scope = effectScope();
  t.after(() => scope.stop());
  const state = scope.run(() => useFileTreeSearch(query => new Promise((resolve, reject) => {
    requests.push({ query, resolve, reject });
  })));
  return { ...state, requests, run: () => t.mock.timers.runAll() };
}

test("only the latest project search can publish, including after clearing", async t => {
  const s = setup(t);
  s.filter.value = "skip";
  s.filter.value = "first";
  s.run();
  s.filter.value = " next ";
  s.run();
  assert.deepEqual(s.requests.map(r => r.query), ["first", "next"]);
  s.requests[1].resolve(["closed/nested/file.ts"]);
  s.requests[0].reject("old failure");
  await Promise.resolve();
  assert.deepEqual(s.paths.value, ["closed/nested/file.ts"]);
  assert.equal(s.status.value, "");

  s.refreshSearch();
  s.run();
  s.filter.value = "";
  s.requests[2].resolve(["old-project/file.ts"]);
  await Promise.resolve();
  assert.deepEqual(s.paths.value, []);
  assert.equal(s.status.value, "");
});

test("a failed search can be retried and distinguished from no matches", async t => {
  const s = setup(t);
  s.filter.value = "missing";
  s.run();
  s.requests[0].reject("offline");
  await Promise.resolve();
  assert.match(s.status.value, /offline/);
  s.refreshSearch();
  s.run();
  assert.equal(s.requests[1].query, "missing");
  s.requests[1].resolve([]);
  await Promise.resolve();
  assert.equal(s.status.value, "No matching files");
});

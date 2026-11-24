import test from "node:test";
import assert from "node:assert/strict";
import { effectScope, ref } from "vue";
import { useTranscriptSearch } from "./transcriptSearch.ts";

function setup(t) {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  const scope = effectScope();
  t.after(() => scope.stop());
  const root = ref("project-a"), filter = ref(" needle "), enabled = ref(false);
  const requests = [];
  const state = scope.run(() => useTranscriptSearch(() => root.value, () => filter.value, enabled,
    (root, query, limit) => new Promise((resolve, reject) => requests.push({ root, query, limit, resolve, reject }))));
  return { ...state, root, filter, enabled, requests, tick: () => t.mock.timers.runAll() };
}

test("reconnect runs the pending query; stale results or errors cannot cross a project or connection change", async t => {
  const s = setup(t);
  s.tick();
  assert.equal(s.requests.length, 0);
  s.enabled.value = true;
  s.tick();
  s.root.value = "project-b";
  s.tick();
  s.enabled.value = false;
  s.requests[0].resolve({ hits: [{ session_id: "old project" }], has_more: false });
  s.requests[1].reject("old connection");
  await Promise.resolve();
  assert.equal(s.results.value, null);
  assert.equal(s.searchError.value, "");
  s.enabled.value = true;
  s.tick();
  assert.deepEqual(s.requests.map(r => [r.root, r.query]), [["project-a", "needle"], ["project-b", "needle"], ["project-b", "needle"]]);
  const result = { hits: [{ session_id: "current" }], has_more: false };
  s.requests[2].resolve(result);
  await Promise.resolve();
  assert.deepEqual(s.results.value, result);
});

test("show more replaces the ranked window, retries failures without losing hits, and respects the engine cap", async t => {
  const s = setup(t);
  s.enabled.value = true;
  void s.searchTranscripts(); // Enter submits without waiting for debounce or sending twice.
  s.tick();
  assert.equal(s.requests.length, 1);
  const first = { hits: [{ session_id: "first" }], has_more: true };
  s.requests[0].resolve(first);
  await Promise.resolve();
  s.loadMore();
  s.requests[1].reject("busy");
  await Promise.resolve();
  assert.deepEqual(s.results.value, first);
  assert.match(s.searchError.value, /busy/);
  let last = s.requests[0].limit;
  for (let tries = 0; tries < 12 && s.canExpand.value; tries++) {
    s.loadMore();
    const request = s.requests.at(-1);
    assert.ok(request.limit > last && request.limit <= 1000);
    last = request.limit;
    const refreshed = { hits: [{ session_id: `rank-${last}` }], has_more: true };
    request.resolve(refreshed);
    await Promise.resolve();
    assert.deepEqual(s.results.value, refreshed);
  }
  assert.equal(last, 1000);
  assert.equal(s.canExpand.value, false);
  s.filter.value = "next";
  s.tick();
  assert.equal(s.requests.at(-1).limit, s.requests[0].limit);
});

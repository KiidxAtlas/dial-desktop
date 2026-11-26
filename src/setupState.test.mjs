import test from "node:test";
import assert from "node:assert/strict";
import { lastRequestLabel, needsModelSetup, welcomeSteps } from "./setupState.ts";

const model = (overrides) => ({ provider: "anthropic", modelId: "m", label: "M", configured: false, local: false, listed: false, ...overrides });

test("welcome checklist follows the engine check", () => {
  const cases = [
    [null, true, ["checking", "later", "later"]],
    [{ state: "missing", error: "x" }, false, ["next", "later", "later"]],
    [{ state: "unlaunchable", error: "os error 193" }, false, ["next", "later", "later"]],
    [{ state: "failed", error: "boom" }, false, ["problem", "next", "later"]],
    [{ state: "unsupported", detail: "" }, false, ["done", "next", "later"]],
    [{ state: "checked", status: { current: "1.2.3" } }, false, ["done", "next", "later"]],
  ];
  for (const [engine, checking, states] of cases) {
    assert.deepEqual(welcomeSteps(engine, checking).map((s) => s.state), states, JSON.stringify(engine));
  }
  assert.match(welcomeSteps({ state: "checked", status: { current: "1.2.3" } }, false)[0].detail, /1\.2\.3/);
});

test("finish-setup card only for a live session without a usable model", () => {
  const cases = [
    [false, false, [], false],
    [true, false, [], true],
    [true, true, [], false],
    [true, true, [model({ catalogOnly: true })], true],
    [true, true, [model({ configured: true })], false],
    [true, true, [model({ local: true })], false],
    [true, true, [model({ listed: true })], false],
  ];
  for (const [live, has, models, expected] of cases) {
    assert.equal(needsModelSetup(live, has ? model({}) : null, models), expected, JSON.stringify([live, has, models]));
  }
  assert.equal(needsModelSetup(true, model({}), [model({}), model({ provider: 'other', listed: true }), model({ modelId: 'other', configured: true })]), true);
  assert.equal(needsModelSetup(true, model({ provider: '' }), [model({})]), false);
});

test('request evidence only describes the exact route that ran', () => {
  const request = { provider: 'anthropic', model: 'm', succeeded: true };
  assert.match(lastRequestLabel(model({}), request), /succeeded/);
  assert.match(lastRequestLabel(model({}), { ...request, succeeded: false }), /did not complete/);
  for (const choice of [null, model({provider: 'other'}), model({modelId: 'other'})]) assert.match(lastRequestLabel(choice, request), /No matching/);
});

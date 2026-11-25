import test from "node:test";
import assert from "node:assert/strict";
import { nameFailure, namedFailure, restartDone, saveRejected, savedState, settingFeedback, settleSaves, startSave } from "./settingsFeedback.ts";

test("failure toasts name the setting", () => {
  assert.equal(namedFailure("Could not save the setting: out of range", "Max steps"), "Could not save “Max steps”: out of range");
  assert.equal(namedFailure("Could not reset the setting", "Theme"), "Could not reset “Theme”");
  assert.equal(namedFailure("Turn failed: nope", "Theme"), null);
});

test("saved state says when the change takes effect", () => {
  assert.equal(savedState({ restart: false, turn: false }), "saved");
  assert.equal(savedState({ restart: false, turn: true }), "turn");
  assert.equal(savedState({ restart: true, turn: true }), "restart");
});

test("pending saves settle in order: a failure names the oldest, the rest succeed", () => {
  startSave({ key: "a", label: "Alpha", restart: false }, false);
  startSave({ key: "b", label: "Beta", restart: true }, false);
  assert.equal(settingFeedback.rows.a, "saving");
  assert.equal(nameFailure("Could not save the setting: bad"), "Could not save “Alpha”: bad");
  assert.equal(settingFeedback.rows.a, "failed");
  assert.equal(nameFailure("Something else"), "Something else");
  settleSaves();
  assert.equal(settingFeedback.rows.b, "restart");
  assert.deepEqual(settingFeedback.restart, ["Beta"]);
  restartDone();
  assert.deepEqual(settingFeedback.restart, []);
  assert.equal(settingFeedback.rows.b, undefined);

  startSave({ key: "c", label: "Gamma", restart: false }, false);
  saveRejected("c");
  settleSaves();
  assert.equal(settingFeedback.rows.c, "failed");
});

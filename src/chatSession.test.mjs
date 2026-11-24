import test from "node:test";
import assert from "node:assert/strict";
import { readTrash, updateTrash, trashKey } from './deferredDelete.ts';
import { denyKeyLabel, isDenyKey, mayAnswerFrom } from "./approvalKeys.ts";
import { initialReconnect, nextReconnect } from "./autoReconnect.ts";
import { turnAnnouncement } from "./turnAnnounce.ts";
import { renderLazily, rememberScroll, savedAnchor, savedScroll } from "./chatScroll.ts";

test('Recently Deleted survives reload, deduplicates and restores only in its own project', () => {
  const values = new Map();
  const store = { getItem: k => values.get(k) ?? null, setItem: (k,v) => values.set(k,v), removeItem: k => values.delete(k) };
  updateTrash(store, '/one', ['a', 'a', 'b'], false);
  updateTrash(store, '/two', ['a'], false);
  assert.deepEqual(readTrash(store.getItem(trashKey('/one'))), ['a','b']);
  assert.deepEqual(updateTrash(store, '/one', ['a'], true), ['b']);
  assert.deepEqual(readTrash(store.getItem(trashKey('/two'))), ['a']);
  store.setItem(trashKey('/two'), 'broken');
  assert.throws(() => updateTrash(store, '/two', ['b'], false));
  assert.equal(store.getItem(trashKey('/two')), 'broken');
});

test("⌘⌫ (macOS) and Ctrl+Backspace (elsewhere) deny; other chords do not", () => {
  const key = (over) => ({ key: "Backspace", metaKey: false, ctrlKey: false, altKey: false, shiftKey: false, ...over });
  const cases = [
    [key({ metaKey: true }), true, true],
    [key({ ctrlKey: true }), true, false],
    [key({ ctrlKey: true }), false, true],
    [key({ metaKey: true }), false, false],
    [key({}), true, false],
    [key({ metaKey: true, shiftKey: true }), true, false],
    [key({ metaKey: true, altKey: true }), true, false],
    [key({ metaKey: true, isComposing: true }), true, false],
    [key({ key: "Delete", metaKey: true }), true, false],
  ];
  for (const [event, mac, want] of cases) assert.equal(isDenyKey(event, mac), want, JSON.stringify({ event, mac }));
  assert.equal(denyKeyLabel(true), "⌘⌫");
  assert.equal(denyKeyLabel(false), "Ctrl+⌫");
});

test("the deny key never fires while text is being edited", () => {
  assert.equal(mayAnswerFrom(null), true);
  assert.equal(mayAnswerFrom({ tagName: "BUTTON" }), true);
  assert.equal(mayAnswerFrom({ tagName: "TEXTAREA", value: "" }), true, "an empty prompt box");
  assert.equal(mayAnswerFrom({ tagName: "TEXTAREA", value: "half a prompt" }), false);
  assert.equal(mayAnswerFrom({ tagName: "INPUT", value: "x" }), false);
  assert.equal(mayAnswerFrom({ tagName: "DIV", isContentEditable: true }), false);
});

test("one automatic reconnect per outage, only for a session that was live", () => {
  const live = { state: "live", model: "m", engineVersion: null };
  const ended = { state: "ended" };
  const failed = (problem) => ({ state: "failed", error: "boom", problem });
  const run = (steps) => {
    let state = initialReconnect;
    return steps.map(([key, connection]) => {
      const next = nextReconnect(state, key, connection);
      state = next.state;
      return next.start;
    });
  };
  // Live, then the engine goes: one countdown; the reconnect fails: no second one; live again resets.
  assert.deepEqual(run([["s", live], ["s", ended], ["s", { state: "starting" }], ["s", failed("other")], ["s", live], ["s", ended]]), [false, true, false, false, false, true]);
  // Never connected (a startup failure) or a session opened onto a stopped engine: no automatic try.
  assert.deepEqual(run([["s", { state: "starting" }], ["s", failed("other")]]), [false, false]);
  assert.deepEqual(run([["s", live], ["t", ended]]), [false, false]);
  // Reconnecting cannot install or update the engine.
  assert.deepEqual(run([["s", live], ["s", failed("missing")]]), [false, false]);
  assert.deepEqual(run([["s", live], ["s", failed("incompatible")]]), [false, false]);
});

test("the live region announces turn state changes, not streaming or session switches", () => {
  const view = (over) => ({ key: "s", turnRunning: false, approvals: 0, question: false, lastKind: "assistant", ...over });
  assert.equal(turnAnnouncement(null, view({ turnRunning: true })), null);
  assert.equal(turnAnnouncement(view(), view({ turnRunning: true })), "Dial is working");
  assert.equal(turnAnnouncement(view({ turnRunning: true }), view({ turnRunning: true, lastKind: "tool" })), null, "progress is not read");
  assert.equal(turnAnnouncement(view({ turnRunning: true }), view({ turnRunning: true, approvals: 1 })), "Dial is waiting for your approval");
  assert.equal(turnAnnouncement(view({ turnRunning: true, approvals: 1 }), view({ turnRunning: true, approvals: 2 })), "Dial is waiting for 2 approvals");
  assert.equal(turnAnnouncement(view({ turnRunning: true }), view({ turnRunning: true, question: true })), "Dial has a question for you");
  assert.equal(turnAnnouncement(view({ turnRunning: true }), view()), "Dial finished");
  assert.equal(turnAnnouncement(view({ turnRunning: true }), view({ lastKind: "error" })), "Dial stopped with an error");
  assert.equal(turnAnnouncement(view({ key: "a", turnRunning: true }), view({ key: "b" })), null);
});

test("older entries render lazily, never the newest or one being scrolled to", () => {
  assert.equal(renderLazily(0, 100), true);
  assert.equal(renderLazily(59, 100), true);
  assert.equal(renderLazily(60, 100), false, "the newest 40 always render");
  assert.equal(renderLazily(5, 30), false, "short conversations render whole");
  assert.equal(renderLazily(3, 100, [3]), false, "a find match or jump target renders");
});

test("a scroll position keeps the entry that was at the top", () => {
  rememberScroll("k-anchor", 900, false, { entry: 12, offset: -30 });
  assert.equal(savedScroll("k-anchor"), 900);
  assert.deepEqual(savedAnchor("k-anchor"), { entry: 12, offset: -30 });
  rememberScroll("k-anchor", 0, true, { entry: 1, offset: 0 });
  assert.equal(savedAnchor("k-anchor"), null, "at the end nothing is kept");
});

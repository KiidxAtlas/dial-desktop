import test from "node:test";
import assert from "node:assert/strict";
import { findOccurrences, transcriptCandidates } from "./chatFind.ts";
import { rememberScroll, savedScroll, scrollKey } from "./chatScroll.ts";
import { entryKey, parseLayout } from "./ui.ts";

const tool = (over) => ({ kind: "tool", key: "t", label: "bash", target: "npm test", input: null, output: null, ...over });

test("find matches reasoning and tool input and output, not only labels", () => {
  const entries = [
    { kind: "user", text: "Fix the parser", images: 0 },
    { kind: "assistant", text: "Done.", reasoning: "The tokenizer drops quotes", streaming: false, abandoned: false },
    tool({ output: { caption: "output", text: "3 tests FAILED" } }),
    tool({ input: { caption: "input", text: "grep -rn Tokenizer" } }),
    { kind: "error", message: "Rate limited" },
  ];
  const rows = (q) => findOccurrences(entries, q).map(hit => [hit.entry, hit.field]);
  assert.deepEqual(rows('tokenizer'), [[1, 'reasoning'], [3, 'input']]);
  assert.deepEqual(rows('failed'), [[2, 'output']]);
  assert.deepEqual(rows('npm test'), [[2, 'target'], [3, 'target']]);
  assert.deepEqual(rows('  '), []);
  assert.deepEqual(rows('bash npm'), [], 'a match cannot straddle unrelated fields');
  const repeated = [{ kind: 'user', text: 'İ [x] then [X]' }];
  assert.deepEqual(findOccurrences(repeated, '[x]').map(m => [m.start, m.end]), [[2, 5], [11, 14]]);
  assert.deepEqual(transcriptCandidates([...entries, entries[0]], { role: 'user', preview: entries[0].text }), [0, 5]);
});

test('find reveals received text and sender; only journaled mail can be a saved user-role hit', () => {
  const text = '[Message from scout]\n<em>needle</em>\nlast line';
  const rows = ['journal', 'workerNotice'].map(source => ({ kind: 'received', source, sender: 'scout', text }));
  assert.deepEqual(findOccurrences(rows, 'needle').map(m => [m.entry, m.field, text.slice(m.start, m.end)]), [[0, 'text', 'needle'], [1, 'text', 'needle']]);
  assert.deepEqual(findOccurrences(rows, 'scout').map(m => m.field), ['sender', 'text', 'sender', 'text']);
  assert.deepEqual(transcriptCandidates(rows, { role: 'user', preview: text }), [0]);
});

test("scroll positions are kept per session, and not for a reader at the end", () => {
  const a = scrollKey("/p", "a");
  const b = scrollKey("/p", "b");
  assert.equal(scrollKey("/p", null), null);
  rememberScroll(a, 420, false);
  rememberScroll(b, 90, true);
  assert.equal(savedScroll(a), 420);
  assert.equal(savedScroll(b), null);
  rememberScroll(a, 10, true);
  assert.equal(savedScroll(a), null);
  assert.equal(savedScroll(null), null);
});

test('a fresh session restores its numeric anchor; malformed stored positions are ignored', async (t) => {
  const storage = new Map();
  const descriptor = Object.getOwnPropertyDescriptor(globalThis, 'localStorage');
  Object.defineProperty(globalThis, 'localStorage', { configurable: true, value: {
    getItem: k => storage.get(k) ?? null, setItem: (k, v) => storage.set(k, v),
  } });
  t.after(() => descriptor ? Object.defineProperty(globalThis, 'localStorage', descriptor) : delete globalThis.localStorage);
  const first = await import('./chatScroll.ts?writer');
  first.rememberScroll('room', 480, false, { entry: 11, offset: -12 });
  const next = await import('./chatScroll.ts?restart');
  assert.equal(next.savedScroll('room'), 480);
  assert.deepEqual(next.savedAnchor('room'), { entry: 11, offset: -12 });
  for (const key of storage.keys()) storage.set(key, '[null,["room",{"top":"bad"}]]');
  const damaged = await import('./chatScroll.ts?damaged');
  assert.equal(damaged.savedScroll('room'), null);
});

test("expanded keys differ between sessions", () => {
  assert.notEqual(entryKey("a", "prompt", 3), entryKey("b", "prompt", 3));
  assert.notEqual(entryKey("a", "prompt", 3), entryKey("a", "reasoning", 3));
});

test("a stored layout falls back to the defaults for missing or bad values", () => {
  const defaults = { sidebarOpen: true, inspectorOpen: false, inspectorTab: "plan" };
  assert.deepEqual(parseLayout(null), defaults);
  assert.deepEqual(parseLayout("not json"), defaults);
  assert.deepEqual(parseLayout('{"sidebarOpen":"no","inspectorTab":"nope"}'), defaults);
  for (const inspectorTab of ["terminal", "workers", "peers"]) {
    const layout = { sidebarOpen: false, inspectorOpen: true, inspectorTab };
    assert.deepEqual(parseLayout(JSON.stringify(layout)), layout);
  }
});

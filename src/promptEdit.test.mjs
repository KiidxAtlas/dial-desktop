import assert from 'node:assert/strict';
import test from 'node:test';
import { LastSubmission } from './promptEdit.ts';
import { computed } from 'vue';
import { beginSend, draftReady, readingFiles, readyForFollowUp, restoreInput } from './draftInput.ts';

const snap = { workspace: { root: '/project' }, sessionId: 'chat', connection: { state: 'live' }, entries: [] };
const scope = { workspaceRoot: '/project', sessionId: 'chat' };
const files = [{ key: 'original-paste', data: 'fixture' }];
const row = { kind: 'user', text: 'staged prompt', images: 1 };

for (const ackFirst of [true, false]) test(`restore original text and paste (${ackFirst ? 'ack' : 'snapshot'} first)`, () => {
  const last = new LastSubmission();
  const finish = last.start(snap, scope, 'my prompt', files);
  if (ackFirst) finish(row.text);
  last.observe({ ...snap, entries: [row] });
  if (!ackFirst) finish(row.text);
  assert.deepEqual(last.forRow(row), { text: 'my prompt', attachments: files });
  assert.equal(last.forRow({ ...row }), undefined);
});

test('never attach stale bytes after rejection, replay, switching or undo', () => {
  for (const invalidate of [last => last.observe(snap), last => last.observe({ ...snap, entries: [row], historyLoading: true }), last => last.observe({ ...snap, entries: [row], sessionId: 'other' }), (_, finish) => finish(), (_, finish) => finish('different text')]) {
    const last = new LastSubmission();
    const finish = last.start(snap, scope, 'my prompt', files);
    last.observe({ ...snap, entries: [row] });
    finish(row.text);
    invalidate(last, finish);
    assert.equal(last.forRow(row), undefined);
  }
});

test('restored picker images send their original preview bytes, never the possibly changed path', () => {
  const last = new LastSubmission();
  const selected = { path: '/chosen.png', key: 'selected', name: 'chosen.png', url: 'data:image/png;base64,aW1n', data: null };
  const finish = last.start(snap, scope, 'look', [selected]);
  last.observe({ ...snap, entries: [row] });
  finish(row.text);
  assert.deepEqual(last.forRow(row).attachments, [{ ...selected, path: null, data: 'aW1n' }]);
  assert.equal(selected.path, '/chosen.png');
});

test('mixed image and path-only document edits keep staged references instead of reopening the document', () => {
  const last = new LastSubmission();
  const image = { name: 'paste.png', url: 'data:image/png;base64,aW1n', data: 'aW1n', path: null };
  const finish = last.start(snap, scope, 'review', [image, { name: 'report.pdf', path: '/report.pdf', data: null, url: null }]);
  last.observe({ ...snap, entries: [row] });
  finish(row.text);
  assert.deepEqual(last.forRow(row), { text: row.text, attachments: [image] });
});

test('a preview appears on acknowledgement for its exact row; replay clears it', () => {
  const last = new LastSubmission();
  const image = { name: 'Local.png', url: 'data:image/png;base64,aW1n' };
  const finish = last.start(snap, scope, 'attached', [image, { name: 'linked', url: 'file:///other' }]);
  const previews = computed(() => last.imagesFor(row));
  last.observe({ ...snap, entries: [row] });
  assert.deepEqual(previews.value, []);
  finish(row.text);
  assert.deepEqual(previews.value, [image]);
  assert.deepEqual(last.imagesFor({ ...row }), []);
  last.observe({ ...snap, entries: [row], historyLoading: true });
  assert.deepEqual(previews.value, []);
});

const draft = () => ({ text: 'next', attachments: [{ key: 'current' }], sending: false, loading: false, reading: 0 });

test('parallel attachment operations block only their draft until both finish, including failures', async () => {
  const a = draft(), b = draft();
  let release, reject;
  const first = readingFiles(a, () => new Promise(resolve => { release = resolve; }));
  const second = readingFiles(a, () => new Promise((_, fail) => { reject = fail; }));
  assert.equal(draftReady(a), false);
  assert.equal(draftReady(b), true);
  release(); await first;
  assert.equal(draftReady(a), false);
  reject(new Error('unreadable'));
  await assert.rejects(second, /unreadable/);
  assert.equal(draftReady(a), true);
});

test('restoration keeps a newer draft intact, including files added while the saved draft loaded', () => {
  const d = draft();
  restoreInput(d, { text: 'earlier', attachments: [{ key: 'saved' }, { key: 'current' }] });
  assert.equal(d.text, 'earlier\n\nnext');
  assert.deepEqual(d.attachments, [{ key: 'saved' }, { key: 'current' }]);
});

test('an unconfirmed regenerate restores its payload to the original draft; acceptance keeps only the next draft', () => {
  for (const accepted of [true, false]) {
    const d = draft();
    const resend = { text: 'undone prompt', attachments: files };
    const done = beginSend(d, resend);
    assert.equal(d.text, 'next');
    assert.equal(draftReady(d), false);
    d.text += ' draft';
    done(accepted);
    assert.equal(d.text, accepted ? 'next draft' : 'undone prompt\n\nnext draft');
    assert.deepEqual(d.attachments, accepted ? [{ key: 'current' }] : [...files, { key: 'current' }]);
    assert.equal(d.pending, undefined);
    assert.equal(draftReady(d), true);
  }
});

test('delayed sends require the original open, idle session, even if a stale snapshot was sendable', () => {
  const idle = { ...snap, canSend: true, turnRunning: false, historyLoading: false, question: null, approvals: [] };
  assert.equal(readyForFollowUp(idle, snap), true);
  assert.equal(readyForFollowUp(null, snap), false);
  for (const change of [
    { sessionId: 'other' }, { workspace: { root: '/elsewhere' } }, { workspace: null },
    { turnRunning: true }, { canSend: false }, { historyLoading: true },
    { connection: { state: 'ended' } }, { approvals: [{}] }, { question: {} },
  ]) assert.equal(readyForFollowUp({ ...idle, ...change }, snap), false, JSON.stringify(change));
});

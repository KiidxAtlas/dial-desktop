import assert from 'node:assert/strict';
import test from 'node:test';
import { LastSubmission } from './promptEdit.ts';

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

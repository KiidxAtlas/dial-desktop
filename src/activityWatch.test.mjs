import test from 'node:test';
import assert from 'node:assert/strict';
import { canAddress, checkPeerReceipt, checkWorkerReceipt, messageTooLong, resolveRecipient } from './activityWatch.ts';

const worker = { id: 'task-2', watchToken: 'original-engine', name: 'Review', status: 'running', started_ms: 10 };
const session = 'a'.repeat(32), peer = { id: 'b'.repeat(32), summary: 'Review' };

test('refresh follows an exact recipient rather than the row position or display name', () => {
  const selected = { kind: 'worker', row: worker };
  const done = { ...worker, status: 'completed' }, other = { ...worker, id: 'task-3' };
  const after = resolveRecipient(selected, [other, done], [peer]);
  assert.deepEqual(after, { kind: 'worker', row: done });
  assert.equal(canAddress(after), false);
  for (const rows of [[], [other], [worker, worker], [{ ...worker, watchToken: 'restarted-engine' }], [{ ...worker, started_ms: 11 }]])
    assert.equal(resolveRecipient(selected, rows, [peer]), null);
  const board = { kind: 'peer', row: peer };
  assert.equal(resolveRecipient(board, [{ ...worker, id: peer.id }], []), null);
  assert.deepEqual(resolveRecipient(board, [], [{ ...peer, id: session }, peer]), board);
});

test('only a single worker, or an exact peer id, can be addressed; cap UTF-8 not JS characters', () => {
  for (const id of ['parent', 'all', 'Review', 'pool-1', 'node:1', 'task-0', 'task-1,task-2'])
    assert.equal(canAddress({ kind: 'worker', row: { ...worker, id } }), false, id);
  assert.equal(canAddress({ kind: 'worker', row: { ...worker, watchToken: '' } }), false);
  assert.equal(canAddress({ kind: 'worker', row: worker }), true);
  assert.equal(canAddress({ kind: 'peer', row: { id: 'short-prefix' } }), false);
  assert.equal(canAddress({ kind: 'peer', row: peer }), true);
  assert.equal(messageTooLong('🖋'.repeat(1024)), false);
  assert.equal(messageTooLong('🖋'.repeat(1024) + 'a'), true);
});

test('success requires this recipient’s explicit queued receipt; refusal and ambiguous results are not success', () => {
  const queued = { to: worker.id, status: 'queued' };
  checkWorkerReceipt(worker.id, { receipts: [queued] });
  for (const receipts of [[], [queued, queued], [{ ...queued, to: 'task-3' }], [{ ...queued, status: 'finished' }]])
    assert.throws(() => checkWorkerReceipt(worker.id, { receipts }));
  assert.throws(() => checkWorkerReceipt(worker.id, { receipts: [{ ...queued, status: 'refused', reason: 'Inbox full' }] }), /Inbox full/);
  const receipt = { session_id: session, to: peer.id, queued: true };
  checkPeerReceipt(session, peer.id, receipt);
  for (const reply of [null, { ...receipt, session_id: peer.id }, { ...receipt, to: session }, { ...receipt, queued: false }])
    assert.throws(() => checkPeerReceipt(session, peer.id, reply));
});

import test from 'node:test';
import assert from 'node:assert/strict';
import { boardSignals, compareCaptures } from './boardSignals.ts';

test('plots only repeated named numeric samples in capture order', () => {
  const [signal, ...rest] = boardSignals('boot 1\ntemp=12, id=0\ntemp=13; raw=0xff\ntemp=14 raw=2V');
  assert.deepEqual(signal, { name: 'temp', min: 12, max: 14, count: 3, points: '0,27 50,15 100,3' });
  assert.deepEqual(rest, []);
});
test('flat and extreme readings have finite coordinates', () => {
  const [flat, extreme] = boardSignals('a=9 b=-1e308\na=9 b=1e308');
  assert.equal(flat.points, '0,15 100,15');
  assert.equal(extreme.points, '0,27 100,3');
});

test('puts both captures on shared numeric and sample-index axes', () => {
  const c = compareCaptures('ready\nv=0\nv=10', 'ready\nv=10\nv=15\nv=20');
  assert.deepEqual(c.difference, { line: 2, before: 'v=0', after: 'v=10' });
  assert.deepEqual(c.traces, [{ name: 'v', min: 0, max: 20, before: '0,27 50,15', after: '0,15 50,9 100,3' }]);
});
test('handles identical logs, missing lines, and missing signals', () => {
  assert.deepEqual(compareCaptures('ready\n', 'ready'), { difference: null, traces: [] });
  assert.deepEqual(compareCaptures('ready\nwarn', 'ready').difference, { line: 2, before: 'warn', after: undefined });
  assert.deepEqual(compareCaptures('', 'warn').difference, { line: 1, before: undefined, after: 'warn' });
  assert.deepEqual(compareCaptures('a=1 a=2', 'b=1 b=2').traces, []);
});

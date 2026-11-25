import assert from 'node:assert/strict';
import test from 'node:test';
import { turnProgress } from './turnProgress.ts';

const active = { turnRunning: true, entries: [], approvals: [], question: null, stats: { running: 0 } };

test('shows reported phases and token counts without inventing a total or hiding estimates', () => {
  for (const [modelProgress, label, detail] of [
    [null, 'Waiting for the model…', undefined],
    [{ phase: 'loading' }, 'Loading model…', undefined],
    [{ phase: 'processing', promptTokens: 100 }, 'Processing prompt…', undefined],
    [{ phase: 'processing', processedTokens: 0, promptTokens: 100 }, 'Processing prompt…', '0 / 100 tokens'],
    [{ phase: 'processing', processedTokens: 12 }, 'Processing prompt…', '12 tokens processed'],
    [{ phase: 'generating', outputTokens: 12, estimated: true }, 'Generating response…', '≈12 tokens'],
    [{ phase: 'generating', outputTokens: 1 }, 'Generating response…', '1 token'],
  ]) {
    const status = turnProgress({ ...active, modelProgress });
    assert.equal(status.label, label);
    assert.equal(status.detail, detail);
  }
});

test('recognizes streamed reasoning and answers from engines without telemetry, not history', () => {
  for (const [streaming, text, reasoning, label] of [
    [true, '', 'hmm', 'Thinking…'], [true, 'Hi', 'hmm', 'Generating response…'],
    [false, 'Hi', 'hmm', 'Waiting for the model…'],
  ]) assert.equal(turnProgress({ ...active, entries: [{ kind: 'assistant', streaming, text, reasoning }] }).label, label);
  assert.equal(turnProgress({ ...active, modelProgress: { phase: 'processing' }, entries: [{ kind: 'assistant', streaming: true, text: 'Hi' }] }).label, 'Generating response…');
});

test('user input, tool execution and turn settlement supersede request telemetry', () => {
  const generating = { ...active, modelProgress: { phase: 'generating' } };
  assert.equal(turnProgress({ ...generating, stats: { running: 1 } }).label, 'Running a tool…');
  assert.equal(turnProgress({ ...generating, approvals: [{}], stats: { running: 1 } }).label, 'Waiting for your approval');
  assert.equal(turnProgress({ ...generating, question: {} }).label, 'Waiting for your answer');
  assert.equal(turnProgress({ ...generating, question: {} }).waiting, true);
  assert.equal(turnProgress({ ...generating, stats: { running: 1 } }).waiting, undefined);
  const live = { kind: 'tool', label: 'Edit', state: 'running' };
  assert.equal(turnProgress({ ...generating, stats: { running: 1 }, entries: [{ kind: 'tool', state: 'succeeded' }, live] }).tool, live);
  assert.equal(turnProgress({ ...generating, turnRunning: false, entries: [{ kind: 'error', message: 'Could not start server' }] }), null);
});

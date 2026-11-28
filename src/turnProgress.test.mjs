import assert from 'node:assert/strict';
import test from 'node:test';
import { cacheHitState, formatLastInference, turnProgress } from './turnProgress.ts';

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

test('surfaces prefill progress fraction, cache hit rate, tok/s and ETA telemetry', () => {
  const prefill = turnProgress({
    ...active,
    modelProgress: { phase: 'processing', processedTokens: 6000, promptTokens: 12000, cachedTokens: 9000, tokensPerSecond: 420, etaSecs: 14.2 },
  });
  assert.equal(prefill.fraction, 0.5);
  assert.equal(prefill.meta, '75% cached · 420 tok/s · ~15s left');

  const gen = turnProgress({
    ...active,
    modelProgress: { phase: 'generating', outputTokens: 84, tokensPerSecond: 34.7, cachedTokens: 8000, promptTokens: 10000 },
  });
  assert.equal(gen.meta, '35 tok/s · 80% cached');
  assert.equal(formatLastInference({ phase: 'generating', tokensPerSecond: 8.4, cachedTokens: 5000, promptTokens: 10000 }), '8.4 tok/s · 50% cached');
  assert.equal(formatLastInference(null), '');
});

test('classifies KV cache hit bands and marks active thinking steps for wrap-up steering', () => {
  assert.equal(cacheHitState(9000, 10000), 'high');
  assert.equal(cacheHitState(4500, 10000), 'mid');
  assert.equal(cacheHitState(500, 10000), 'low');
  assert.equal(cacheHitState(0, 4096), 'low');
  assert.equal(cacheHitState(0, 512), undefined);

  const cold = turnProgress({
    ...active,
    modelProgress: { phase: 'processing', processedTokens: 1024, promptTokens: 8192, cachedTokens: 0, tokensPerSecond: 300 },
  });
  assert.equal(cold.cacheState, 'low');
  assert.equal(cold.meta, '0% cached · 300 tok/s');

  const thinking = turnProgress({
    ...active,
    entries: [{ kind: 'assistant', streaming: true, text: '', reasoning: 'Evaluating options…' }],
  });
  assert.equal(thinking.label, 'Thinking…');
  assert.equal(thinking.thinking, true);

  const answering = turnProgress({
    ...active,
    entries: [{ kind: 'assistant', streaming: true, text: 'Here is the fix.', reasoning: 'Done.' }],
  });
  assert.equal(answering.thinking, false);
});

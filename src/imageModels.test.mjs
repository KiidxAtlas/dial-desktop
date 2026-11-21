import assert from 'node:assert/strict';
import test from 'node:test';
import { imageChoices, imageModelLabel } from './imageModels.ts';

test('only advertised image generators are selectable', () => {
  const local = { provider: 'mlx', model_id: '/models/flux', image_api: null };
  const sub = { provider: 'subscription', model_id: 'artist', output_modalities: ['image'], base_url: 'https://example.test' };
  const vision = { provider: 'subscription', provider_label: 'Subscription', model_id: 'reader', vision: true };
  const disabled = { ...sub, model_id: 'disabled', enabled: false };
  const lookalike = { provider: 'other', model_id: 'other/gpt-image-1' };
  assert.deepEqual(imageChoices([local, vision, sub], [sub, lookalike, disabled]), [local, sub]);
  assert.deepEqual(imageChoices([], []), []);
});

test('the resolved image route is selectable across providers without guessing from model names', () => {
  const row = { provider: 'chatgpt', model_id: 'hosted', current: true };
  const other = { ...row, provider: 'openrouter', current: false };
  const idle = { ...row, model_id: 'idle', current: false };
  const catalog = [other, idle, row];
  const route = { ok: true, api: 'Responses image tool', model: 'hosted', role: null };
  assert.deepEqual(imageChoices([], catalog, route), [row]);
  assert.deepEqual(imageChoices([], catalog, { ...route, ok: false }), []);
  assert.deepEqual(imageChoices([], catalog, { ...route, api: 'OpenRouter', role: { provider: 'openrouter', model: 'hosted' } }), [other]);
  assert.deepEqual(imageChoices([], catalog, { ...route, role: { provider: 'unknown', model: 'hosted' } }), []);
  assert.deepEqual(imageChoices([], [{ ...other, current: true }], { ...route, api: 'Other image API' }), [{ ...other, current: true }]);
  assert.deepEqual(imageChoices([], catalog, { ...route, model: 'idle', role: { provider: 'chatgpt', model: 'idle' } }), [idle]);
});

test('cached image capabilities filter incapable models and annotate verified vs candidate labels', () => {
  const verified = {
    provider: 'chatgpt',
    model_id: 'gpt-6.2-astra',
    label: 'GPT-6.2 Astra',
    image_api: 'responses',
    image_capability: 'capable',
    image_capability_source: 'verified',
    image_checked_at: 1700000000,
  };
  const candidate = {
    provider: 'chatgpt',
    model_id: 'gpt-5.4',
    label: 'GPT-5.4',
    image_api: null,
    image_capability: 'candidate',
    image_capability_source: 'candidate',
  };
  const incapable = {
    provider: 'chatgpt',
    model_id: 'refused-model',
    label: 'Refused',
    image_api: null,
    image_capability: 'incapable',
    image_capability_source: 'verified',
  };
  const metadata = {
    provider: 'openrouter',
    model_id: 'google/gemini-3.1-flash-image',
    label: 'Gemini 3.1 Flash Image',
    image_api: 'chat-completions',
    image_capability: 'capable',
    image_capability_source: 'metadata',
  };
  const picked = imageChoices([verified, candidate, incapable, metadata], []);
  assert.deepEqual(picked, [verified, candidate, metadata]);
  assert.equal(imageModelLabel(verified), 'GPT-6.2 Astra · verified');
  assert.equal(imageModelLabel(candidate), 'GPT-5.4 · unverified');
  assert.equal(imageModelLabel(metadata), 'Gemini 3.1 Flash Image');
});

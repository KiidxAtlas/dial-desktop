import assert from 'node:assert/strict';
import test from 'node:test';
import { removeSavedKey } from './providerKeys.ts';

const key = { provider: 'api-key:openai', name: 'OpenAI', key_saved: true, can_remove: true };
const forbidden = () => assert.fail('unexpected credential operation');

test('unsupported providers, cancellations and closed dialogs cannot remove credentials', async () => {
  for (const patch of [{ can_remove: undefined }, { can_remove: 'true' }, { provider: 'openai' }, { key_saved: false }]) {
    assert.equal(await removeSavedKey({ ...key, ...patch }, forbidden, forbidden, forbidden, () => true), false);
  }
  for (const consent of [false, true]) {
    let current = true;
    assert.equal(await removeSavedKey(key, async () => { current = !consent; return consent; }, forbidden, forbidden, () => current), false);
  }
});

test('removal targets only the chosen key and requires inventory confirmation', async () => {
  for (const [inventory, success] of [[[], false], [[key], false], [[{ ...key, provider: 'api-key:other', key_saved: false }], false], [[{ ...key, key_saved: false }], true]]) {
    const removed = [];
    const result = removeSavedKey(key, async () => true, async id => removed.push(id), async () => inventory, () => true);
    if (success) assert.equal(await result, true);
    else await assert.rejects(result, /could not verify/);
    assert.deepEqual(removed, [key.provider]);
  }
});

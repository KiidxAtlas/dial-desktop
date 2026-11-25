import test from 'node:test';
import assert from 'node:assert/strict';
import { textInputDefaults } from './textInputs.ts';

const none = () => false;
const only = (...names) => (name) => names.includes(name);

test('one-line text fields get autocorrect, capitals and spell checking off', () => {
  for (const type of ['', 'text', 'search', 'URL', 'email']) {
    assert.deepEqual(textInputDefaults('input', type, none), { autocorrect: 'off', autocapitalize: 'off', spellcheck: 'false' });
  }
});

test('text areas keep spell checking; prose fields opt in', () => {
  assert.deepEqual(textInputDefaults('textarea', '', none), { autocorrect: 'off', autocapitalize: 'off' });
  assert.deepEqual(textInputDefaults('input', 'text', only('data-prose')), { autocorrect: 'off', autocapitalize: 'off' });
});

test('a field\'s own attributes win', () => {
  assert.deepEqual(textInputDefaults('input', 'text', only('autocorrect', 'spellcheck')), { autocapitalize: 'off' });
  assert.deepEqual(textInputDefaults('textarea', '', only('autocorrect', 'autocapitalize')), {});
});

test('non-text inputs are left alone', () => {
  for (const type of ['checkbox', 'number', 'color', 'range', 'file', 'password']) {
    assert.deepEqual(textInputDefaults('input', type, none), {});
  }
  assert.deepEqual(textInputDefaults('select', '', none), {});
});

import test from 'node:test';
import assert from 'node:assert/strict';
import { GUEST_KEY_COMBOS, MAX_TYPED_TEXT, comboInputs, importProgressText, keyForChar, textInputs } from './appRunner.ts';

const GB = 1024 ** 3;
const cases = [
  [{ copied: 0, total: 4 * GB }, '0% · 0.0 of 4.0 GB copied'],
  [{ copied: 1.5 * GB, total: 4 * GB }, '37% · 1.5 of 4.0 GB copied'],
  [{ copied: 4 * GB, total: 4 * GB }, '100% · 4.0 of 4.0 GB copied'],
  [{ copied: 5 * GB, total: 4 * GB }, '100% · 4.0 of 4.0 GB copied'],
  [{ copied: 2 * GB, total: 0 }, '2.0 GB copied'],
];

test('import progress shows percent and GB copied, clamped to the total', () => {
  for (const [progress, text] of cases) assert.equal(importProgressText(progress), text, JSON.stringify(progress));
});

test('import progress shows the conversion share while qemu-img converts', () => {
  assert.equal(importProgressText({ copied: 4217, total: 10000, converting: true }), 'Converting to raw with qemu-img · 42%');
  assert.equal(importProgressText({ copied: 0, total: 0, converting: true }), 'Converting to raw with qemu-img · 0%');
});

test('key combinations press in order and release in reverse', () => {
  assert.deepEqual(comboInputs(['ControlLeft', 'AltLeft', 'Delete']).map((e) => `${e.code}${e.down ? '↓' : '↑'}`), [
    'ControlLeft↓', 'AltLeft↓', 'Delete↓', 'Delete↑', 'AltLeft↑', 'ControlLeft↑',
  ]);
  for (const combo of GUEST_KEY_COMBOS) assert.ok(combo.codes.length > 0, combo.label);
});

test('typed text becomes US-layout key presses; unmapped characters are counted', () => {
  for (const [char, key] of [
    ['a', { code: 'KeyA', shift: false }], ['Z', { code: 'KeyZ', shift: true }], ['7', { code: 'Digit7', shift: false }],
    ['!', { code: 'Digit1', shift: true }], [')', { code: 'Digit0', shift: true }], ['?', { code: 'Slash', shift: true }],
    ['\n', { code: 'Enter', shift: false }], [' ', { code: 'Space', shift: false }], ['é', null], ['😀', null],
  ]) assert.deepEqual(keyForChar(char), key, JSON.stringify(char));
  const { inputs, typed, skipped } = textInputs('Hi\r\né😀');
  assert.equal(typed, 3);
  assert.equal(skipped, 2);
  assert.deepEqual(inputs.slice(0, 4).map((e) => `${e.code}${e.down ? '↓' : '↑'}`), ['ShiftLeft↓', 'KeyH↓', 'KeyH↑', 'ShiftLeft↑']);
  assert.equal(inputs.at(-2).code, 'Enter');
  assert.equal(textInputs('x'.repeat(MAX_TYPED_TEXT + 10)).typed, MAX_TYPED_TEXT);
});

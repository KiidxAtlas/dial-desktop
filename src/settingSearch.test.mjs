import assert from 'node:assert/strict';
import test from 'node:test';
import { settingMatches } from './settingSearch.ts';

const busy = {
  key: 'ui.busy_send',
  label: 'Enter while working',
  description: 'Choose opens a send-action chooser without sending.',
  section: 'Input',
  tab: 'Keys',
  control: { type: 'select', options: [{ value: 'redirect', label: 'Redirect current task' }, { value: 'after_turn', label: 'Send after turn' }] },
};

test('settings search finds a setting by its choices as well as its name', () => {
  assert.ok(settingMatches(['enter'], busy));
  assert.ok(settingMatches(['redirect'], busy), 'a choice label or value finds the setting');
  assert.ok(settingMatches(['after_turn'], busy));
  assert.ok(settingMatches(['ui.busy_send'], busy));
  assert.ok(settingMatches(['keyboard'], busy, 'Keyboard'), 'the page label counts');
  assert.ok(!settingMatches(['redirect', 'memory'], busy), 'every term must match');
  assert.ok(!settingMatches(['redirect'], { ...busy, control: { type: 'toggle' } }));
});

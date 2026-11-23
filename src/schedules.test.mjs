import test from 'node:test';
import assert from 'node:assert/strict';
import { saveSchedule, typedSchedule } from './schedules.ts';

test('an unattended schedule is saved only with consent to its actual access and trigger', async () => {
  for (const [args, expected, access] of [
    [['--every', '1h', 'check', 'docs'], { every: '1h', prompt: 'check docs' }, 'Read-only'],
    [['--watch', '*.md', '--allow', 'write', '--', '--yes'], { watch: ['*.md'], allow: ['write'], prompt: '--yes' }, 'Read-only plus write'],
    [['--watch', '*.rs', '--yes', 'review'], { watch: ['*.rs'], approve_all: true, prompt: 'review' }, 'ALL tools pre-approved'],
  ]) {
    const calls = [];
    for (const consent of [false, true]) {
      assert.equal(await saveSchedule(typedSchedule(args), '/project', async summary => {
        for (const detail of [access, expected.prompt, expected.every ?? expected.watch[0], '/project']) assert.ok(summary.includes(detail));
        return consent;
      }, async params => calls.push(params)), consent);
      assert.deepEqual(calls, consent ? [{ ...expected, confirm: true }] : []);
    }
  }
});

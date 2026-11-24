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

test('next run follows the engine: the later of creation and last run, plus the interval', async () => {
  const { nextRun, schedulerHeadline } = await import('./schedules.ts');
  for (const [entry, last, expected] of [
    [{ every_secs: 60, created: 1000 }, null, 1060],
    [{ every_secs: 60, created: 1000 }, 2000, 2060],
    [{ every_secs: 60, created: 1000 }, 500, 1060],
    [{ watch: ['*.md'], created: 1000 }, 2000, null],
  ]) assert.equal(nextRun(entry, last), expected);
  // Our own runner wins over the engine's lease probe, which it also holds.
  assert.equal(schedulerHeadline({ phase: 'running' }, true).ours, true);
  assert.equal(schedulerHeadline({ phase: 'stopped' }, true).title, 'Running elsewhere');
  assert.equal(schedulerHeadline(null, false).ours, false);
});

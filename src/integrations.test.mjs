import assert from 'node:assert/strict';
import test from 'node:test';
import { computerSetting, engineTooOld, hookFromDraft, joinCommand, retryIntegrationRead, savedHook, splitCommand, sshFromDraft } from './integrations.ts';

test('commands split on spaces, group quoted words and round-trip through join', () => {
  assert.deepEqual(splitCommand('  ./check.sh --fast  '), ['./check.sh', '--fast']);
  assert.deepEqual(splitCommand(`python3 "my hook.py" 'a b' ""`), ['python3', 'my hook.py', 'a b', '']);
  assert.deepEqual(splitCommand('say "he said \\"hi\\""'), ['say', 'he said "hi"']);
  assert.throws(() => splitCommand('echo "open'), /quote/);
  for (const parts of [['a'], ['my prog', '--x=1', ''], ['q"uote', 'back\\slash']]) {
    assert.deepEqual(splitCommand(joinCommand(parts)), parts);
  }
});

test('a hook draft is validated and saved without the reported mode', () => {
  const events = ['pre_tool_use', 'stop'];
  const draft = { event: 'pre_tool_use', matcher: ' bash ', command: 'guard.sh', timeout: '', failure: 'default' };
  const hook = hookFromDraft(draft, events);
  assert.deepEqual(hook, { event: 'pre_tool_use', command: ['guard.sh'], timeout_secs: 30, matcher: 'bash' });
  assert.equal(hookFromDraft({ ...draft, failure: 'open' }, events).fail_closed, false);
  assert.throws(() => hookFromDraft({ ...draft, event: 'tool_call' }, events), /when/);
  assert.throws(() => hookFromDraft({ ...draft, command: '  ' }, events), /program/);
  for (const timeout of ['-1', '1.5', 'x', '90000']) assert.throws(() => hookFromDraft({ ...draft, timeout }, events), /Timeout/);
  assert.deepEqual(savedHook({ ...hook, fails_closed: true, fail_closed: null, matcher: null }), { event: 'pre_tool_use', command: ['guard.sh'], timeout_secs: 30 });
});

test('an ssh draft needs a fresh valid name and host, and a real port', () => {
  const draft = { name: 'box', host: '10.0.0.9', user: '', port: '', key: '' };
  assert.deepEqual(sshFromDraft(draft, []), { name: 'box', value: { host: '10.0.0.9' } });
  assert.deepEqual(sshFromDraft({ ...draft, user: 'ops', port: '2222', key: '~/.ssh/id' }, []).value, { host: '10.0.0.9', user: 'ops', port: 2222, key: '~/.ssh/id' });
  assert.throws(() => sshFromDraft(draft, ['box']), /exists/);
  for (const name of ['1box', 'a.b', '']) assert.throws(() => sshFromDraft({ ...draft, name }, []), /letter/);
  assert.throws(() => sshFromDraft({ ...draft, host: 'a b' }, []), /host/);
  for (const port of ['0', '70000', '22.5']) assert.throws(() => sshFromDraft({ ...draft, port }, []), /Port/);
});

test('only a missing method reads as an old engine', () => {
  assert.equal(engineTooOld(new Error('the engine does not serve `ssh_hosts` on this connection')), true);
  assert.equal(engineTooOld('invalid_params: bad'), false);
});

test('computer use changes only its entry in the effective settings layer', () => {
  for (const scope of [undefined, 'global', 'project']) {
    const current = { scope, value: ['browser', 'computer', 'git', 'computer'] };
    const off = computerSetting(current, false);
    assert.deepEqual(off, { key: 'integrations', scope: scope ?? 'global', value: ['browser', 'git'] });
    assert.deepEqual(computerSetting(off, true).value, ['browser', 'git', 'computer']);
  }
  for (const value of [null, 'browser', [1]]) assert.throws(() => computerSetting({ value }, true), /no settings were changed/);
});

test('computer settings recover from occupied read slots, not permission or connection failures', async () => {
  const full = new Error('busy: 4 reads are already running; retry when one finishes');
  let calls = 0;
  assert.equal(await retryIntegrationRead(async () => {
    if (++calls === 1) throw full;
    return 'off';
  }, () => true), 'off');
  assert.equal(calls, 2);
  for (const failure of ['busy: the control connection is closing', 'access denied']) {
    calls = 0;
    await assert.rejects(retryIntegrationRead(async () => { calls++; throw Error(failure); }, () => true), { message: failure });
    assert.equal(calls, 1);
  }
  calls = 0;
  await assert.rejects(retryIntegrationRead(async () => { calls++; throw full; }, () => calls === 0), /Settings page changed/);
  assert.equal(calls, 1);
});

import test from 'node:test';
import assert from 'node:assert/strict';
import { createAppInstaller } from './appInstaller.ts';

for (const accept of [false, true]) {
  test(`terminal confirmation ${accept ? 'revalidates idle before install' : 'can leave all work running'}`, async () => {
    const decisions = [], installs = [];
    const reserved = Promise.withResolvers();
    const updater = createAppInstaller(() => false, {
      downloadApp: async () => {},
      tryReserveRestart: async confirmed => {
        decisions.push(confirmed);
        if (!confirmed.length) return { state: 'confirmTerminals', ids: [32] };
        return reserved.promise;
      },
      confirmTerminals: async count => { assert.equal(count, 1); return accept; },
      installApp: async () => installs.push('install'),
      restartApp: async () => installs.push('restart'),
    });
    const done = updater.run('release');
    await new Promise(resolve => setImmediate(resolve));
    assert.deepEqual(installs, []);
    assert.deepEqual(decisions, accept ? [[], [32]] : [[]]);
    if (accept) {
      assert.equal(updater.phase.value, 'waiting');
      await updater.run('release'); // no second install while waiting
      reserved.resolve({ state: 'reserved' });
    }
    await done;
    assert.deepEqual(installs, accept ? ['install', 'restart'] : []);
    if (!accept) assert.equal(updater.phase.value, 'idle');
  });
}

test('unpublished work starting during download defers installation until the native gate opens', async () => {
  const download = Promise.withResolvers();
  let busy = false, installs = 0;
  const updater = createAppInstaller(() => false, {
    downloadApp: () => download.promise,
    tryReserveRestart: async () => ({ state: busy ? 'busy' : 'reserved' }),
    installApp: async () => { installs++; }, restartApp: async () => {},
  });
  const done = updater.run('release');
  busy = true;
  download.resolve();
  await new Promise(resolve => setImmediate(resolve));
  assert.equal(updater.phase.value, 'waiting');
  assert.equal(installs, 0);
  busy = false;
  await done;
  assert.equal(installs, 1);
});

for (const [fail, expected] of [
  ['download', ['download', 'download', 'reserve', 'install', 'restart']],
  ['reserve', ['download', 'reserve', 'download', 'reserve', 'install', 'restart']],
  ['install', ['download', 'reserve', 'install', 'release', 'download', 'reserve', 'install', 'restart']],
  ['restart', ['download', 'reserve', 'install', 'restart', 'release', 'reserve', 'restart']],
]) {
  test(`${fail} failure stays visible and retry completes the update`, async () => {
    const calls = [];
    const step = name => async () => {
      calls.push(name);
      if (name === fail && calls.filter(s => s === name).length === 1) throw Error('denied');
    };
    const updater = createAppInstaller(() => false, {
      downloadApp: step('download'), installApp: step('install'), restartApp: step('restart'),
      tryReserveRestart: async () => { await step('reserve')(); return { state: 'reserved' }; },
      releaseRestart: step('release'),
    });
    await updater.run('release');
    assert.match(updater.status.value, /denied/);
    assert.equal(updater.phase.value, 'idle');
    await updater.run('release');
    assert.deepEqual(calls, expected);
    assert.equal(updater.error.value, '');
  });
}

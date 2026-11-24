import test from 'node:test';
import assert from 'node:assert/strict';
import { listedSkills, installedPackages } from './skills.ts';

test('only real skill inventory rows become switches, including paths with spaces', () => {
  const help = 'Skills: /skills list | sources | enable <name>';
  assert.deepEqual(listedSkills(`No installed skills. Browse /skills sources, then import a directory.\n${help}`), []);
  assert.deepEqual(listedSkills(`review [off; project/dial] /my project/review — Review changes\n${help}`), [
    { name: 'review', on: false, origin: 'project/dial', description: 'Review changes' },
  ]);
});

test('package actions target the package, not each skill or a misleading single scope', () => {
  assert.deepEqual(installedPackages('No skill packages installed. Install one with /skills install …'), []);
  assert.deepEqual(installedPackages('tools [project scope] https://example.com/tools at 012abc — skills: review, clean\ntools [user scope] https://example.com/tools at 987def — skills: review, lint'), [
    { name: 'tools', scopes: new Set(['project', 'user']), skills: new Set(['review', 'clean', 'lint']) },
  ]);
});

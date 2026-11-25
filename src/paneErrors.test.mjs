import test from 'node:test';
import assert from 'node:assert/strict';
import { paneError, paneErrorText } from './paneErrors.ts';

const cases = [
  ['Could not run git: No such file or directory (os error 2)', 'Git is not installed'],
  ['Diffs need the project to be a Git repository', 'This project is not a Git repository'],
  ['Reverting needs the project to be a Git repository', 'This project is not a Git repository'],
  ['Cannot read secret.txt: Permission denied (os error 13)', 'Permission denied'],
  ['Cannot open gone.txt: No such file or directory (os error 2)', 'Not found'],
  ['../etc is outside the project folder', 'Outside the project'],
  ['src is a folder', 'That is a folder'],
  ['The file is too large to show', 'The file is too large'],
  ['The project changed before the file panel could load', 'The project changed'],
  ['git commit failed: Author identity unknown\n\n*** Please tell me who you are.', "Git doesn't know who you are"],
  ['Nothing is staged. Stage files first, then commit.', 'Nothing to commit'],
  ['Staging needs the project to be a Git repository', 'This project is not a Git repository'],
  ['That change is no longer in the file. Refresh the diff and try again.', 'The file changed meanwhile'],
  ['The change could not be undone cleanly: error: patch failed: a.txt:1', 'The file changed meanwhile'],
  ['Files outside this project folder are staged too. Unstage them or commit from a terminal.', 'Other files are staged'],
];

test('known failures get a title and a next step', () => {
  for (const [raw, title] of cases) {
    const got = paneError(new Error(raw));
    assert.equal(got.title, title, raw);
    assert.equal(got.detail, raw);
    assert.ok(got.hint, raw);
  }
});

test('unknown failures keep their message', () => {
  assert.deepEqual(paneError('Error: boom'), { title: 'Something went wrong', detail: 'boom' });
  assert.equal(paneErrorText({ message: 'odd' }), 'odd');
  assert.equal(paneError(undefined).detail, 'Unknown error');
  assert.match(paneErrorText('src is a folder'), /^That is a folder\. .*\(src is a folder\)$/);
});

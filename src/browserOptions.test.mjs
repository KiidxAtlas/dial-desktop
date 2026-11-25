import test from 'node:test';
import assert from 'node:assert/strict';
import { HISTORY_LIMIT, isSearchTemplate, pushHistory, searchTemplate } from './browserOptions.ts';

test('search templates need http(s) and %s, like the backend', () => {
  for (const [template, ok] of [
    ['https://kagi.com/search?q=%s', true],
    ['http://localhost:8888/?q=%s', true],
    ['https://example.com/?q=', false],
    ['javascript:%s', false],
    ['not a url %s', false],
  ]) assert.equal(isSearchTemplate(template), ok, template);
  assert.equal(searchTemplate({ engine: 'duckduckgo', custom: '' }), null);
  assert.equal(searchTemplate({ engine: 'google', custom: '' }), 'https://www.google.com/search?q=%s');
  assert.equal(searchTemplate({ engine: 'custom', custom: ' https://s.example/?q=%s ' }), 'https://s.example/?q=%s');
  assert.equal(searchTemplate({ engine: 'custom', custom: 'bad' }), null);
});

test('history keeps recent pages newest first, once each, capped', () => {
  let list = pushHistory([], 'about:blank', '');
  assert.deepEqual(list, []);
  list = pushHistory(list, 'https://a/', 'A');
  list = pushHistory(list, 'https://b/', '');
  list = pushHistory(list, 'https://a/', '');
  assert.deepEqual(list, [{ url: 'https://a/', title: 'A' }, { url: 'https://b/', title: '' }]);
  for (let i = 0; i < HISTORY_LIMIT + 5; i++) list = pushHistory(list, `https://p${i}/`, '');
  assert.equal(list.length, HISTORY_LIMIT);
  assert.equal(list[0].url, `https://p${HISTORY_LIMIT + 4}/`);
});

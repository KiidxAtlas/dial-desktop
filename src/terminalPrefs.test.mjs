import test from 'node:test';
import assert from 'node:assert/strict';
import { parseTerminalPrefs } from './terminalPrefs.ts';

test('terminal prefs fall back and clamp field by field', () => {
  const table = [
    [null, { fontSize: 12.5, scrollback: 5000 }],
    ['not json', { fontSize: 12.5, scrollback: 5000 }],
    ['{"fontSize":15,"scrollback":20000}', { fontSize: 15, scrollback: 20000 }],
    ['{"fontSize":2,"scrollback":1e9}', { fontSize: 9, scrollback: 100000 }],
    ['{"fontSize":"big","scrollback":1234.6}', { fontSize: 12.5, scrollback: 1235 }],
  ];
  for (const [raw, want] of table) assert.deepEqual(parseTerminalPrefs(raw), want, String(raw));
});

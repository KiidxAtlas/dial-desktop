import test from 'node:test';
import assert from 'node:assert/strict';
import { boardDetails, boardName, cleanLog, hexRows, parseHex } from './boardConsole.ts';

const board = (over) => ({ path: '/dev/cu.x', transport: 'usb', product: null, manufacturer: null, serial_number: null, usb_id: null, kind: null, hint: null, ...over });

test('names a device by its own name, then the guess, then how it is attached', () => {
  const flipper = board({ product: 'Flipper Ana7one', kind: 'Flipper Zero', manufacturer: 'Flipper Devices Inc.', usb_id: '0483:5740', serial_number: 'flip_Ana7one' });
  assert.equal(boardName(flipper), 'Flipper Ana7one');
  assert.equal(boardDetails(flipper), 'Flipper Zero · Flipper Devices Inc. · USB 0483:5740 · S/N flip_Ana7one');
  const bridge = board({ kind: 'FTDI USB-serial bridge', usb_id: '0403:6001' });
  assert.equal(boardName(bridge), 'FTDI USB-serial bridge');
  assert.equal(boardDetails(bridge), 'USB 0403:6001');
  assert.equal(boardName(board({ transport: 'bluetooth' })), 'Bluetooth serial port');
  assert.equal(boardDetails(board({ transport: 'bluetooth' })), '');
  assert.equal(boardDetails(board({ transport: 'other', product: ' ' })), '');
});

test('reads hex bytes in the usual spellings and refuses anything else', () => {
  for (const text of ['DE AD be ef', '0xDE,0xAD,0xBE,0xEF', 'deadbeef', 'de:ad-be;ef']) {
    assert.deepEqual([...parseHex(text)], [0xde, 0xad, 0xbe, 0xef], text);
  }
  assert.deepEqual([...parseHex('1 2 a')], [1, 2, 10]);
  for (const text of ['', '  ', 'hello', 'abc', '0x', 'de ag']) assert.equal(parseHex(text), null, text);
});

test('hex rows show sixteen bytes with their printable text', () => {
  const bytes = Uint8Array.from([...'Hello, board!\r\n'].map(c => c.charCodeAt(0)).concat([0, 0xff]));
  assert.equal(hexRows(bytes),
    '48 65 6c 6c 6f 2c 20 62 6f 61 72 64 21 0d 0a 00  Hello, board!...\r\n' +
    'ff                                               .\r\n');
  assert.equal(hexRows(new Uint8Array()), '');
});

test('log text drops colour codes and control characters but keeps lines and tabs', () => {
  assert.equal(cleanLog('\x1b[0;32mI (31) boot: ok\x1b[0m\r\n>>> \x07a\tb\rc\x1b]0;title\x07'), 'I (31) boot: ok\n>>> a\tb\nc');
});

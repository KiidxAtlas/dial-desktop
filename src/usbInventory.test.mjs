import assert from 'node:assert/strict';
import test from 'node:test';
import { compareUsb, batteryText, filterUsb, usbReport } from './usbInventory.ts';

const a = { product: 'Phone', kind: 'USB device', usb_id: '1234:abcd', location: '1:1', functions: [], speed: null, interfaces: [] };
const b = { ...a, location: '1:2' };
test('USB scan comparison distinguishes identical models at different ports and ignores enumeration order', () => {
  assert.deepEqual(compareUsb([a, b], [b, a]), []);
  assert.deepEqual(compareUsb([a], [b]), ['Added at 1:2: Phone (1234:abcd)', 'Removed from 1:1: Phone (1234:abcd)']);
  assert.match(compareUsb([b], [{ ...b, functions: ['USB audio'] }])[0], /^Changed at 1:2/);
});
test('battery readings preserve zero and false, not missing measurements', () => {
  assert.equal(batteryText({ percent: 0, usb_powered: false, temperature_c: 0, voltage_mv: null }), '0% charge · USB power off · 0 °C');
});
test('filter, report and compare link and interface details without conflating missing data', () => {
  const x = { ...a, speed: '5 Gb/s', functions: ['USB video', 'USB audio'], interfaces: ['#0: 01/01/00', '#1: 0e/01/00'] };
  assert.deepEqual(filterUsb([b, x], 'VIDEO 1234:ABCD 5'), [x]);
  assert.deepEqual(filterUsb([x], 'printer'), []);
  assert.match(usbReport(a), /Link: not reported/);
  assert.deepEqual(compareUsb([x], [{ ...x, interfaces: [...x.interfaces].reverse() }]), []);
  assert.deepEqual(compareUsb([x], [{ ...x, speed: '480 Mb/s', functions: ['USB video'] }]), ['Changed at 1:1: link 5 Gb/s → 480 Mb/s; removed USB audio']);
});

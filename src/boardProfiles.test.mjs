import test from 'node:test';
import assert from 'node:assert/strict';
import { PROFILES, chipFromLog, detectBoard, dualRows, pinCommand, profile, recipeRoles } from './boardProfiles.ts';
import models from './boardCatalog.json' with { type: 'json' };

test('every recipe wires only pins its board has, and pin ids are unique', () => {
  for (const p of PROFILES) {
    const ids = p.pins.map(pin => pin.id);
    assert.equal(new Set(ids).size, ids.length, p.id);
    for (const r of p.recipes) for (const w of r.wires) assert.ok(ids.includes(w.pin), `${p.id}/${r.id} uses ${w.pin}`);
  }
});

test('recipes never wire a reserved pin, and ESP picks avoid strapping, USB and flash pins', () => {
  for (const p of PROFILES) for (const r of p.recipes) for (const w of r.wires) {
    const pin = p.pins.find(x => x.id === w.pin);
    assert.notEqual(pin.kind, 'reserved', `${p.id}/${r.id} ${w.pin}`);
    if (p.id.startsWith('esp') && pin.number !== undefined) assert.equal(pin.kind, 'io', `${p.id}/${r.id} ${w.pin}`);
  }
});

test('ESP pin facts follow the ESP-IDF tables', () => {
  const pin = (id, gpio) => profile(id).pins.find(p => p.id === `GPIO${gpio}`);
  assert.equal(pin('esp32c6', 12).kind, 'reserved');
  assert.equal(pin('esp32c6', 9).kind, 'caution');
  assert.match(pin('esp32c6', 6).functions[0], /ADC1_CH6/);
  assert.equal(pin('esp32c3', 14).kind, 'reserved');
  assert.equal(pin('esp32', 34).functions.includes('Input only'), true);
  assert.equal(pin('esp32s3', 22), undefined);
  assert.equal(profile('esp32').pins.filter(p => p.number !== undefined).length, 32);
});

test('the Pico header reads down the left side and back up the right', () => {
  const pico = profile('pico');
  assert.equal(pico.pins.length, 40);
  assert.deepEqual(pico.pins.map(p => p.number), Array.from({ length: 40 }, (_, i) => i + 1));
  const rows = dualRows(pico.pins);
  assert.deepEqual([rows[0][0].label, rows[0][1].label], ['GP0', 'VBUS']);
  assert.deepEqual([rows[19][0].label, rows[19][1].label], ['GP15', 'GP16']);
  assert.equal(profile('flipper').pins.length, 18);
});

test('names the chip from ESP boot logs', () => {
  assert.equal(chipFromLog('ESP-ROM:esp32c6-20220919\nBuild:Sep 19 2022'), 'esp32c6');
  assert.equal(chipFromLog('I (24) boot: chip revision: v0.4\nESP-ROM:esp32s3-20210327'), 'esp32s3');
  assert.equal(chipFromLog('ets Jun  8 2016 00:22:57\n\nrst:0x1 (POWERON_RESET)'), 'esp32');
  assert.equal(chipFromLog('I (300) app: chip type: ESP32-C3'), 'esp32c3');
  assert.equal(chipFromLog('temp=21\n>>> '), null);
});

test('detects a board from USB facts, and the boot log wins over a guess', () => {
  const usb = (kind, product = null) => ({ kind, product, usb_id: null });
  assert.equal(detectBoard(usb('Flipper Zero', 'Flipper Ana7one')).id, 'flipper');
  assert.equal(detectBoard(usb('Raspberry Pi RP2040/RP2350', 'Pico W')).id, 'pico');
  assert.equal(detectBoard(usb('Raspberry Pi RP2040/RP2350')).id, null);
  const native = detectBoard(usb('Espressif ESP32 (native USB)', 'USB JTAG/serial debug unit'));
  assert.equal(native.id, null);
  assert.deepEqual(native.candidates, ['esp32c6', 'esp32c3', 'esp32s3']);
  assert.equal(detectBoard(usb('Espressif ESP32 (native USB)'), 'ESP-ROM:esp32c6-20220919').id, 'esp32c6');
  assert.equal(detectBoard(usb('Silicon Labs CP210x bridge')).candidates.includes('bridge'), true);
  assert.deepEqual(detectBoard(usb(null)), { id: null, reason: '', candidates: [] });
});

test('model-specific identities select a guide; shared IDs and partial names do not', () => {
  const detect = (kind, product = '') => detectBoard({ kind, product, usb_id: null });
  const usbIds = models.flatMap(m => m.usb);
  assert.equal(new Set(usbIds).size, usbIds.length);
  assert.ok(usbIds.every(id => /^[0-9a-f]{4}:[0-9a-f]{4}$/.test(id)));
  assert.equal(new Set(PROFILES.map(p => p.id)).size, PROFILES.length);
  for (const m of models) {
    const family = m.name.startsWith('Arduino ') ? 'Arduino board' : 'Adafruit board';
    assert.equal(detect(m.name, 'USB Serial').id, m.id);
    assert.equal(detect(family, m.name).id, m.id);
    const generic = detect(family, 'USB Serial');
    assert.equal(generic.id, null);
    assert.ok(generic.candidates.includes(m.id));
  }
  assert.equal(detect('Adafruit board', 'CircuitPlayground Express').id, 'circuit-playground-express');
  assert.equal(detect('Adafruit board', 'Feather RP2040 CAN').id, null);
  assert.equal(detect(null, 'Micro').id, null);
  assert.equal(detect('Arduino Uno').id, 'uno');
  assert.equal(detectBoard({ kind: 'Arduino Mega 2560', product: null, usb_id: null }, 'ESP-ROM:esp32c3-20210207').id, 'esp32c3');
});

test('pin commands and recipe roles', () => {
  const flipper = profile('flipper'), a7 = flipper.pins.find(p => p.id === 'PA7');
  assert.deepEqual(pinCommand(flipper.pinCommands.high, a7), ['gpio mode PA7 1', 'gpio set PA7 1']);
  const roles = recipeRoles(flipper.recipes.find(r => r.id === 'led'));
  assert.equal(roles.get('PA7'), 'LED long leg (+)');
  assert.equal(recipeRoles(null).size, 0);
});

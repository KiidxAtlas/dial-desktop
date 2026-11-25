import test from 'node:test';
import assert from 'node:assert/strict';
import { maxInspectorWidth, MIN_WIDTH, sidebarFits } from './layoutRoom.ts';

test('the sidebar steps aside only while an open inspector leaves the conversation too little room', () => {
  assert.equal(sidebarFits(900, false), true);
  assert.equal(sidebarFits(1000, true), false);
  assert.equal(sidebarFits(1079, true), false);
  assert.equal(sidebarFits(1080, true), true);
  assert.equal(sidebarFits(1600, true), true);
});

test('a wide panel gets its room: the sidebar steps aside for it sooner than for a list', () => {
  assert.equal(sidebarFits(1150, true, false), true);
  assert.equal(sidebarFits(1150, true, true), false);
  assert.equal(sidebarFits(1239, true, true), false);
  assert.equal(sidebarFits(1240, true, true), true);
  assert.equal(sidebarFits(900, false, true), true);
});

test('the inspector keeps the conversation its room and never drops below its minimum', () => {
  assert.equal(maxInspectorWidth(1000, false), 460);
  assert.equal(maxInspectorWidth(1400, true), 580);
  assert.equal(maxInspectorWidth(600, true), MIN_WIDTH);
});

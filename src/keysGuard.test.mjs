import test from "node:test";
import assert from "node:assert/strict";
import { commandKey, escapeStopsTurn, inOtherField, inTerminal, shortcutLabel, terminalOwnsKey } from "./keysGuard.ts";

/** A stand-in element that is inside the elements `inside` names (by selector part). */
function el(...inside) {
  return {
    closest(selector) {
      return selector.split(",").some((part) => inside.some((name) => part.trim().startsWith(name))) ? this : null;
    },
  };
}
const key = (over) => ({ key: "k", code: "KeyK", metaKey: false, ctrlKey: false, altKey: false, shiftKey: false, ...over });

test("commandKey is ⌘ on macOS and Ctrl elsewhere", () => {
  const cases = [
    [{ metaKey: true, ctrlKey: false }, true, true],
    [{ metaKey: false, ctrlKey: true }, true, false],
    [{ metaKey: false, ctrlKey: true }, false, true],
    [{ metaKey: true, ctrlKey: false }, false, false],
  ];
  for (const [event, mac, want] of cases) assert.equal(commandKey(event, mac), want, JSON.stringify([event, mac]));
});

test("shortcut labels distinguish the platform modifier, literal Control and multi-chord hints", () => {
  for (const [mac, other] of [
    ["⌘/", "Ctrl+/"], ["⌘⌫", "Ctrl+Backspace"], ["⌥⌘↓", "Ctrl+Alt+↓"],
    ["⌃⇧Tab  ⌃Tab", "Ctrl+Shift+Tab  Ctrl+Tab"], ["⌘-click  ⇧-click", "Ctrl+click  Shift+click"],
  ]) {
    assert.equal(shortcutLabel(mac, true), mac);
    assert.equal(shortcutLabel(mac, false), other);
  }
});

test("Esc stops the turn only from the conversation or the prompt box with nothing open", () => {
  const cases = [
    [el(), false, true],
    [null, false, true],
    [el("textarea", "[data-prompt]"), false, true],
    [el("textarea"), false, false],
    [el("input"), false, false],
    [el("[contenteditable"), false, false],
    [el(".xterm", "textarea"), false, false],
    [el(), true, false],
  ];
  for (const [target, popover, want] of cases) assert.equal(escapeStopsTurn(target, popover), want);
  assert.equal(inTerminal(el(".xterm")), true);
  assert.equal(inOtherField(el("textarea", "[data-prompt]")), false);
});

test("a terminal keeps its keys apart from ⌘ on macOS and Ctrl+`", () => {
  assert.equal(terminalOwnsKey(key({ metaKey: true }), true), false);
  assert.equal(terminalOwnsKey(key({ ctrlKey: true }), true), true);
  assert.equal(terminalOwnsKey(key({ ctrlKey: true }), false), true);
  assert.equal(terminalOwnsKey(key({ key: "Escape", code: "Escape" }), true), true);
  assert.equal(terminalOwnsKey(key({ key: "`", code: "Backquote", ctrlKey: true }), false), false);
});

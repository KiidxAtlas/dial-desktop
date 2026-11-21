import test from "node:test";
import assert from "node:assert/strict";
import {
  hasLegacyControlChars,
  isLegacyFunctionKey,
  moveCaretHorizontally,
  normalizeArrowKey,
  sanitizeEditable,
  stripLegacyControlChars,
} from "./macKeys.ts";

test("normalizeArrowKey maps legacy Mac ASCII and AppKit PUA arrow characters", () => {
  assert.equal(normalizeArrowKey("\u001c"), "ArrowLeft");
  assert.equal(normalizeArrowKey("\u001d"), "ArrowRight");
  assert.equal(normalizeArrowKey("\u001e"), "ArrowUp");
  assert.equal(normalizeArrowKey("\u001f"), "ArrowDown");
  assert.equal(normalizeArrowKey("\uf700"), "ArrowUp");
  assert.equal(normalizeArrowKey("\uf701"), "ArrowDown");
  assert.equal(normalizeArrowKey("\uf702"), "ArrowLeft");
  assert.equal(normalizeArrowKey("\uf703"), "ArrowRight");
  assert.equal(normalizeArrowKey("ArrowRight"), "ArrowRight");
  assert.equal(normalizeArrowKey("a"), "a");
});

test("isLegacyFunctionKey and stripLegacyControlChars catch all AppKit PUA function keys", () => {
  assert.equal(isLegacyFunctionKey("\uf700"), true);
  assert.equal(isLegacyFunctionKey("\uf703"), true);
  assert.equal(isLegacyFunctionKey("\u001d"), true);
  assert.equal(isLegacyFunctionKey("ArrowRight"), false);
  assert.equal(isLegacyFunctionKey("a"), false);
  assert.equal(hasLegacyControlChars("hello\uf703"), true);
  assert.equal(hasLegacyControlChars("hello"), false);
  assert.equal(stripLegacyControlChars("\uf702he\u001dllo\uf700"), "hello");
});

test("sanitizeEditable strips stray arrow box characters and keeps the caret at 0 in an empty field", () => {
  const el = {
    value: "\uf703",
    selectionStart: 1,
    selectionEnd: 1,
    setSelectionRange(start, end) {
      this.selectionStart = start;
      this.selectionEnd = end;
    },
  };
  assert.equal(sanitizeEditable(el), true);
  assert.equal(el.value, "");
  assert.equal(el.selectionStart, 0);
  assert.equal(el.selectionEnd, 0);

  const mid = {
    value: "ab\u001dcd",
    selectionStart: 3,
    selectionEnd: 3,
    setSelectionRange(start, end) {
      this.selectionStart = start;
      this.selectionEnd = end;
    },
  };
  assert.equal(sanitizeEditable(mid), true);
  assert.equal(mid.value, "abcd");
  assert.equal(mid.selectionStart, 2);
  assert.equal(mid.selectionEnd, 2);
});

test("moveCaretHorizontally is a no-op on empty fields and steps cleanly on non-empty text", () => {
  const empty = {
    value: "",
    selectionStart: 0,
    selectionEnd: 0,
    called: false,
    setSelectionRange(start, end) {
      this.called = true;
      this.selectionStart = start;
      this.selectionEnd = end;
    },
  };
  moveCaretHorizontally(empty, 1, false);
  assert.equal(empty.called, false);
  assert.equal(empty.selectionStart, 0);

  const field = {
    value: "hello",
    selectionStart: 2,
    selectionEnd: 2,
    selectionDirection: "forward",
    setSelectionRange(start, end, dir) {
      this.selectionStart = start;
      this.selectionEnd = end;
      if (dir) this.selectionDirection = dir;
    },
  };
  moveCaretHorizontally(field, 1, false);
  assert.equal(field.selectionStart, 3);
  moveCaretHorizontally(field, -1, false);
  assert.equal(field.selectionStart, 2);
});

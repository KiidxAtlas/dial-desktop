// macOS WKWebView can hand arrow and function keys to text fields as legacy ASCII control codes
// (U+001C–U+001F) or AppKit NSEvent Private Use Area characters (U+F700–U+F747). Left alone those
// get inserted into <input>/<textarea>, advancing the caret and drawing a missing-glyph box (□).

const LEGACY_ARROW_MAP: Record<string, string> = {
  "\u001c": "ArrowLeft",
  "\u001d": "ArrowRight",
  "\u001e": "ArrowUp",
  "\u001f": "ArrowDown",
  "\uf700": "ArrowUp",
  "\uf701": "ArrowDown",
  "\uf702": "ArrowLeft",
  "\uf703": "ArrowRight",
};

const LEGACY_CONTROL_SINGLE = /[\u001c-\u001f\uf700-\uf747]/;
const LEGACY_CONTROL_GLOBAL = /[\u001c-\u001f\uf700-\uf747]/g;

/** Map legacy Mac/AppKit arrow key characters to standard DOM key names. */
export function normalizeArrowKey(key: string): string {
  return LEGACY_ARROW_MAP[key] ?? key;
}

/** True when `key` is a single legacy Mac arrow code or AppKit function-key PUA character. */
export function isLegacyFunctionKey(key: string): boolean {
  return key.length === 1 && LEGACY_CONTROL_SINGLE.test(key);
}

/** True when `text` contains any legacy Mac arrow or AppKit function-key character. */
export function hasLegacyControlChars(text: string): boolean {
  return LEGACY_CONTROL_SINGLE.test(text);
}

/** Remove legacy Mac arrow and AppKit function-key characters from `value`. */
export function stripLegacyControlChars(value: string): string {
  return value.replace(LEGACY_CONTROL_GLOBAL, "");
}

/** Remove any legacy control characters from an input/textarea while keeping caret offsets sane. */
export function sanitizeEditable(el: {
  value: string;
  selectionStart: number | null;
  selectionEnd: number | null;
  setSelectionRange?: (start: number, end: number) => void;
}): boolean {
  if (!hasLegacyControlChars(el.value)) return false;
  const start = el.selectionStart ?? el.value.length;
  const end = el.selectionEnd ?? start;
  const cleanBeforeStart = stripLegacyControlChars(el.value.slice(0, start)).length;
  const cleanBeforeEnd = stripLegacyControlChars(el.value.slice(0, end)).length;
  el.value = stripLegacyControlChars(el.value);
  try {
    el.setSelectionRange?.(cleanBeforeStart, cleanBeforeEnd);
  } catch {
    // Some input types (e.g. number/date) disallow setSelectionRange.
  }
  return true;
}

/** Move the caret left or right inside a non-empty text control; no-op when empty. */
export function moveCaretHorizontally(
  el: {
    value: string;
    selectionStart: number | null;
    selectionEnd: number | null;
    selectionDirection?: "forward" | "backward" | "none" | null;
    setSelectionRange?: (start: number, end: number, direction?: "forward" | "backward" | "none") => void;
  },
  delta: -1 | 1,
  extend: boolean,
): void {
  const len = el.value.length;
  if (!len || !el.setSelectionRange) return;
  const start = el.selectionStart ?? len;
  const end = el.selectionEnd ?? start;
  try {
    if (!extend) {
      const target =
        start !== end
          ? delta < 0
            ? start
            : end
          : Math.max(0, Math.min(len, start + delta));
      el.setSelectionRange(target, target);
      return;
    }
    const dir = el.selectionDirection === "backward" ? "backward" : "forward";
    if (start === end) {
      const next = Math.max(0, Math.min(len, start + delta));
      if (delta < 0) el.setSelectionRange(next, end, "backward");
      else el.setSelectionRange(start, next, "forward");
    } else if (dir === "backward") {
      const next = Math.max(0, Math.min(len, start + delta));
      if (next <= end) el.setSelectionRange(next, end, "backward");
      else el.setSelectionRange(end, next, "forward");
    } else {
      const next = Math.max(0, Math.min(len, end + delta));
      if (next >= start) el.setSelectionRange(start, next, "forward");
      else el.setSelectionRange(next, start, "backward");
    }
  } catch {
    // Ignore non-selectable input types.
  }
}

function isTextControl(target: EventTarget | null): target is HTMLInputElement | HTMLTextAreaElement {
  return (
    typeof HTMLElement !== "undefined" &&
    (target instanceof HTMLTextAreaElement || target instanceof HTMLInputElement)
  );
}

function isEditableTarget(target: EventTarget | null): boolean {
  if (isTextControl(target)) return true;
  if (typeof HTMLElement !== "undefined" && target instanceof HTMLElement) {
    return target.isContentEditable;
  }
  return false;
}

/**
 * Guard every text field in the app against macOS WKWebView inserting legacy arrow / AppKit
 * function-key characters as placeholder boxes (□). Does not stop propagation so feature
 * handlers (history recall, menus, type page navigation) continue to work normally.
 */
export function installLegacyKeyGuard(doc: Document = document): () => void {
  const onBeforeInput = (event: Event) => {
    const input = event as InputEvent;
    if (typeof input.data === "string" && hasLegacyControlChars(input.data)) {
      event.preventDefault();
    }
  };

  const onKeyDown = (event: KeyboardEvent) => {
    if (event.isComposing || event.defaultPrevented || !isLegacyFunctionKey(event.key)) return;
    if (!isEditableTarget(event.target)) return;
    event.preventDefault();
    if (isTextControl(event.target) && !event.metaKey && !event.ctrlKey && !event.altKey) {
      const normalized = normalizeArrowKey(event.key);
      if (normalized === "ArrowLeft") moveCaretHorizontally(event.target, -1, event.shiftKey);
      else if (normalized === "ArrowRight") moveCaretHorizontally(event.target, 1, event.shiftKey);
    }
  };

  const onInput = (event: Event) => {
    if (isTextControl(event.target)) sanitizeEditable(event.target);
  };

  doc.addEventListener("beforeinput", onBeforeInput, true);
  doc.addEventListener("keydown", onKeyDown);
  doc.addEventListener("input", onInput, true);
  return () => {
    doc.removeEventListener("beforeinput", onBeforeInput, true);
    doc.removeEventListener("keydown", onKeyDown);
    doc.removeEventListener("input", onInput, true);
  };
}

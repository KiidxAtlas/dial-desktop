// Which window-wide shortcuts a key press may trigger, given where focus is. Kept apart from
// App.vue so the decisions can be tested without a DOM.

/** The parts of a keyboard event the decisions read. */
export interface KeyLike {
  key: string;
  code?: string;
  metaKey: boolean;
  ctrlKey: boolean;
  altKey: boolean;
  shiftKey: boolean;
}

/** Something `closest` can be asked of: an element, or a stand-in in tests. */
export interface Closest {
  closest(selector: string): unknown;
}

/** Text fields other than the composer's prompt box, where Esc belongs to the field. */
const FIELD = "input, textarea, select, [contenteditable]:not([contenteditable=false])";
/** The composer's own prompt box carries this attribute. */
const PROMPT = "[data-prompt]";

/** The app's command modifier: ⌘ on macOS, Ctrl elsewhere (Ctrl on a Mac is the terminal's). */
export function commandKey(event: Pick<KeyLike, "metaKey" | "ctrlKey">, mac: boolean): boolean {
  return mac ? event.metaKey : event.ctrlKey;
}

/** Shortcut lists use compact Mac notation in source; render the equivalent chord for this keyboard. */
export function shortcutLabel(keys: string, mac: boolean): string {
  if (mac) return keys;
  return keys.replace(/[⌘⌃⌥⇧]+-?/g, chord => [
    chord.includes("⌘") || chord.includes("⌃") ? "Ctrl" : "",
    chord.includes("⌥") ? "Alt" : "", chord.includes("⇧") ? "Shift" : "",
  ].filter(Boolean).join("+") + "+").replaceAll("⌫", "Backspace");
}

function isClosest(target: unknown): target is Closest {
  return !!target && typeof (target as Closest).closest === "function";
}

/** Focus is in an embedded terminal. */
export function inTerminal(target: unknown): boolean {
  return isClosest(target) && !!target.closest(".xterm");
}

/** Focus is in a text field that is not the composer's prompt box. */
export function inOtherField(target: unknown): boolean {
  return isClosest(target) && !!target.closest(FIELD) && !target.closest(PROMPT);
}

/** Focus is in any text field, the prompt box included, or a terminal. */
export function typingIn(target: unknown): boolean {
  return isClosest(target) && !!target.closest(`${FIELD}, ${PROMPT}, .xterm`);
}

/**
 * A key pressed in a terminal belongs to the shell, except ⌘ shortcuts on macOS (the shell never
 * sees ⌘) and Ctrl+` which shows or hides the terminal itself.
 */
export function terminalOwnsKey(event: KeyLike, mac: boolean): boolean {
  if (mac && event.metaKey) return false;
  if (event.ctrlKey && !event.metaKey && !event.altKey && event.code === "Backquote") return false;
  return true;
}

/** Esc stops the running turn only from the conversation or the prompt box, with nothing else open. */
export function escapeStopsTurn(target: unknown, popoverOpen: boolean): boolean {
  return !popoverOpen && !inTerminal(target) && !inOtherField(target);
}

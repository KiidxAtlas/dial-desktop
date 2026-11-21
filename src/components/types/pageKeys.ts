// The ← and → that flip a type's pages (a book's leaves, a calendar's months).
//
// macOS can hand a web view an arrow as its legacy character instead of `ArrowRight`: U+001C/U+001D
// (the old arrow char codes) or U+F702/U+F703 (AppKit's function-key characters). Left alone those
// reach a focused text field and draw as an empty box, so they count as arrows too.

const LEFT = new Set(["ArrowLeft", "Left", "\u001c", "\uf702"]);
const RIGHT = new Set(["ArrowRight", "Right", "\u001d", "\uf703"]);

/** -1 for ←, 1 for →, 0 for any other key. */
export function arrowOf(event: KeyboardEvent): -1 | 0 | 1 {
  if (event.metaKey || event.ctrlKey || event.altKey || event.shiftKey || event.isComposing) return 0;
  if (LEFT.has(event.key) || event.keyCode === 37) return -1;
  if (RIGHT.has(event.key) || event.keyCode === 39) return 1;
  return 0;
}

/**
 * Whether `event`'s arrow is the page's: not taken by a dialog, nor by a text field with text in
 * it to move through. An empty field (the composer right after a type opens) has no caret to move,
 * so its arrows turn the page, and nothing is typed into it.
 */
export function pageArrow(event: KeyboardEvent): -1 | 0 | 1 {
  const step = arrowOf(event);
  if (!step) return 0;
  const target = event.target instanceof Element ? event.target : null;
  if (target?.closest("select, [role=dialog], [role=menu], [contenteditable]:not([contenteditable=false])")) return 0;
  const field = target?.closest("input, textarea") as HTMLInputElement | HTMLTextAreaElement | null;
  if (field && field.value !== "") {
    // A legacy arrow character would be typed as a box: drop it, leave the caret alone.
    if (event.key.length === 1) event.preventDefault();
    return 0;
  }
  return step;
}

// Keys that answer an approval request from the keyboard (see the approval card in Composer.vue).

export interface KeyLike {
  key: string;
  metaKey: boolean;
  ctrlKey: boolean;
  altKey: boolean;
  shiftKey: boolean;
  isComposing?: boolean;
}

/** ⌘⌫ on macOS, Ctrl+Backspace elsewhere, denies the request on screen. */
export function isDenyKey(event: KeyLike, mac: boolean): boolean {
  if (event.isComposing || event.altKey || event.shiftKey || event.key !== "Backspace") return false;
  return mac ? event.metaKey && !event.ctrlKey : event.ctrlKey && !event.metaKey;
}

/** How the deny key reads on its button. */
export function denyKeyLabel(mac: boolean): string {
  return mac ? "⌘⌫" : "Ctrl+⌫";
}

/**
 * Whether a key pressed in `target` may answer the request: not while text is being edited there,
 * where ⌘⌫ / Ctrl+Backspace deletes text. An empty field (the prompt box waiting) is fine.
 */
export function mayAnswerFrom(target: { tagName?: string; value?: unknown; isContentEditable?: boolean } | null): boolean {
  if (!target) return true;
  if (target.isContentEditable) return false;
  const tag = target.tagName?.toUpperCase();
  if (tag === "INPUT" || tag === "TEXTAREA") return !String(target.value ?? "").length;
  return true;
}

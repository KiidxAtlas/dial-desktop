// Text fields across the app share one set of typing defaults, so a new field can't bring back
// macOS autocorrect (the "D ×" bubble under a word) or auto-capitalised commands and paths.
//
// - Autocorrect and auto-capitalisation are off in every text field and text area.
// - Spell checking is off in one-line fields, which hold names, paths, commands and searches;
//   add `data-prose` to a one-line field where people write sentences. Text areas keep it.
// - An attribute a field sets itself always wins.

/** Input types that take free text; others (checkbox, number, color…) are left alone. */
const TEXT_TYPES = new Set(["", "text", "search", "url", "email"]);

/** The attributes to add to a field: `tag` lower-case, `type` its type attribute (or ""),
 *  `has(name)` whether it already sets attribute `name`. */
export function textInputDefaults(tag: string, type: string, has: (name: string) => boolean): Record<string, string> {
  const textarea = tag === "textarea";
  if (!textarea && !(tag === "input" && TEXT_TYPES.has(type.toLowerCase()))) return {};
  const out: Record<string, string> = {};
  if (!has("autocorrect")) out.autocorrect = "off";
  if (!has("autocapitalize")) out.autocapitalize = "off";
  if (!textarea && !has("spellcheck") && !has("data-prose")) out.spellcheck = "false";
  return out;
}

function apply(target: EventTarget | null) {
  if (!(target instanceof HTMLInputElement || target instanceof HTMLTextAreaElement)) return;
  const defaults = textInputDefaults(target.tagName.toLowerCase(), target.getAttribute("type") ?? "", (name) => target.hasAttribute(name));
  for (const [name, value] of Object.entries(defaults)) target.setAttribute(name, value);
}

/** Apply the defaults to each field as it gains focus, before anything is typed into it. */
export function installTextInputDefaults() {
  document.addEventListener("focusin", (event) => apply(event.target), true);
}

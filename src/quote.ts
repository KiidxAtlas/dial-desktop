// Quoting part of the conversation into the prompt box, Markdown-style ("> ").

/** `text` as a Markdown block quote, blank edge lines dropped, followed by an empty line. */
export function quoteBlock(text: string) {
  const lines = text.replace(/\r\n?/g, "\n").replace(/\s+$/, "").replace(/^\s*\n/, "").split("\n");
  if (!lines.some((line) => line.trim())) return "";
  return lines.map((line) => (line.trim() ? `> ${line}` : ">")).join("\n") + "\n\n";
}

/** The draft with the quote appended: on its own paragraph after any existing text. */
export function withQuote(draft: string, text: string) {
  const quote = quoteBlock(text);
  if (!quote) return draft;
  const head = draft.replace(/\s+$/, "");
  return head ? `${head}\n\n${quote}` : quote;
}

/** A prompt is folded when it is longer than this many lines or characters. */
export const LONG_LINES = 12;
export const LONG_CHARS = 900;
export function isLongPrompt(text: string) {
  return text.length > LONG_CHARS || text.split("\n").length > LONG_LINES;
}

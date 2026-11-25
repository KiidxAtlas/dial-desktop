// Images an answer shows, to hand to the agent with a "Save to…" request.

const IMAGE = /!\[[^\]]*\]\(\s*<?([^)\s>]+)>?(?:\s+"[^"]*")?\s*\)/g;

/** Images an answer shows: Markdown image paths, absolute or relative to `root`. */
export function markdownImages(markdown: string, root: string): string[] {
  const out: string[] = [];
  for (const m of markdown.matchAll(IMAGE)) {
    let path = decodeURI(m[1]).replace(/^file:\/\//, "");
    if (/^[a-z]+:/i.test(path) && !/^[a-z]:[\\/]/i.test(path)) continue; // web and data URLs
    if (!path.startsWith("/") && !/^[a-z]:[\\/]/i.test(path)) path = `${root.replace(/[\\/]+$/, "")}/${path.replace(/^\.\//, "")}`;
    if (!out.includes(path)) out.push(path);
  }
  return out;
}

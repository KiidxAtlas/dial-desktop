// Images an answer shows, to hand to the agent with a "Save to…" request.

const IMAGE = /!\[[^\]]*\]\(\s*<?([^)\s>]+)>?(?:\s+"[^"]*")?\s*\)/g;

/** Images an answer shows: Markdown image paths, absolute or relative to `root`. */
export function markdownImages(markdown: string, root: string): string[] {
  const out: string[] = [];
  for (const m of markdown.matchAll(IMAGE)) {
    let path: string;
    try {
      if (/^file:\/\//i.test(m[1])) {
        const url = new URL(m[1]);
        path = decodeURIComponent(url.pathname);
        if (url.hostname) path = `//${url.hostname}${path}`;
        else path = path.replace(/^\/([a-z]:\/)/i, "$1");
      } else {
        path = decodeURI(m[1]);
        if (/^[a-z][a-z\d+.-]*:/i.test(path) && !/^[a-z]:[\\/]/i.test(path)) continue;
      }
    } catch { continue; } // One malformed link must not discard the rest of the answer.
    if (!/^(?:\/|\\\\|[a-z]:[\\/])/i.test(path)) path = `${root.replace(/[\\/]+$/, "")}/${path.replace(/^\.\//, "")}`;
    if (!out.includes(path)) out.push(path);
  }
  return out;
}

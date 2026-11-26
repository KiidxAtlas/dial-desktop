// Joining and stripping project paths on any platform. The backend gives project-relative paths
// `/`-separated; the project root is native (`C:\Users\me\app` on Windows).

/** The separator `root` uses: `\` for a Windows path, `/` otherwise. */
export function separatorOf(root: string): "/" | "\\" {
  return /^[A-Za-z]:[\\/]|^\\\\/.test(root) || (root.includes("\\") && !root.includes("/")) ? "\\" : "/";
}

/** `relative` (`/`-separated) inside `root`, as a native absolute path. */
export function joinPath(root: string, relative: string): string {
  const sep = separatorOf(root);
  const rest = sep === "\\" ? relative.replace(/^[\\/]+/, "").replace(/\//g, sep) : relative.replace(/^\/+/, "");
  if (!rest) return root;
  return root.endsWith("/") || (sep === "\\" && root.endsWith(sep)) ? root + rest : root + sep + rest;
}

/** `path` relative to `root`, `/`-separated; `path` unchanged (but `/`-separated) when outside it.
 * Windows roots compare case-insensitively and accept either separator. */
export function relativeTo(root: string, path: string): string {
  const windows = separatorOf(root) === "\\";
  const norm = (p: string) => (windows ? p.replace(/\\/g, "/") : p).replace(/\/+$/, "");
  const base = norm(root), full = windows ? path.replace(/\\/g, "/") : path;
  const same = (a: string, b: string) => (windows ? a.toLowerCase() === b.toLowerCase() : a === b);
  if (root === "/" && full.startsWith("/")) return full.slice(1);
  if (base && same(full.slice(0, base.length), base) && full[base.length] === "/") return full.slice(base.length + 1);
  return full;
}

/** Decode a file citation, not a URL or a fragment. This formats its display path; the backend
 * still resolves it inside the project. A malformed percent escape throws URIError. */
export function projectFileLink(root: string, href: string) {
  if (!href || href.startsWith("#")) return null;
  const decoded = decodeURIComponent(href.replace(/[?#].*$/, ""));
  // Markdown percent-encodes Windows backslashes. A single drive separator is not a URL scheme.
  if (/^[a-z][a-z\d+.-]*:/i.test(href) &&
      !(separatorOf(root) === "\\" && /^[a-z]:[\\/](?![\\/])/i.test(decoded))) return null;
  const path = relativeTo(root, decoded).replace(/^\.\//, "");
  if (!path) return null;
  const match = /#L([1-9]\d*)(?:-L([1-9]\d*))?$/.exec(href);
  const start = Number(match?.[1]), end = Number(match?.[2] ?? start);
  const lines = Number.isSafeInteger(start) && Number.isSafeInteger(end) && end >= start ? { start, end } : null;
  return { path, lines };
}

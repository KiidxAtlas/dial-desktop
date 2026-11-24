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

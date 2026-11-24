// Project file paths in terminal output (`src/app.ts:12`, `./a/b.rs`, an absolute path inside the
// project), so a click opens them in the Files panel. URLs are left to the web-links addon.

export interface PathLink {
  /** 0-based index of the first character in the line, and one past the last. */
  start: number;
  end: number;
  /** Project-relative, `/`-separated. */
  path: string;
  line: number | null;
}

// A path with at least one folder and a file extension, after a line start, space, quote or bracket.
const PATH = /(?<=^|[\s'"(\[<=])((?:\.{0,2}\/|[A-Za-z]:[\\/])?(?:[\w@.+-]+[\\/])+[\w@.+-]*\.[A-Za-z0-9]+)(?::(\d+)(?::\d+)?)?/g;

/** Path links in one terminal line. `inProject` turns an absolute path into a project-relative one
 * (see `relativeTo` in paths.ts); absolute paths it leaves unchanged are outside and skipped. */
export function findPathLinks(text: string, inProject: (absolute: string) => string): PathLink[] {
  const out: PathLink[] = [];
  for (const match of text.matchAll(PATH)) {
    const raw = match[1];
    const absolute = /^([A-Za-z]:)?[\\/]/.test(raw);
    let path = absolute ? inProject(raw) : raw.replace(/\\/g, "/").replace(/^\.\//, "");
    if (absolute && /^([A-Za-z]:)?[\\/]/.test(path)) continue;
    if (path.startsWith("../")) continue;
    path = path.replace(/\/+$/, "");
    const start = match.index ?? 0;
    out.push({ start, end: start + match[0].length, path, line: match[2] ? Number(match[2]) : null });
  }
  return out;
}

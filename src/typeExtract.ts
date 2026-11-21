// Turning an answer into items of a type: pure functions, no Vue or Tauri, so they test alone.
// Mirrors the field kinds of `src-tauri/src/types.rs`; Rust re-checks every value it is given.

export type FieldKind =
  | "text" | "longtext" | "markdown" | "list" | "steps" | "image" | "images"
  | "time" | "date" | "datetime" | "number" | "url" | "tags" | "rating" | "toggle";

export type Layout = "book" | "checklist" | "board" | "wheel" | "calendar" | "cards" | "table" | "custom";

export interface Field { key: string; label: string; kind: FieldKind }

export interface TypeView {
  layout: Layout;
  titleField: string | null;
  subtitleField: string | null;
  imageField: string | null;
  timeField: string | null;
  reminders: boolean;
  leadMinutes: number[];
  dailyReset: boolean;
  tagline: string;
  /** A book's pages: `cookbook`, `journal`, `guide`, or empty to pick from the fields. */
  style: string;
}

export interface Blueprint {
  fields: Field[];
  view: TypeView;
}

export type Fields = Record<string, unknown>;

interface Section { heading: string; lines: string[] }

const BULLET = /^\s*(?:[-*+•]|\d+[.)])\s+(.*)$/;
const ORDERED = /^\s*\d+[.)]\s+/;
const HEADING = /^\s{0,3}#{1,6}\s+(.*?)\s*#*\s*$/;
/** A bold line standing in for a heading: `**Ingredients**` or `**Ingredients:**`. */
const BOLD_HEADING = /^\s*\*\*([^*]{1,60}?):?\*\*:?\s*$/;
const IMAGE = /!\[[^\]]*\]\(\s*<?([^)\s>]+)>?(?:\s+"[^"]*")?\s*\)/g;

/** Plain text of a line of Markdown: no emphasis, links as their words, no inline code ticks. */
export function plain(text: string): string {
  return text
    .replace(/!\[[^\]]*\]\([^)]*\)/g, "")
    .replace(/\[([^\]]*)\]\([^)]*\)/g, "$1")
    .replace(/(\*\*|__)(.*?)\1/g, "$2")
    .replace(/(^|[^*])\*([^*]+)\*/g, "$1$2")
    .replace(/`([^`]*)`/g, "$1")
    .replace(/\s+/g, " ")
    .trim();
}

function sections(markdown: string): Section[] {
  const out: Section[] = [{ heading: "", lines: [] }];
  let fence = false;
  for (const line of markdown.split(/\r?\n/)) {
    if (/^\s*(```|~~~)/.test(line)) fence = !fence;
    const heading = !fence && (HEADING.exec(line) ?? BOLD_HEADING.exec(line));
    if (heading) out.push({ heading: plain(heading[1]), lines: [] });
    else out[out.length - 1].lines.push(line);
  }
  return out;
}

const words = (text: string) => text.toLowerCase().replace(/[^a-z0-9]+/g, " ").trim();

/** Whether a heading names this field: "Ingredients" for `ingredients`; `loose` also takes
 * common synonyms for its kind ("Method" for a steps field). */
function names(heading: string, field: Field, loose = false): boolean {
  const h = words(heading);
  if (!h) return false;
  const label = words(field.label);
  const key = words(field.key.replace(/_/g, " "));
  const stem = (w: string) => w.replace(/s$/, "");
  if ([label, key].some((n) => n && (h === n || stem(h) === stem(n) || h.includes(n) || n.includes(h)))) return true;
  if (!loose) return false;
  const synonyms: Partial<Record<FieldKind, string[]>> = {
    steps: ["instructions", "method", "directions", "steps", "how to", "preparation"],
    list: ["ingredients", "what you need", "materials", "supplies", "items"],
  };
  return (synonyms[field.kind] ?? []).some((s) => h.includes(s));
}

function listOf(lines: string[], ordered?: boolean): string[] {
  return lines
    .filter((l) => BULLET.test(l) && (ordered === undefined || ORDERED.test(l) === ordered))
    .map((l) => plain(BULLET.exec(l)![1]))
    .filter(Boolean);
}

/** `HH:MM` (24 hour) from text like "7:30 am", "19:05" or "7pm". */
export function findTime(text: string): string | null {
  for (const m of text.matchAll(/\b(\d{1,2})(?::(\d{2}))?\s*(am|pm|a\.m\.|p\.m\.)?(?!\d)(?!:\d)/gi)) {
    if (!m[2] && !m[3]) continue;
    let hour = Number(m[1]);
    const minute = Number(m[2] ?? 0);
    const half = m[3]?.toLowerCase()[0];
    if (half === "p" && hour < 12) hour += 12;
    if (half === "a" && hour === 12) hour = 0;
    if (hour > 23 || minute > 59) continue;
    return `${String(hour).padStart(2, "0")}:${String(minute).padStart(2, "0")}`;
  }
  return null;
}

/** The line without its time and the separator after it: "7:00 – Wake up" → "Wake up". */
export function withoutTime(text: string): string {
  return text
    .replace(/\s*(?:\b(?:at|by)\s+|@\s*)?\b\d{1,2}(?::\d{2}\s*(?:am|pm|a\.m\.|p\.m\.)?|\s*(?:am|pm|a\.m\.|p\.m\.))(?!\d)(?:\s*[-–—]\s*\d{1,2}(?::\d{2})?\s*(?:am|pm)?)?/i, " ")
    .replace(/^\s*[-–—:|·]+\s*/, "")
    .trim();
}

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

/** "Label: value" anywhere in the answer, for short fields like "Serves: 4". */
function labelled(markdown: string, field: Field): string | null {
  for (const line of markdown.split(/\r?\n/)) {
    const text = plain(line.replace(BULLET, "$1"));
    const at = text.indexOf(":");
    if (at < 1 || at > 40) continue;
    if (names(text.slice(0, at), { ...field, kind: "text" })) {
      const value = text.slice(at + 1).trim();
      if (value) return value;
    }
  }
  return null;
}

/**
 * The fields of one item from an answer. Headings that name a field fill it ("Ingredients" →
 * a list field); the first heading or line is the title; what no field took becomes the first
 * Markdown field, so nothing the user saved is lost.
 */
export function extractFields(def: Blueprint, markdown: string, images: string[]): Fields {
  const parts = sections(markdown.trim());
  const fields: Fields = {};
  const used = new Set<Section>();
  const titleKey = def.view.titleField;
  const firstHeading = parts.find((p) => p.heading);
  const firstLine = parts[0].lines.map(plain).find(Boolean) ?? "";
  const title = (firstHeading?.heading || firstLine).slice(0, 160);
  const free = (p: Section) => !used.has(p) && !!p.heading;

  for (const field of def.fields) {
    const section = parts.find((p) => free(p) && names(p.heading, field)) ?? parts.find((p) => free(p) && names(p.heading, field, true));
    const take = (value: unknown) => {
      fields[field.key] = value;
      if (section) used.add(section);
    };
    switch (field.kind) {
      case "list":
      case "tags": {
        const list = section ? listOf(section.lines) : [];
        if (list.length) take(list);
        else if (field.kind === "tags") {
          const value = labelled(markdown, field);
          if (value) take(value.split(/\s*,\s*/).filter(Boolean));
        }
        break;
      }
      case "steps": {
        const list = section ? listOf(section.lines) : [];
        if (list.length) take(list);
        break;
      }
      case "image":
        if (images[0]) fields[field.key] = images[0];
        break;
      case "images":
        if (images.length) fields[field.key] = images;
        break;
      case "markdown":
      case "longtext":
        if (section) take(section.lines.join("\n").trim());
        break;
      case "time": {
        const value = labelled(markdown, field) ?? markdown;
        const time = findTime(value);
        if (time && (labelled(markdown, field) || field.key === def.view.timeField)) fields[field.key] = time;
        break;
      }
      case "date": {
        const m = /\b(\d{4}-\d{2}-\d{2})\b/.exec(labelled(markdown, field) ?? markdown);
        if (m) fields[field.key] = m[1];
        break;
      }
      case "url": {
        const m = /\bhttps?:\/\/[^\s)>\]]+/.exec(markdown);
        if (m) fields[field.key] = m[0];
        break;
      }
      case "number":
      case "rating": {
        const value = labelled(markdown, field);
        const n = value && /-?\d+(?:\.\d+)?/.exec(value);
        if (n) fields[field.key] = Number(n[0]);
        break;
      }
      case "text":
      case "datetime": {
        if (field.key === titleKey) fields[field.key] = title;
        else {
          const value = labelled(markdown, field) ?? (section ? plain(section.lines.join(" ")) : null);
          if (value) take(value.slice(0, 2000));
        }
        break;
      }
      case "toggle":
        break;
    }
  }

  // Everything no field took, so the item keeps the whole answer.
  const rest = def.fields.find((f) => (f.kind === "markdown" || f.kind === "longtext") && fields[f.key] === undefined);
  if (rest) {
    const left = parts
      .filter((p) => !used.has(p) && p !== firstHeading)
      .map((p) => (p.heading && p !== parts[0] ? `### ${p.heading}\n` : "") + p.lines.join("\n"))
      .join("\n")
      .replace(firstHeading ? "" : firstLine, "")
      .replace(IMAGE, "")
      .trim();
    if (left) fields[rest.key] = left;
  }
  return fields;
}

/**
 * The items an answer becomes. A checklist or a wheel takes each line of a list as its own item
 * ("7:00 – Wake up" → a task at 07:00); a board takes each image; other layouts keep the answer
 * as one item.
 */
export function splitIntoItems(def: Blueprint, markdown: string, images: string[]): Fields[] {
  const titleKey = def.view.titleField;
  const layout = def.view.layout;
  if ((layout === "checklist" || layout === "wheel" || layout === "calendar") && titleKey) {
    const lines = markdown.split(/\r?\n/);
    const rows = lines.filter((l) => BULLET.test(l) || /^\s*\|.*\|\s*$/.test(l));
    const items: Fields[] = [];
    for (const row of rows) {
      let text: string;
      if (row.trim().startsWith("|")) {
        const cells = row.split("|").map((c) => plain(c)).filter(Boolean);
        if (!cells.length || cells.every((c) => /^:?-+:?$/.test(c))) continue;
        text = cells.join(" – ");
      } else text = plain(BULLET.exec(row)![1]);
      const time = def.view.timeField ? findTime(text) : null;
      const title = (time ? withoutTime(text) : text).replace(/[.:]$/, "").trim();
      if (!title) continue;
      const item: Fields = { [titleKey]: title.slice(0, 300) };
      if (time && def.view.timeField) item[def.view.timeField] = time;
      items.push(item);
    }
    // A table's header row is not a task.
    if (items.length > 1 && rows[0]?.trim().startsWith("|") && rows[1] && /^\s*\|[\s:|-]+\|\s*$/.test(rows[1])) items.shift();
    if (items.length) return items.slice(0, 200);
  }
  if (layout === "board" && images.length > 1) {
    const imageKey = def.view.imageField;
    if (imageKey) return images.map((path) => ({ [imageKey]: path, ...(titleKey ? { [titleKey]: "" } : {}) }));
  }
  return [extractFields(def, markdown, images)];
}

/** The local day as `YYYY-MM-DD`. */
export function today(now = new Date()): string {
  return `${now.getFullYear()}-${String(now.getMonth() + 1).padStart(2, "0")}-${String(now.getDate()).padStart(2, "0")}`;
}

/** When an item's time next falls, from `HH:MM` (today) or `YYYY-MM-DD HH:MM`. */
export function occurrence(value: unknown, now = new Date()): Date | null {
  if (typeof value !== "string") return null;
  const full = /^(\d{4})-(\d{2})-(\d{2})[ T](\d{2}):(\d{2})/.exec(value.trim());
  if (full) return new Date(+full[1], +full[2] - 1, +full[3], +full[4], +full[5]);
  const time = /^(\d{1,2}):(\d{2})$/.exec(value.trim());
  if (!time) return null;
  return new Date(now.getFullYear(), now.getMonth(), now.getDate(), +time[1], +time[2]);
}

/** Whether a checked item still counts as done: a daily list unticks the next day. */
export function isDone(item: { done: boolean; doneOn: string | null }, dailyReset: boolean, now = new Date()): boolean {
  return item.done && (!dailyReset || item.doneOn === today(now));
}

/**
 * The day (and time, if any) a calendar places a value on: `YYYY-MM-DD`, `YYYY-MM-DD HH:MM`, or
 * a date written out ("Nov 22 2026 3:00 PM") as older types kept it in plain text.
 */
export function dateOf(value: unknown): { at: Date; allDay: boolean } | null {
  if (typeof value !== "string" || !value.trim()) return null;
  const text = value.trim();
  const iso = /^(\d{4})-(\d{2})-(\d{2})(?:[ T](\d{2}):(\d{2}))?/.exec(text);
  if (iso) {
    const allDay = iso[4] === undefined;
    const at = new Date(+iso[1], +iso[2] - 1, +iso[3], allDay ? 0 : +iso[4], allDay ? 0 : +iso[5]);
    return Number.isNaN(at.getTime()) ? null : { at, allDay };
  }
  const time = findTime(text);
  const clock = time ? time.split(":").map(Number) : [0, 0];
  const year = /\b(\d{4})\b/.exec(text);
  const thisYear = new Date().getFullYear();
  // "Nov 22", "22 November", "November 22nd, 2026"; then "11/22" (month first) or "11/22/2026".
  const MONTH = "(jan|feb|mar|apr|may|jun|jul|aug|sep|oct|nov|dec)[a-z]*\\.?";
  let month = -1;
  let day = 0;
  const named = new RegExp(`\\b${MONTH}\\s+(\\d{1,2})(?:st|nd|rd|th)?\\b`, "i").exec(text);
  const dayFirst = new RegExp(`\\b(\\d{1,2})(?:st|nd|rd|th)?\\s+(?:of\\s+)?${MONTH}`, "i").exec(text);
  const numeric = /\b(\d{1,2})\/(\d{1,2})(?:\/(\d{2,4}))?\b/.exec(text);
  const MONTHS = ["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"];
  if (named) [month, day] = [MONTHS.indexOf(named[1].toLowerCase()), +named[2]];
  else if (dayFirst) [month, day] = [MONTHS.indexOf(dayFirst[2].toLowerCase()), +dayFirst[1]];
  else if (numeric) [month, day] = [+numeric[1] - 1, +numeric[2]];
  else return null;
  const y = year ? +year[1] : numeric?.[3] ? 2000 + (+numeric[3] % 100) : thisYear;
  const at = new Date(y, month, day, clock[0], clock[1]);
  if (month < 0 || month > 11 || at.getMonth() !== month || at.getDate() !== day) return null;
  return { at, allDay: !time };
}

/** The local day of `at` as `YYYY-MM-DD`: the key a calendar groups by. */
export const dayKey = (at: Date) => today(at);

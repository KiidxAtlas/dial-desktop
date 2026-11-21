// A book's pages: which page shows what, worked out from the type's fields, so that every item
// gets as many pages as it needs (turned, never scrolled) and each style its own kind of page.
//
// Pages are measured in lines of body text. Every page sets its type in units of its own width
// (`cqw`), so a line holds about the same number of characters whatever size the book is drawn
// at, and these estimates hold at any window size.

import type { Field, Fields, Item, TypeDef } from "../../savedTypes";

export type BookStyle = "cookbook" | "journal" | "guide";

/** One run of content on a page. `start` numbers a continued list or method where it left off. */
export type Block =
  | { kind: "heading"; text: string; more: boolean }
  | { kind: "items"; field: Field; items: string[]; start: number; ordered: boolean }
  | { kind: "prose"; field: Field; text: string; first: boolean }
  | { kind: "note"; field: Field; text: string }
  | { kind: "value"; field: Field; value: unknown };

export type Page =
  | { kind: "blank" }
  | { kind: "cover" }
  | { kind: "inside" }
  | { kind: "contents"; from: number; to: number }
  /** An item's picture: its photo, or its drawing. */
  | { kind: "art"; item: Item; number: number }
  /** An item's first page of words: its title and what comes first. */
  | { kind: "head"; item: Item; number: number; blocks: Block[] }
  | { kind: "flow"; item: Item; number: number; blocks: Block[]; folio: number }
  /** The rest of an item's last leaf: a closing ornament, room to write. */
  | { kind: "tail"; item: Item; number: number };

export interface Book {
  spreads: { left: Page; right: Page }[];
  /** The spread each item opens on, by item index. */
  starts: number[];
  /** The spread the contents start on. */
  contents: number;
}

/** Lines of body text a page holds, and its first page under the title. */
export const PAGE_LINES = 27;
/** Lines the title block of a head page takes (see `.title-block` in BookPage.vue). */
export function titleLines(def: TypeDef, item: Item, style: BookStyle): number {
  const text = (key: string | null) => (key ? textOf(item.fields[key]).trim() : "");
  const title = text(def.view.titleField) || "Untitled";
  // The title is set at 2.5em: about 24 characters a line, each line near 2.7 body lines.
  let lines = Math.ceil(title.length / 24) * 2.7 + 1.5;
  const sub = text(def.view.subtitleField);
  if (sub) lines += linesOf(sub) + 0.4;
  if (metaFields(def, item, style).length) lines += 3.2;
  if (def.fields.some((f) => f.kind === "tags" && present(item.fields, f.key))) lines += 2;
  return lines + (style === "journal" ? 1 : 0);
}
/** Characters in a line of body text (conservative: a missed estimate is cut, not scrolled). */
export const LINE_CHARS = 62;
/** Contents lines on a contents page. */
export const CONTENTS_LINES = 14;

const INGREDIENT = /ingredient|shopping|you.?ll need|what.?you.?need|pantry|component/i;
const METHOD = /method|step|instruction|direction|how|prep/i;
const NOTE = /note|tip|variation|serve|storage|story|memory|why/i;

/** The style a book draws: the one its blueprint asks for, or the one its fields suit. */
export function bookStyle(def: TypeDef): BookStyle {
  const style = def.view.style;
  if (style === "cookbook" || style === "journal" || style === "guide") return style;
  const words = `${def.name} ${def.description} ${def.fields.map((f) => `${f.key} ${f.label}`).join(" ")}`;
  if (/recipe|cook|kitchen|ingredient|bak(e|ing)|dish|meal|food/i.test(words)) return "cookbook";
  if (/journal|diary|log|entry|gratitude|dream|reflection/i.test(words) || def.fields.some((f) => f.kind === "date" || f.kind === "datetime")) return "journal";
  return "guide";
}

function textOf(value: unknown): string {
  if (Array.isArray(value)) return value.map(String).join("\n");
  return value === null || value === undefined ? "" : String(value);
}

function linesOf(text: string, width = LINE_CHARS): number {
  return text.split("\n").reduce((n, line) => n + Math.max(1, Math.ceil(line.length / width)), 0);
}

/** What a field is for on a page. */
export type Role = "ingredients" | "method" | "note" | "body" | "meta" | "tags" | "skip";

export function roleOf(def: TypeDef, field: Field, style: BookStyle): Role {
  const v = def.view;
  if (field.key === v.titleField || field.key === v.subtitleField || field.key === v.imageField) return "skip";
  if (field.kind === "image" || field.kind === "images") return "skip";
  if (field.kind === "tags") return "tags";
  const name = `${field.key} ${field.label}`;
  if (field.kind === "steps") return "method";
  if (field.kind === "list") return style === "cookbook" && (INGREDIENT.test(name) || !METHOD.test(name)) ? "ingredients" : METHOD.test(name) ? "method" : "body";
  if (field.kind === "longtext" || field.kind === "markdown") return style === "cookbook" && NOTE.test(name) ? "note" : "body";
  return "meta";
}

/** The fields shown with the title: servings, times, ratings… */
export function metaFields(def: TypeDef, item: Item, style: BookStyle): Field[] {
  // A journal shows its day in the running head already.
  return def.fields.filter((f) => roleOf(def, f, style) === "meta" && present(item.fields, f.key) && !(style === "journal" && f.key === def.view.timeField));
}

function present(fields: Fields, key: string): boolean {
  const value = fields[key];
  return value !== undefined && value !== null && value !== "" && !(Array.isArray(value) && !value.length);
}

/** Split prose into paragraphs that each fit a page, long ones cut at sentences. */
function paragraphs(text: string): string[] {
  const out: string[] = [];
  for (const para of text.split(/\n\s*\n/)) {
    const trimmed = para.trim();
    if (!trimmed) continue;
    if (linesOf(trimmed) <= PAGE_LINES - 4) {
      out.push(trimmed);
      continue;
    }
    let run = "";
    for (const sentence of trimmed.match(/[^.!?]+[.!?]*\s*/g) ?? [trimmed]) {
      if (run && linesOf(run + sentence) > PAGE_LINES - 4) {
        out.push(run.trim());
        run = "";
      }
      run += sentence;
    }
    if (run.trim()) out.push(run.trim());
  }
  return out;
}

/** One unit of flow: a heading goes with what follows it. */
type Unit = { block: Block; lines: number; heading?: boolean };

function unitsFor(def: TypeDef, item: Item, style: BookStyle): Unit[] {
  const units: Unit[] = [];
  const fields = def.fields.filter((f) => present(item.fields, f.key));
  const order: Role[] = style === "cookbook" ? ["ingredients", "method", "body", "note"] : ["body", "ingredients", "method", "note"];
  for (const role of order) {
    for (const field of fields.filter((f) => roleOf(def, f, style) === role)) {
      const value = item.fields[field.key];
      const label = field.label || field.key;
      if (role === "ingredients" || role === "method" || field.kind === "list" || field.kind === "steps") {
        const items = (Array.isArray(value) ? value : textOf(value).split("\n")).map(String).filter((s) => s.trim());
        const ordered = role === "method" || field.kind === "steps";
        units.push({ block: { kind: "heading", text: label, more: false }, lines: 1.8, heading: true });
        // Short ingredients sit in two columns (see `columns` in BookPage.vue): half a line each.
        const halved = !ordered && items.length >= 6 && items.every((s) => s.length <= 26);
        // Each entry is its own unit, so a long method runs on over the page.
        items.forEach((entry, i) =>
          units.push({
            block: { kind: "items", field, items: [entry], start: i, ordered },
            lines: halved ? 0.5 : linesOf(entry, ordered ? LINE_CHARS - 6 : role === "ingredients" ? LINE_CHARS - 4 : LINE_CHARS) + (ordered ? 0.7 : 0.15),
          }),
        );
      } else if (role === "note") {
        const text = textOf(value).trim();
        units.push({ block: { kind: "note", field, text }, lines: linesOf(text, LINE_CHARS - 8) * 1.1 + 2.6 });
      } else if (field.kind === "longtext" || field.kind === "markdown") {
        const shown = fields.filter((f) => roleOf(def, f, style) === "body").length > 1 || style === "cookbook";
        if (shown) units.push({ block: { kind: "heading", text: label, more: false }, lines: 1.8, heading: true });
        paragraphs(textOf(value)).forEach((text, i) =>
          units.push({ block: { kind: "prose", field, text, first: i === 0 && !shown }, lines: linesOf(text) + 0.8 }),
        );
      } else {
        units.push({ block: { kind: "value", field, value }, lines: 2 });
      }
    }
  }
  return units;
}

/** Merge neighbouring entries of one list into one block, as they are drawn. */
function merge(blocks: Block[]): Block[] {
  const out: Block[] = [];
  for (const block of blocks) {
    const prev = out[out.length - 1];
    if (block.kind === "items" && prev?.kind === "items" && prev.field.key === block.field.key) {
      prev.items.push(...block.items);
    } else {
      out.push(block.kind === "items" ? { ...block, items: [...block.items] } : block);
    }
  }
  return out;
}

/** Lay `units` onto pages: the first holds `first` lines, the rest a full page. */
function paginate(units: Unit[], first: number): Block[][] {
  const pages: Block[][] = [[]];
  let room = first;
  for (let i = 0; i < units.length; i++) {
    const unit = units[i];
    // A heading never ends a page: it moves over with the first thing under it.
    const need = unit.heading && units[i + 1] ? unit.lines + Math.min(units[i + 1].lines, 3) : unit.lines;
    const page = pages[pages.length - 1];
    if (need > room && page.length) {
      pages.push([]);
      room = PAGE_LINES;
      // A list or method that runs over names itself again, "continued".
      const prev = units.slice(0, i).reverse().find((u) => u.heading);
      if (!unit.heading && prev && unit.block.kind === "items") {
        pages[pages.length - 1].push({ ...(prev.block as Extract<Block, { kind: "heading" }>), more: true });
        room -= prev.lines;
      }
    }
    pages[pages.length - 1].push(unit.block);
    room -= unit.lines;
  }
  return pages.map(merge);
}

/** The pages of one item: always an even run, opening on a left page. */
export function itemPages(def: TypeDef, item: Item, number: number, style: BookStyle): Page[] {
  const units = unitsFor(def, item, style);
  // The running head and the gap under the title take about three lines.
  const [head, ...rest] = paginate(units, PAGE_LINES - titleLines(def, item, style) - 1);
  const pages: Page[] = [{ kind: "art", item, number }, { kind: "head", item, number, blocks: head ?? [] }];
  rest.forEach((blocks) => pages.push({ kind: "flow", item, number, blocks, folio: 0 }));
  if (pages.length % 2) pages.push({ kind: "tail", item, number });
  return pages;
}

/** The whole book: cover, contents, then each item's pages, as spreads. */
export function bookOf(def: TypeDef, items: Item[], style: BookStyle = bookStyle(def)): Book {
  const spreads: Book["spreads"] = [{ left: { kind: "blank" }, right: { kind: "cover" } }];
  const contents = spreads.length;
  // The inside cover faces the first contents page; long contents run on over more spreads.
  const contentsPages = Math.max(1, Math.ceil(items.length / CONTENTS_LINES));
  const front: Page[] = [{ kind: "inside" }];
  for (let p = 0; p < contentsPages; p++) front.push({ kind: "contents", from: p * CONTENTS_LINES, to: Math.min(items.length, (p + 1) * CONTENTS_LINES) });
  if (front.length % 2) front.push({ kind: "blank" });
  for (let i = 0; i < front.length; i += 2) spreads.push({ left: front[i], right: front[i + 1] });
  const starts: number[] = [];
  items.forEach((item, index) => {
    starts.push(spreads.length);
    const pages = itemPages(def, item, index + 1, style);
    for (let i = 0; i < pages.length; i += 2) spreads.push({ left: pages[i], right: pages[i + 1] });
  });
  // Folios: page numbers, counted from the first item's first page.
  let folio = 0;
  for (const s of spreads.slice(contents + Math.ceil(front.length / 2))) {
    for (const page of [s.left, s.right]) {
      folio++;
      if (page.kind === "flow") page.folio = folio;
    }
  }
  return { spreads, starts, contents };
}

/** The page number an item opens on (its art page is 1 for the first item). */
export function folioOf(book: Book, index: number): number {
  const first = book.starts[0] ?? 0;
  return ((book.starts[index] ?? first) - first) * 2 + 1;
}

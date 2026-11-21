// Interactive answer cards (`dial-app` blocks): declarative JSON the app renders, never code it runs.
// Text fields are Markdown. A block without `kind` is the original branching `screens` card.
import { formulaNames, parseFormula, type Formula } from './formula';

/** A follow-up the reader can put in the prompt box; it is never sent automatically. */
export interface Ask { label: string; ask: string }
export type Choice = { label: string; to: string } | Ask;
export interface ChatScreen { id: string; text: string; choices: Choice[] }
export interface GuideStep { title: string; text: string; checklist: string[] }
export interface CompareTab { label: string; text: string }
export interface CalcInput {
  id: string;
  label: string;
  type: 'slider' | 'number' | 'select' | 'toggle';
  min: number;
  max: number;
  step: number;
  value: number;
  unit?: string;
  options: { label: string; value: number }[];
}
export interface CalcOutput { label: string; formula: Formula; unit?: string; decimals: number; id?: string }
export interface ExploreNode { title?: string; text: string; ask?: string }

interface Base { title: string; followUps: Ask[] }
export type ChatApp = Base & (
  | { kind: 'screens'; screens: ChatScreen[] }
  | { kind: 'guide'; steps: GuideStep[] }
  | { kind: 'compare'; tabs: CompareTab[] }
  | { kind: 'calculator'; inputs: CalcInput[]; outputs: CalcOutput[] }
  | { kind: 'explore'; diagram: string; intro?: string; nodes: Record<string, ExploreNode> }
);
export type ChatAppKind = ChatApp['kind'];

const TEXT = 4000;
const LABEL = 160;
const PROMPT = 2000;
const ID = /^[A-Za-z_][\w-]{0,63}$/;
const NAME = /^[A-Za-z_]\w{0,31}$/;

class Invalid extends Error {}
const fail = (): never => { throw new Invalid(); };
const record = (v: unknown): Record<string, unknown> => (!!v && typeof v === 'object' && !Array.isArray(v) ? v as Record<string, unknown> : fail());
const text = (v: unknown, max: number): string => (typeof v === 'string' && v.trim() && v.length <= max ? v : fail());
const optText = (v: unknown, max: number) => (v === undefined ? undefined : text(v, max));
const list = (v: unknown, min: number, max: number): unknown[] => (Array.isArray(v) && v.length >= min && v.length <= max ? v : fail());
const optList = (v: unknown, max: number) => (v === undefined ? [] : list(v, 0, max));
const num = (v: unknown, fallback?: number): number => (v === undefined && fallback !== undefined ? fallback : typeof v === 'number' && Number.isFinite(v) ? v : fail());
const unique = (ids: string[]) => (new Set(ids).size === ids.length ? ids : fail());

const ask = (v: unknown): Ask => { const a = record(v); return { label: text(a.label, LABEL), ask: text(a.ask, PROMPT) }; };

function screens(v: Record<string, unknown>) {
  const items = list(v.screens, 1, 40).map(raw => {
    const s = record(raw);
    const choices = optList(s.choices, 10).map((c): Choice => {
      const r = record(c);
      return r.ask !== undefined ? ask(r) : { label: text(r.label, LABEL), to: text(r.to, 64) };
    });
    return { id: text(s.id, 64), text: text(s.text, TEXT), choices };
  });
  const ids = new Set(unique(items.map(s => s.id)));
  for (const s of items) for (const c of s.choices) if ('to' in c && !ids.has(c.to)) fail();
  return { kind: 'screens' as const, screens: items };
}

function guide(v: Record<string, unknown>) {
  const steps = list(v.steps, 1, 30).map(raw => {
    const s = record(raw);
    return { title: text(s.title, LABEL), text: text(s.text, TEXT), checklist: optList(s.checklist, 12).map(c => text(c, 300)) };
  });
  return { kind: 'guide' as const, steps };
}

function compare(v: Record<string, unknown>) {
  const tabs = list(v.tabs, 2, 8).map(raw => { const t = record(raw); return { label: text(t.label, 60), text: text(t.text, TEXT) }; });
  return { kind: 'compare' as const, tabs };
}

function calculator(v: Record<string, unknown>) {
  const inputs = list(v.inputs, 1, 12).map((raw): CalcInput => {
    const i = record(raw);
    const id = text(i.id, 32);
    if (!NAME.test(id)) fail();
    const type = i.type ?? 'slider';
    const unit = optText(i.unit, 24);
    const label = text(i.label, LABEL);
    if (type === 'toggle') return { id, label, type, min: 0, max: 1, step: 1, value: i.value ? 1 : 0, unit, options: [] };
    if (type === 'select') {
      const options = list(i.options, 2, 12).map(o => { const r = record(o); return { label: text(r.label, 80), value: num(r.value) }; });
      const value = num(i.value, options[0].value);
      if (!options.some(o => o.value === value)) fail();
      return { id, label, type, min: 0, max: 0, step: 0, value, unit, options };
    }
    if (type !== 'slider' && type !== 'number') fail();
    const min = num(i.min, 0);
    const max = num(i.max, type === 'slider' ? 100 : Number.MAX_SAFE_INTEGER);
    const step = num(i.step, 1);
    const value = num(i.value, min);
    if (!(min < max) || !(step > 0) || value < min || value > max || (type === 'slider' && !Number.isFinite(max))) fail();
    return { id, label, type: type as 'slider' | 'number', min, max, step, value, unit, options: [] };
  });
  const known = new Set(unique(inputs.map(i => i.id)));
  const outputs = list(v.outputs, 1, 8).map((raw): CalcOutput => {
    const o = record(raw);
    let formula: Formula;
    try { formula = parseFormula(text(o.formula, 400)); } catch { return fail(); }
    // Each output may use the inputs and the outputs before it.
    for (const name of formulaNames(formula)) if (!known.has(name)) fail();
    const id = optText(o.id, 32);
    if (id !== undefined) {
      if (!NAME.test(id) || known.has(id)) fail();
      known.add(id);
    }
    const decimals = num(o.decimals, 2);
    if (!Number.isInteger(decimals) || decimals < 0 || decimals > 6) fail();
    return { id, label: text(o.label, LABEL), formula, unit: optText(o.unit, 24), decimals };
  });
  return { kind: 'calculator' as const, inputs, outputs };
}

function explore(v: Record<string, unknown>) {
  const diagram = text(v.diagram, 8000);
  const raw = record(v.nodes);
  const keys = Object.keys(raw);
  if (!keys.length || keys.length > 40) fail();
  const nodes: Record<string, ExploreNode> = Object.create(null);
  for (const key of keys) {
    if (!ID.test(key)) fail();
    const n = record(raw[key]);
    nodes[key] = { title: optText(n.title, LABEL), text: text(n.text, TEXT), ask: optText(n.ask, PROMPT) };
  }
  return { kind: 'explore' as const, diagram, intro: optText(v.intro, TEXT), nodes };
}

const KINDS = { screens, guide, compare, calculator, explore };

/** The card described by `source`, or `null` when it is not a valid `dial-app` block. */
export function parseChatApp(source: string): ChatApp | null {
  if (source.length > 24000) return null;
  let value: unknown;
  try { value = JSON.parse(source); } catch { return null; }
  try {
    const v = record(value);
    const kind = v.kind ?? 'screens';
    if (typeof kind !== 'string' || !Object.hasOwn(KINDS, kind)) return null;
    const body = KINDS[kind as ChatAppKind](v);
    return { title: text(v.title, 120), followUps: optList(v.followUps, 4).map(ask), ...body };
  } catch (error) {
    if (error instanceof Invalid) return null;
    throw error;
  }
}

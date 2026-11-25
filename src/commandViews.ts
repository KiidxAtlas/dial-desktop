// Readable views of engine replies for the command panel: `/cost`, `/context`, `/status` and
// `/collab status` as titled rows instead of a key/value dump. The raw reply stays behind "Raw".

import { count, usd } from "./engineTools";
import type { ResultRow } from "./commandRunner";
import type { ContextView } from "./types";

export interface View { text?: string; rows?: ResultRow[] }

const num = (v: unknown) => (typeof v === "number" && Number.isFinite(v) ? v : 0);
const plural = (n: number, word: string) => `${count(n)} ${word}${n === 1 ? "" : "s"}`;
const word = (v: unknown) => (typeof v === "string" ? v : v == null ? "" : JSON.stringify(v));

/** One usage counter set (`llm::usage::Counts`) as a row. */
function usageRow(label: string, c: any): ResultRow {
  const cost = c?.observed_cost_usd == null ? "not priced" : usd(c.observed_cost_usd);
  const parts = [
    plural(num(c?.requests), "request"),
    `${count(num(c?.input))} in`,
    `${count(num(c?.output))} out`,
    `${count(num(c?.cache_read))} from cache`,
  ];
  if (num(c?.reasoning)) parts.push(`${count(num(c.reasoning))} reasoning`);
  if (num(c?.unpriced_requests)) parts.push(`${count(num(c.unpriced_requests))} without a price`);
  return { title: `${label} · ${cost}`, detail: parts.join(" · ") };
}

/** A model's main and worker counters added together. */
function both(m: any) {
  const a = m?.main ?? {}, b = m?.workers ?? {};
  const sum = (key: string) => num(a[key]) + num(b[key]);
  const priced = a.observed_cost_usd != null || b.observed_cost_usd != null;
  return {
    requests: sum("requests"), input: sum("input"), output: sum("output"), cache_read: sum("cache_read"),
    reasoning: sum("reasoning"), unpriced_requests: sum("unpriced_requests"),
    observed_cost_usd: priced ? num(a.observed_cost_usd) + num(b.observed_cost_usd) : null,
  };
}

/** `/cost`: what this session spent, then the latest turn, workers and each model. */
export function costView(r: any): View {
  const total = r?.usage?.total;
  if (!total?.all) return { text: `Observed session cost: ${usd(r?.observed_cost ?? r?.cost)}` };
  const rows = [usageRow("This session", total.all)];
  if (num(r.usage.turn?.all?.requests)) rows.push(usageRow("Latest turn", r.usage.turn.all));
  if (num(total.workers?.requests)) rows.push(usageRow("Workers", total.workers));
  const models = Object.entries(total.by_model ?? {});
  if (models.length > 1 || num(total.workers?.requests)) for (const [model, counts] of models) rows.push(usageRow(model, both(counts)));
  return { text: "Spend Dial observed for priced requests since this session opened. It is not an invoice total.", rows };
}

/** `/context`: how full the conversation is and what it has sent so far. */
export function contextView(r: any, context: ContextView | null): View {
  const m = r?.metrics ?? {};
  const rows: ResultRow[] = [];
  if (context) {
    const share = context.window ? ` of ${count(context.window)} (${Math.min(100, Math.round((context.tokens / context.window) * 100))}%)` : "";
    rows.push({ title: `In context · ${count(context.tokens)} tokens${share}`, detail: context.estimated ? "Estimated until the model reports its usage." : undefined });
  }
  rows.push({
    title: `Conversation · ${plural(num(r?.messages), "message")}`,
    detail: num(m.compactions) ? `Compacted ${num(m.compactions) === 1 ? "once" : `${count(m.compactions)} times`} so far.` : "Not compacted yet.",
  });
  const tokens = [`${count(num(m.prompt_tokens))} in`, `${count(num(m.output_tokens))} out`, `${count(num(m.cached_tokens))} read from cache`];
  if (num(m.cache_write_tokens)) tokens.push(`${count(num(m.cache_write_tokens))} written to cache`);
  if (num(m.reasoning_tokens)) tokens.push(`${count(num(m.reasoning_tokens))} reasoning`);
  rows.push({ title: `Model replies · ${count(num(m.responses))}`, detail: tokens.join(" · ") });
  return { rows };
}

/** `/status`: the session, its model, its limits and its folders. */
export function statusView(r: any): View {
  const model = r?.current_model ?? {};
  const thinking = model.thinking?.level ?? model.thinking?.default_level;
  const tools: string[] = Array.isArray(r?.tools) ? r.tools : [];
  const dirs: string[] = Array.isArray(r?.workspace?.dirs) ? r.workspace.dirs.map(word) : [];
  const rows: ResultRow[] = [
    { title: `Session · ${r?.title || "Untitled"}`, detail: word(r?.session_id) || undefined },
    {
      title: `Model · ${model.model_id ? `${model.provider}/${model.model_id}` : "none chosen"}`,
      detail: thinking ? `Reasoning: ${thinking}` : undefined,
    },
    { title: `Approval · ${word(r?.approval) || "unknown"}`, detail: r?.plan_mode ? "Plan mode is on: Dial plans before it changes anything." : "Plan mode is off." },
    {
      title: `Tools · ${count(tools.length)} available`,
      detail: tools.length ? `${tools.slice(0, 14).join(", ")}${tools.length > 14 ? ` and ${tools.length - 14} more` : ""}` : undefined,
    },
    { title: `Project · ${word(r?.workspace?.root) || "unknown"}`, detail: dirs.length ? `Also: ${dirs.join(", ")}` : "No extra folders." },
    { title: `Observed cost · ${usd(r?.observed_cost ?? r?.cost)}`, detail: "Priced requests this session; not an invoice total." },
  ];
  return { rows };
}

/** `/collab status`: the room this session hosts or joined, and rooms hosted on this computer. */
export function collabView(r: any): View {
  const room = r?.room ?? {};
  const access = (readOnly: unknown) => (readOnly ? "guests can read only" : "guests can steer");
  const rows: ResultRow[] = [];
  let text: string | undefined;
  if (room.role === "host") {
    const peers: any[] = Array.isArray(room.peers) ? room.peers : [];
    rows.push({ title: `Hosting a room · ${access(room.read_only)}`, detail: peers.length ? `${plural(peers.length, "guest")} connected.` : "No guests yet. Share the link with /collab invite." });
    for (const p of peers) rows.push({ title: word(p.name) || word(p.id), detail: p.writable ? "Can steer" : "Reads only" });
  } else if (room.role === "guest") {
    const state = room.ended ? "The host ended the room." : room.admitted ? `${plural(num(room.replicated_entries), "entry").replace(/entrys$/, "entries")} received.` : "Waiting for the host to let you in.";
    rows.push({ title: `Joined as a guest · ${room.read_only ? "read only" : "you can steer"}`, detail: state });
    if (room.question) rows.push({ title: "The host is asking a question", detail: "Answer with /collab answer <option number>." });
  } else {
    text = room.joining ? "Joining a room…" : "No room is open. Host this session with /collab start, or join one with /collab join <link>.";
  }
  for (const h of Array.isArray(r?.local_hosts) ? r.local_hosts : [])
    rows.push({
      title: `On this computer · ${word(h.model) || "session"}${h.read_only ? " · read only" : ""}`,
      detail: [word(h.cwd), plural(num(h.guests), "guest")].filter(Boolean).join(" · "),
    });
  const notices = (Array.isArray(room.notices) ? room.notices : []).map(word).filter(Boolean);
  if (notices.length) text = [text, ...notices].filter(Boolean).join("\n");
  return { text, rows };
}

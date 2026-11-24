// Custom model endpoints (`providers.<id>` in the engine config) and the "Test connection"
// result: pure checks and wording, so the Providers page stays a thin view.

/** Wire protocols a self-hosted or proxy endpoint usually speaks (`ApiProtocol`, kebab-case). */
export const ENDPOINT_PROTOCOLS = [
  { value: "chat-completions", label: "OpenAI-compatible (Chat Completions)" },
  { value: "responses", label: "OpenAI Responses" },
  { value: "anthropic", label: "Anthropic Messages" },
  { value: "gemini", label: "Google Gemini" },
  { value: "ollama", label: "Ollama native" },
] as const;

export interface EndpointForm {
  id: string;
  label: string;
  baseUrl: string;
  protocol: string;
  apiKeyEnv: string;
  allowInsecure: boolean;
}

export interface EndpointCheck {
  /** Per-field problems that block saving. */
  errors: Partial<Record<"id" | "baseUrl" | "apiKeyEnv" | "protocol", string>>;
  /** Saving works, but the endpoint may not. */
  warnings: string[];
}

/** Mirrors the engine's `is_local_host`: plain HTTP is allowed there without `allow_insecure`. */
export function isLocalHost(host: string): boolean {
  const h = host.replace(/^\[|\]$/g, "").toLowerCase();
  const v4 = h.match(/^(\d{1,3})\.(\d{1,3})\.(\d{1,3})\.(\d{1,3})$/);
  if (v4) {
    const [a, b] = [Number(v4[1]), Number(v4[2])];
    return a === 127 || a === 10 || (a === 172 && b >= 16 && b <= 31) || (a === 192 && b === 168) || (a === 169 && b === 254);
  }
  if (h.includes(":")) return h === "::1" || /^f[cd]/.test(h) || /^fe[89ab]/.test(h);
  return h === "localhost" || h.endsWith(".localhost") || h.endsWith(".local") || h.endsWith(".docker.internal");
}

/** Validate the "Add custom endpoint" form against the providers Dial already knows. */
export function checkEndpoint(form: EndpointForm, existing: Iterable<string>): EndpointCheck {
  const errors: EndpointCheck["errors"] = {};
  const warnings: string[] = [];
  const id = form.id.trim();
  const taken = new Set([...existing].map((p) => p.toLowerCase()));
  if (!id) errors.id = "Give the endpoint a short id, such as my-proxy.";
  else if (!/^[a-z0-9-]+$/.test(id)) errors.id = "Use lowercase letters, digits and dashes only.";
  else if (taken.has(id)) errors.id = `“${id}” is already a provider. Pick another id.`;

  const raw = form.baseUrl.trim();
  let url: URL | null = null;
  try {
    url = raw ? new URL(raw) : null;
  } catch {
    url = null;
  }
  if (!raw) errors.baseUrl = "Enter the server address, such as http://localhost:11434/v1.";
  else if (!url || (url.protocol !== "http:" && url.protocol !== "https:") || !url.hostname) errors.baseUrl = "Start with http:// or https:// and include a host.";
  else if (url.protocol === "http:" && !isLocalHost(url.hostname) && !form.allowInsecure) {
    warnings.push(`Plain http:// to ${url.hostname} is refused unless you allow insecure connections (allow_insecure). Use https:// or tick “Allow plain HTTP”.`);
  }

  if (!ENDPOINT_PROTOCOLS.some((p) => p.value === form.protocol)) errors.protocol = "Pick a protocol.";
  const env = form.apiKeyEnv.trim();
  if (env && !/^[A-Za-z_][A-Za-z0-9_]*$/.test(env)) errors.apiKeyEnv = "A variable name uses letters, digits and underscores, such as MY_PROXY_KEY.";
  return { errors, warnings };
}

/** The `providers.<id>` table saved for a valid form; empty fields are left out. */
export function endpointTable(form: EndpointForm): Record<string, unknown> {
  const table: Record<string, unknown> = {
    label: form.label.trim() || form.id.trim(),
    base_url: form.baseUrl.trim().replace(/\/+$/, ""),
    protocol: form.protocol,
  };
  if (form.apiKeyEnv.trim()) table.api_key_env = form.apiKeyEnv.trim();
  if (form.allowInsecure) table.allow_insecure = true;
  return table;
}

/** A saved `providers.<id>` table as the form shows it, for editing. */
export function endpointForm(id: string, spec: Record<string, unknown>): EndpointForm {
  const text = (v: unknown) => (typeof v === "string" ? v : "");
  return {
    id,
    label: text(spec.label) === id ? "" : text(spec.label),
    baseUrl: text(spec.base_url),
    // A protocol this form does not list is kept as saved (the page offers it as an option).
    protocol: text(spec.protocol) || "chat-completions",
    apiKeyEnv: text(spec.api_key_env),
    allowInsecure: spec.allow_insecure === true,
  };
}

/** The table saved for an edited endpoint: the form's fields over the saved table, so keys the
 *  form does not show (headers, model lists) survive, and optional fields cleared in it go. */
export function editedTable(saved: Record<string, unknown>, form: EndpointForm): Record<string, unknown> {
  const table: Record<string, unknown> = { ...saved, ...endpointTable(form) };
  if (!form.apiKeyEnv.trim()) delete table.api_key_env;
  if (!form.allowInsecure) delete table.allow_insecure;
  return table;
}

/** `list_models` with `refresh: true`: the parts a connection test reads. */
export interface ProviderStatus {
  id: string;
  label?: string;
  base_url?: string | null;
  state: string;
  detail?: string;
  models?: number;
}
export interface ListedReply {
  models?: { provider?: string; sources?: string[] }[];
  checked?: { provider_status?: ProviderStatus[] } | null;
}
export interface TestResult {
  ok: boolean;
  text: string;
}

const OK_STATES = new Set(["ok", "ready", "reachable", "cached"]);
const STATE_TEXT: Record<string, string> = {
  "auth-required": "Needs an API key or sign-in",
  "not-set-up": "Not set up",
  unavailable: "Could not connect",
  disabled: "Turned off in the configuration",
};

const plural = (n: number) => `${n} model${n === 1 ? "" : "s"}`;

/** What a connection test found for `provider`: the engine's probe status when it reports one
 *  (newer engines), else a count of the models listed for it. */
export function testResult(reply: ListedReply, provider: string): TestResult {
  // Discovery can report one id twice (a built-in and a configured record); the healthiest wins.
  const rank = (s: ProviderStatus) => (OK_STATES.has(s.state) ? 2 : 0) + (Number(s.models ?? 0) > 0 ? 1 : 0);
  const status = (reply.checked?.provider_status ?? [])
    .filter((s) => s.id === provider)
    .reduce<ProviderStatus | undefined>((best, s) => (!best || rank(s) > rank(best) ? s : best), undefined);
  if (!status && Array.isArray(reply.checked?.provider_status) && !(reply.models ?? []).some((m) => m.provider === provider)) {
    return { ok: false, text: "Dial did not check this provider. If you just added it, restart the engine (Settings › Updates) and test again." };
  }
  if (status) {
    const detail = status.detail?.trim();
    const models = Number(status.models ?? 0);
    if (OK_STATES.has(status.state)) {
      return models > 0
        ? { ok: true, text: `✓ Connected · ${plural(models)}` }
        : { ok: true, text: `Connected, but no models were listed${detail ? `: ${detail}` : ""}` };
    }
    const head = STATE_TEXT[status.state] ?? status.state;
    return { ok: false, text: detail ? `${head}: ${detail}` : head };
  }
  // A catalog-only row is a model Dial knows of, not one the server answered with.
  const found = (reply.models ?? []).filter((m) => m.provider === provider && !(m.sources?.length && m.sources.every((s) => s === "catalog"))).length;
  return found > 0 ? { ok: true, text: `✓ ${plural(found)}` } : { ok: false, text: "No models found" };
}

/** The `list_models` provider id behind a Providers-page entry: an API key's `api-key:openai`
 *  is `openai`; the ChatGPT account (`openai` in `auth_providers`) lists as `chatgpt`. */
export function testProviderId(entry: string): string {
  if (entry.startsWith("api-key:")) return entry.slice("api-key:".length);
  return entry === "openai" ? "chatgpt" : entry;
}

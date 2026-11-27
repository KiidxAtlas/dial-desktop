// Pure helpers for the Hooks & SSH page: turning what the user typed into the values the engine
// saves (`hooks`, `ssh_hosts.<name>`) and spotting an engine too old to serve the page.

export interface Hook {
  event: string;
  matcher?: string | null;
  command: string[];
  timeout_secs: number;
  fail_closed?: boolean | null;
  /** Effective mode the engine reports; never saved. */
  fails_closed?: boolean;
}

export interface SshHost {
  name: string;
  host: string;
  user?: string | null;
  port?: number | null;
  key?: string | null;
  origin: "dial" | "ssh_config";
  source?: string | null;
}

/** Split a command line into program and arguments; quotes group words, nothing is expanded. */
export function splitCommand(line: string): string[] {
  const out: string[] = [];
  let word = "";
  let quote: '"' | "'" | null = null;
  let started = false;
  for (let i = 0; i < line.length; i++) {
    const c = line[i];
    if (quote) {
      if (c === quote) quote = null;
      else if (c === "\\" && quote === '"' && (line[i + 1] === '"' || line[i + 1] === "\\")) word += line[++i];
      else word += c;
    } else if (c === '"' || c === "'") {
      quote = c;
      started = true;
    } else if (/\s/.test(c)) {
      if (started) out.push(word);
      word = "";
      started = false;
    } else {
      word += c;
      started = true;
    }
  }
  if (quote) throw new Error("A quote is not closed");
  if (started) out.push(word);
  return out;
}

/** Show a saved command the way `splitCommand` reads it back. */
export function joinCommand(parts: string[]): string {
  return parts.map((p) => (p !== "" && /^[^\s"'\\]+$/.test(p) ? p : `"${p.replace(/["\\]/g, (m) => `\\${m}`)}"`)).join(" ");
}

/** A hook as the configuration stores it: only the fields `[[hooks]]` accepts. */
export function savedHook(hook: Hook): Record<string, unknown> {
  const out: Record<string, unknown> = { event: hook.event, command: hook.command, timeout_secs: hook.timeout_secs };
  if (hook.matcher) out.matcher = hook.matcher;
  if (typeof hook.fail_closed === "boolean") out.fail_closed = hook.fail_closed;
  return out;
}

export interface HookDraft { event: string; matcher: string; command: string; timeout: string; failure: "default" | "closed" | "open" }

/** Validate a new hook; throws a sentence the form shows. */
export function hookFromDraft(draft: HookDraft, events: string[]): Hook {
  if (!events.includes(draft.event)) throw new Error("Choose when the hook runs");
  const command = splitCommand(draft.command.trim());
  if (!command.length || !command[0]) throw new Error("Enter the program to run");
  const timeout = draft.timeout.trim() === "" ? 30 : Number(draft.timeout);
  if (!Number.isInteger(timeout) || timeout < 0 || timeout > 86400) throw new Error("Timeout is whole seconds from 0 to 86400 (0 means no limit)");
  const hook: Hook = { event: draft.event, command, timeout_secs: timeout };
  if (draft.matcher.trim()) hook.matcher = draft.matcher.trim();
  if (draft.failure !== "default") hook.fail_closed = draft.failure === "closed";
  return hook;
}

export interface SshDraft { name: string; host: string; user: string; port: string; key: string }

/** Validate a new SSH host; returns the saved name and table, or throws a sentence. */
export function sshFromDraft(draft: SshDraft, taken: string[]): { name: string; value: Record<string, unknown> } {
  const name = draft.name.trim();
  if (!/^[A-Za-z][A-Za-z0-9_-]*$/.test(name)) throw new Error("Names start with a letter, then letters, digits, - or _");
  if (taken.includes(name)) throw new Error(`${name} already exists; remove it first`);
  const host = draft.host.trim();
  if (!host || /\s/.test(host)) throw new Error("Enter the host name or address");
  const value: Record<string, unknown> = { host };
  if (draft.user.trim()) value.user = draft.user.trim();
  if (draft.port.trim()) {
    const port = Number(draft.port);
    if (!Number.isInteger(port) || port < 1 || port > 65535) throw new Error("Port is a number from 1 to 65535");
    value.port = port;
  }
  if (draft.key.trim()) value.key = draft.key.trim();
  return { name, value };
}

/** Whether an error means the running engine predates a method (the client refuses it locally). */
export function engineTooOld(error: unknown): boolean {
  return /does not serve `|unknown method|method not found/i.test(String(error instanceof Error ? error.message : error));
}

/** Change only computer use in the layer that supplies the effective integrations list. */
export function computerSetting(current: { value: unknown; scope?: "global" | "project" }, on: boolean) {
  if (!Array.isArray(current.value) || !current.value.every((name): name is string => typeof name === "string")) {
    throw new Error("Could not read the integrations list; no settings were changed.");
  }
  const value = current.value.filter((name) => name !== "computer");
  if (on) value.push("computer");
  return { key: "integrations", scope: current.scope ?? "global", value };
}

/** Only retry reads refused before admission, never writes or a generic busy/closing error. */
export async function retryIntegrationRead<T>(read: () => Promise<T>, current: () => boolean): Promise<T> {
  for (let attempt = 0; ; attempt++) {
    if (!current()) throw new Error("Settings page changed");
    try {
      return await read();
    } catch (error) {
      if (attempt >= 5 || !/\b\d+ reads are already running; retry when one finishes\b/.test(String(error))) throw error;
      await new Promise((resolve) => setTimeout(resolve, 100 * 2 ** attempt));
    }
  }
}

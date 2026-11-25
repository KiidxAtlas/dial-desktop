import test from "node:test";
import assert from "node:assert/strict";
import { buildExport, exportable, importPlan, isSecretKey, parseImport, preview, scrub } from "./settingsTransfer.ts";

const row = (key, value, extra = {}) => ({ key, label: key, description: "", tab: "Common", section: "", control: { type: "text" }, value, default: null, modified: true, redacted: false, restart: false, ...extra });

test("credential names are recognised without catching ordinary settings", () => {
  for (const [key, secret] of [
    ["providers.lab.api_key", true],
    ["providers.lab.apiKey", true],
    ["github.token", true],
    ["mcp.x.client_secret", true],
    ["headers.Proxy-Authorization", true],
    ["headers.Set-Cookie", true],
    ["proxy.password", true],
    ["providers.lab.api_key_env", false],
    ["model.max_tokens", false],
    ["voice.push_to_talk_key", false],
    ["ui.keys", false],
  ]) assert.equal(isSecretKey(key), secret, key);
});

test("export keeps each layer's values and never a credential", () => {
  const rows = [
    row("model", "anthropic/claude", { source: "global" }),
    row("approval", "ask", { source: "project" }),
    row("github.token", "ghp_x", { source: "global" }),
    row("search.key", "••••", { source: "global", redacted: true }),
    row("providers", { lab: { base_url: "http://x", api_key: "sk", api_key_env: "LAB", headers: { authorization: "Bearer y" } } }, { source: "global" }),
    row("providers.lab.base_url", "http://x", { source: "global" }),
    row("sandbox.mode", "dev", { modified: false }),
  ];
  const file = buildExport(rows, true, new Date(0));
  assert.deepEqual(file, {
    format: "dial-settings", version: 1, exported: "1970-01-01T00:00:00.000Z",
    settings: { model: "anthropic/claude", providers: { lab: { base_url: "http://x", api_key_env: "LAB", headers: {} } } },
    project: { approval: "ask" },
  });
  assert.equal(buildExport(rows, false).project, undefined);
  // Engines that do not report layers: every changed value, as the everywhere layer.
  const plain = [row("model", "m"), row("approval", "ask", { modified: false })];
  assert.deepEqual(exportable(plain, "global"), { model: "m" });
  assert.deepEqual(exportable(plain, "project"), {});
  assert.equal(scrub("********"), undefined);
});

test("import reads exports and plain tables, and rejects anything else", () => {
  assert.deepEqual(parseImport(JSON.stringify({ format: "dial-settings", version: 1, settings: { a: 1 }, project: { b: 2 } })), { settings: { a: 1 }, project: { b: 2 } });
  assert.deepEqual(parseImport('{"model":"x"}'), { settings: { model: "x" }, project: null });
  for (const bad of ["nope", "[1]", '"x"', '{"format":"dial-settings","settings":[]}']) assert.throws(() => parseImport(bad), /settings/, bad);
});

test("import plan shows changes, equal values, unknown keys and skipped credentials", () => {
  const rows = [row("model", "a"), row("approval", "ask"), row("search.key", "••", { redacted: true })];
  const plan = importPlan({ model: "b", approval: "ask", "providers.lab": { base_url: "u" }, "search.key": "k", "x.token": "t" }, rows);
  assert.deepEqual(plan.map((p) => [p.key, p.kind]), [["model", "change"], ["approval", "same"], ["providers.lab", "new"], ["search.key", "secret"], ["x.token", "secret"]]);
  assert.equal(preview({ a: "x".repeat(80) }, 10), '{"a":"xxx…');
  assert.equal(preview(undefined), "—");
});

test("import never previews or replaces a credential inside a setting", () => {
  const safe = { base_url: "http://localhost", api_key_env: "LAB" };
  for (const [key, incoming, rows] of [
    ["providers.lab", { ...safe, headers: { "X-Api-Key": "private" } }, []],
    ["proxies", [{ options: { accessToken: "private" } }], []],
    ["proxies", [{ opaque: "<redacted>" }], []],
    ["providers.lab", safe, [row("providers.lab", { token: "private" })]],
    ["providers.lab", safe, [row("providers", { lab: { token: "private" } })]],
    ["providers.lab", safe, [row("providers.lab.opaque", "private", { redacted: true })]],
    ["providers.lab.opaque.value", "private", [row("providers.lab.opaque", {}, { redacted: true })]],
    ["credentials.value", "private", []],
  ]) {
    const [item] = importPlan({ [key]: incoming }, rows);
    assert.equal(item.kind, "secret", key);
    assert.equal(item.current, undefined);
    assert.equal(item.value, undefined);
  }
  const [allowed] = importPlan({ "providers.labs": safe }, [row("providers.lab.token", "private")]);
  assert.equal(allowed.kind, "new");
  assert.deepEqual(allowed.value, safe);
});

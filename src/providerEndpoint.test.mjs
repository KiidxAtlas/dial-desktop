import test from "node:test";
import assert from "node:assert/strict";
import { checkEndpoint, editedTable, endpointForm, endpointTable, isLocalHost, testProviderId, testResult } from "./providerEndpoint.ts";

const form = { id: "my-proxy", label: "", baseUrl: "https://proxy.example.com/v1/", protocol: "chat-completions", apiKeyEnv: "", allowInsecure: false };

test("endpoint form validation", () => {
  for (const [patch, field] of [
    [{ id: "" }, "id"],
    [{ id: "My Proxy" }, "id"],
    [{ id: "openai" }, "id"],
    [{ baseUrl: "" }, "baseUrl"],
    [{ baseUrl: "ftp://x" }, "baseUrl"],
    [{ baseUrl: "localhost:11434" }, "baseUrl"],
    [{ protocol: "smoke-signals" }, "protocol"],
    [{ apiKeyEnv: "MY-KEY" }, "apiKeyEnv"],
  ]) {
    const { errors } = checkEndpoint({ ...form, ...patch }, ["openai", "anthropic"]);
    assert.deepEqual(Object.keys(errors), [field], JSON.stringify(patch));
  }
  assert.deepEqual(checkEndpoint(form, ["openai"]), { errors: {}, warnings: [] });
});

test("plain http warns only for non-local hosts without allow_insecure", () => {
  for (const [url, insecure, warns] of [
    ["http://localhost:11434/v1", false, false],
    ["http://127.0.0.1:1234/v1", false, false],
    ["http://192.168.1.5:8080", false, false],
    ["http://[::1]:8080", false, false],
    ["http://gpu.example.com/v1", false, true],
    ["http://gpu.example.com/v1", true, false],
    ["https://gpu.example.com/v1", false, false],
  ]) {
    const { errors, warnings } = checkEndpoint({ ...form, baseUrl: url, allowInsecure: insecure }, []);
    assert.deepEqual(errors, {}, url);
    assert.equal(warnings.length > 0, warns, url);
  }
  assert.equal(isLocalHost("lab.local"), true);
  assert.equal(isLocalHost("172.32.0.1"), false);
});

test("saved table drops empty fields and trailing slashes", () => {
  assert.deepEqual(endpointTable(form), { label: "my-proxy", base_url: "https://proxy.example.com/v1", protocol: "chat-completions" });
  assert.deepEqual(endpointTable({ ...form, label: " Proxy ", apiKeyEnv: "PROXY_KEY", allowInsecure: true }), {
    label: "Proxy", base_url: "https://proxy.example.com/v1", protocol: "chat-completions", api_key_env: "PROXY_KEY", allow_insecure: true,
  });
});

test("connection test wording from provider_status, with a model-count fallback", () => {
  const status = (state, detail = "", models = 0) => ({ models: [], checked: { provider_status: [{ id: "p", state, detail, models }] } });
  for (const [reply, ok, text] of [
    [status("ok", "", 3), true, "✓ Connected · 3 models"],
    [status("reachable", "Listing empty", 0), true, "Connected, but no models were listed: Listing empty"],
    [status("auth-required", "HTTP 401"), false, "Needs an API key or sign-in: HTTP 401"],
    [status("unavailable", "Connection refused"), false, "Could not connect: Connection refused"],
    [status("not-set-up"), false, "Not set up"],
    [status("disabled"), false, "Turned off in the configuration"],
    [status("weird", "x"), false, "weird: x"],
    // Older engines: no `checked.provider_status`; catalog-only rows do not count.
    [{ models: [{ provider: "p", sources: ["listed"] }, { provider: "p" }, { provider: "p", sources: ["catalog"] }, { provider: "q" }] }, true, "✓ 2 models"],
    [{ models: [{ provider: "q" }], checked: { providers: 1 } }, false, "No models found"],
    [{}, false, "No models found"],
    // Newer engine that did not probe this id (not applied yet).
    [{ models: [], checked: { provider_status: [{ id: "q", state: "ok" }] } }, false, /did not check/],
    // One id reported twice: the healthy record wins over the failed one.
    [{ models: [], checked: { provider_status: [{ id: "p", state: "unavailable", models: 0 }, { id: "p", state: "ok", models: 4 }] } }, true, "✓ Connected · 4 models"],
  ]) {
    const got = testResult(reply, "p");
    assert.equal(got.ok, ok);
    if (text instanceof RegExp) assert.match(got.text, text);
    else assert.equal(got.text, text);
  }
  assert.equal(testProviderId("api-key:openai"), "openai");
  assert.equal(testProviderId("openai"), "chatgpt");
  assert.equal(testProviderId("anthropic"), "anthropic");
});

test("editing keeps keys the form does not show and drops cleared optional ones", () => {
  const saved = { label: "Lab", base_url: "http://gpu.example.com/v1", protocol: "bedrock", api_key_env: "LAB_KEY", allow_insecure: true, headers: { "x-team": "ml" } };
  const edit = endpointForm("lab", saved);
  assert.deepEqual(edit, { id: "lab", label: "Lab", baseUrl: "http://gpu.example.com/v1", protocol: "bedrock", apiKeyEnv: "LAB_KEY", allowInsecure: true });
  assert.equal(endpointForm("lab", { label: "lab", base_url: "x" }).label, "", "a label equal to the id reads as unset");
  assert.deepEqual(editedTable(saved, { ...edit, baseUrl: "https://gpu.example.com/v2/", apiKeyEnv: " ", allowInsecure: false }), {
    label: "Lab", base_url: "https://gpu.example.com/v2", protocol: "bedrock", headers: { "x-team": "ml" },
  });
});

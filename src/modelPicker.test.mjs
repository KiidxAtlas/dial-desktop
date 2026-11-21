import test from "node:test";
import assert from "node:assert/strict";
import { buildPickerGroups, pickerProviders, usableModels } from "./modelPicker.ts";

function row(overrides) {
  return {
    provider: "anthropic",
    modelId: "claude-sonnet",
    label: "Claude Sonnet",
    contextWindow: 200000,
    reasoning: true,
    vision: true,
    configured: false,
    local: false,
    listed: false,
    catalogOnly: false,
    ...overrides,
  };
}

test("usableModels drops disabled catalog-only providers, stale bundled rows, and account-scoped bundled IDs", () => {
  const models = [
    // Disabled provider with only static catalog rows (e.g. from older engine build)
    row({ provider: "copilot", modelId: "gpt-4o", catalogOnly: true }),
    // Account-scoped provider with stale bundled row + real listed row
    row({ provider: "google-antigravity", modelId: "claude-opus-5-5", catalogOnly: true }),
    row({ provider: "google-antigravity", modelId: "gemini-3-pro", listed: true }),
    // Provider with live listing + stale bundled row
    row({ provider: "anthropic", modelId: "claude-3-haiku-20240307", catalogOnly: true }),
    row({ provider: "anthropic", modelId: "claude-sonnet-4-6", listed: true }),
    // Current unconfigured model is always preserved
    row({ provider: "chatgpt", modelId: "gemini-4-neon", catalogOnly: true }),
    // Local downloaded model is always preserved
    row({ provider: "mlx", modelId: "qwen-4bit", local: true }),
  ];

  const clean = usableModels(
    models,
    (m) => m.provider === "chatgpt" && m.modelId === "gemini-4-neon",
  );
  const ids = clean.map((m) => `${m.provider}/${m.modelId}`);
  assert.deepEqual(ids, [
    "google-antigravity/gemini-3-pro",
    "anthropic/claude-sonnet-4-6",
    "chatgpt/gemini-4-neon",
    "mlx/qwen-4bit",
  ]);
});

test("buildPickerGroups collapses large providers by default, expands on search or provider filter", () => {
  const openrouter = Array.from({ length: 20 }, (_, i) =>
    row({
      provider: "openrouter",
      modelId: `vendor/model-${i}`,
      label: `Model ${i}`,
      listed: true,
    }),
  );
  const models = [
    row({ provider: "anthropic", modelId: "claude-main", configured: true, listed: true }),
    row({ provider: "chatgpt", modelId: "gpt-6-astra", listed: true }),
    row({ provider: "mlx", modelId: "local-qwen", local: true }),
    ...openrouter,
  ];
  const isCurrent = (m) => m.provider === "anthropic" && m.modelId === "claude-main";

  // Providers exclude local-only mlx and include counts.
  assert.deepEqual(pickerProviders(models), [
    { provider: "anthropic", count: 1 },
    { provider: "chatgpt", count: 1 },
    { provider: "openrouter", count: 20 },
  ]);

  // Default "all" view keeps Your models, Downloaded, and compact chatgpt inline, collapsing openrouter.
  const browsing = buildPickerGroups(models, { query: "", filter: "all", isCurrent });
  assert.deepEqual(
    browsing.map((g) => ({ title: g.title, shown: g.rows.length, hidden: g.hiddenCount })),
    [
      { title: "Your models", shown: 1, hidden: 0 },
      { title: "Downloaded models", shown: 1, hidden: 0 },
      { title: "chatgpt", shown: 1, hidden: 0 },
      { title: "openrouter", shown: 0, hidden: 20 },
    ],
  );

  // Searching immediately shows matching openrouter rows without collapsing.
  const searched = buildPickerGroups(models, { query: "model-1", filter: "all", isCurrent });
  const orGroup = searched.find((g) => g.provider === "openrouter");
  assert.ok(orGroup);
  assert.equal(orGroup.hiddenCount, 0);
  assert.equal(orGroup.rows.length, 11); // model-1 and model-10..19

  // Selecting the openrouter provider filter shows all 20 rows immediately.
  const filtered = buildPickerGroups(models, { query: "", filter: "openrouter", isCurrent });
  assert.equal(filtered.length, 1);
  assert.equal(filtered[0].rows.length, 20);
  assert.equal(filtered[0].hiddenCount, 0);
});

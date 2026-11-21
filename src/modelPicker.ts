import type { ModelOption } from "./types";

export const DEFAULT_INLINE_LIMIT = 12;

export type PickerFilter = "all" | "downloaded" | (string & {});

export interface PickerGroup {
  title: string;
  provider?: string;
  rows: ModelOption[];
  hiddenCount: number;
}

export interface PickerProvider {
  provider: string;
  count: number;
}

const ACCOUNT_SCOPED_PROVIDERS = new Set(["chatgpt", "google-antigravity"]);

/**
 * Filter out unverified static catalog-only rows that flood the picker with models that do not
 * work:
 * - any catalog-only row for a provider that already has a real live listing,
 * - any catalog-only row for a provider with no configured, local, or listed models at all
 *   (for example disabled or unauthenticated providers from older engine builds),
 * - any catalog-only row for account-scoped subscription providers (ChatGPT, Antigravity).
 */
export function usableModels(
  models: ModelOption[],
  isCurrent: (m: ModelOption) => boolean = () => false,
): ModelOption[] {
  const listedProviders = new Set<string>();
  const activeProviders = new Set<string>();
  for (const m of models) {
    if (m.listed) listedProviders.add(m.provider);
    if (m.listed || m.configured || m.local || isCurrent(m)) {
      activeProviders.add(m.provider);
    }
  }
  return models.filter((m) => {
    if (isCurrent(m) || m.configured || m.local || m.listed) return true;
    if (!m.catalogOnly) return true;
    if (ACCOUNT_SCOPED_PROVIDERS.has(m.provider)) return false;
    if (listedProviders.has(m.provider)) return false;
    return activeProviders.has(m.provider);
  });
}

/** Distinct non-local providers available in the cleaned picker list, sorted alphabetically. */
export function pickerProviders(models: ModelOption[]): PickerProvider[] {
  const counts = new Map<string, number>();
  for (const m of models) {
    if (m.local) continue;
    counts.set(m.provider, (counts.get(m.provider) ?? 0) + 1);
  }
  return Array.from(counts.entries())
    .sort(([a], [b]) => a.localeCompare(b))
    .map(([provider, count]) => ({ provider, count }));
}

export interface BuildGroupsOptions {
  query: string;
  filter: PickerFilter;
  expandedProviders?: ReadonlySet<string>;
  isCurrent: (m: ModelOption) => boolean;
  inlineLimit?: number;
}

/**
 * Group models for the composer picker.
 * - Typing a query always searches across every matching usable model without collapsing.
 * - Selecting a specific provider shows all of that provider's usable models.
 * - The default "all" view keeps Your models, Downloaded models, and compact providers inline,
 *   while collapsing large catalogs (such as OpenRouter) behind a single expand row until
 *   searched, filtered by provider, or explicitly expanded.
 */
export function buildPickerGroups(
  models: ModelOption[],
  options: BuildGroupsOptions,
): PickerGroup[] {
  const { query, filter, isCurrent } = options;
  const expanded = options.expandedProviders ?? new Set<string>();
  const inlineLimit = options.inlineLimit ?? DEFAULT_INLINE_LIMIT;
  const words = query.trim().toLowerCase().split(/\s+/).filter(Boolean);
  const searching = words.length > 0;
  const matches = (m: ModelOption) => {
    if (!searching) return true;
    const text = `${m.label} ${m.modelId} ${m.provider}`.toLowerCase();
    return words.every((w) => text.includes(w));
  };

  if (filter === "downloaded") {
    const local = models.filter((m) => m.local && matches(m));
    return local.length ? [{ title: "Downloaded models", rows: local, hiddenCount: 0 }] : [];
  }

  if (filter !== "all") {
    const rows = models.filter((m) => !m.local && m.provider === filter && matches(m));
    return rows.length ? [{ title: filter, provider: filter, rows, hiddenCount: 0 }] : [];
  }

  const rows = models.filter(matches);
  const local = rows.filter((m) => m.local);
  const mine = rows.filter((m) => !m.local && (m.configured || isCurrent(m)));
  const mineSet = new Set(mine);
  const byProvider = new Map<string, ModelOption[]>();
  for (const m of rows) {
    if (m.local || mineSet.has(m)) continue;
    const list = byProvider.get(m.provider);
    if (list) list.push(m);
    else byProvider.set(m.provider, [m]);
  }

  const out: PickerGroup[] = [];
  if (mine.length) out.push({ title: "Your models", rows: mine, hiddenCount: 0 });
  if (local.length) out.push({ title: "Downloaded models", rows: local, hiddenCount: 0 });
  for (const [provider, list] of byProvider) {
    if (searching || list.length <= inlineLimit || expanded.has(provider)) {
      out.push({ title: provider, provider, rows: list, hiddenCount: 0 });
    } else {
      out.push({ title: provider, provider, rows: [], hiddenCount: list.length });
    }
  }
  return out;
}
